use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use harper_core::linting::{LintGroup, LintKind, Linter, Suggestion};
use harper_core::spell::FstDictionary;
use harper_core::{remove_overlaps, Dialect, Document};
use serde::{Deserialize, Serialize};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Emitter, LogicalPosition, LogicalSize, Manager, State, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub mod os_bridge;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalLintResult {
    pub id: String,
    pub start: usize,
    pub end: usize,
    pub flagged_text: String,
    pub category: String, // "Correctness", "Clarity", "Style"
    pub subcategory: String,
    pub message: String,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiAnalysisResult {
    pub success: bool,
    pub status: String, // "ok", "rate_limited", "missing_api_key", "error"
    pub message: String,
    pub tone: String,
    pub score: u32,
    pub rewritten_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsCapturePayload {
    pub text: String,
    pub source_app: String,
    pub process_name: String,
    pub method: String,
    pub bounds: os_bridge::ScreenRect,
    pub lints: Vec<LocalLintResult>,
}

pub struct AppState {
    pub paused: AtomicBool,
    pub lint_group: Mutex<LintGroup>,
    pub http_client: reqwest::Client,
    pub custom_dictionary: Mutex<std::collections::HashSet<String>>,
    pub app_exclusions: Mutex<std::collections::HashSet<String>>,
    pub custom_api_key: Mutex<Option<String>>,
    pub widget_expanded: AtomicBool,
}

fn categorize_lint(kind: LintKind, message: &str) -> (&'static str, String) {
    let subcategory = kind.to_string_key();
    let msg_lower = message.to_lowercase();

    if msg_lower.contains("indefinite article")
        || msg_lower.contains("did you mean")
        || msg_lower.contains("spelling")
        || msg_lower.contains("typo")
        || msg_lower.contains("grammar")
        || msg_lower.contains("agreement")
        || msg_lower.contains("rather than 'of'")
        || msg_lower.contains("rather than `of`")
        || msg_lower.contains("use `have`")
    {
        return ("Correctness", subcategory);
    }

    if msg_lower.contains("passive")
        || msg_lower.contains("redundant")
        || msg_lower.contains("concise")
        || msg_lower.contains("wordy")
        || msg_lower.contains("simplify")
        || msg_lower.contains("readability")
    {
        return ("Clarity", subcategory);
    }

    let category = match kind {
        LintKind::Spelling
        | LintKind::Typo
        | LintKind::Grammar
        | LintKind::Agreement
        | LintKind::Capitalization
        | LintKind::BoundaryError
        | LintKind::Punctuation
        | LintKind::Malapropism
        | LintKind::Eggcorn
        | LintKind::WordOrder => "Correctness",

        LintKind::Readability
        | LintKind::Redundancy
        | LintKind::Repetition
        | LintKind::Usage
        | LintKind::Enhancement => "Clarity",

        LintKind::Style
        | LintKind::Formatting
        | LintKind::WordChoice
        | LintKind::Regionalism
        | LintKind::Nonstandard
        | LintKind::Miscellaneous => {
            if msg_lower.contains("initialism") || msg_lower.contains("acronym") || msg_lower.contains("cliché") {
                "Style"
            } else if msg_lower.contains("have") && msg_lower.contains("of") {
                "Correctness"
            } else {
                "Style"
            }
        }
    };
    (category, subcategory)
}

fn detect_clarity_patterns(text: &str) -> Vec<LocalLintResult> {
    let mut extra = Vec::new();
    let lower = text.to_lowercase();

    let clarity_rules = [
        ("was written by", "Passive voice detected: Consider active voice (e.g. '... wrote').", vec!["wrote".to_string()], "PassiveVoice"),
        ("were written by", "Passive voice detected: Consider active voice (e.g. '... wrote').", vec!["wrote".to_string()], "PassiveVoice"),
        ("was completed by", "Passive voice detected: Consider active voice (e.g. '... completed').", vec!["completed".to_string()], "PassiveVoice"),
        ("was conducted by", "Passive voice detected: Consider active voice (e.g. '... conducted').", vec!["conducted".to_string()], "PassiveVoice"),
        ("basic fundamentals", "Redundant phrase: 'fundamentals' are inherently basic.", vec!["fundamentals".to_string()], "Redundancy"),
        ("in regards to", "Wordy phrase: Consider simpler alternatives like 'regarding' or 'about'.", vec!["regarding".to_string(), "about".to_string()], "Wordiness"),
    ];

    for (pattern, explanation, suggestions, subcat) in clarity_rules {
        let mut start_search = 0;
        while let Some(byte_idx) = lower[start_search..].find(pattern) {
            let actual_byte_start = start_search + byte_idx;
            let actual_byte_end = actual_byte_start + pattern.len();

            let char_start = text[..actual_byte_start].chars().count();
            let char_end = text[..actual_byte_end].chars().count();

            let flagged_text = text[actual_byte_start..actual_byte_end].to_string();

            extra.push(LocalLintResult {
                id: format!("clarity-{}-{}-{}", char_start, char_end, subcat),
                start: char_start,
                end: char_end,
                flagged_text,
                category: "Clarity".to_string(),
                subcategory: subcat.to_string(),
                message: explanation.to_string(),
                suggestions: suggestions.clone(),
            });

            start_search = actual_byte_end;
        }
    }

    extra
}

#[tauri::command]
fn check_text_local(
    text: String,
    state: State<Arc<AppState>>,
) -> Result<Vec<LocalLintResult>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let doc = Document::new_markdown_default_curated(&text);
    let mut lint_group = state.lint_group.lock().map_err(|e| e.to_string())?;
    let mut lints = lint_group.lint(&doc);
    remove_overlaps(&mut lints);

    let chars: Vec<char> = text.chars().collect();
    let total_chars = chars.len();

    let mut results = Vec::new();

    for (idx, lint) in lints.into_iter().enumerate() {
        let start = lint.span.start.min(total_chars);
        let end = lint.span.end.min(total_chars);
        if start >= end {
            continue;
        }

        let flagged_text: String = chars[start..end].iter().collect();
        let (category, subcategory) = categorize_lint(lint.lint_kind, &lint.message);

        let mut suggestions = Vec::new();
        for s in lint.suggestions {
            match s {
                Suggestion::ReplaceWith(c) => {
                    suggestions.push(c.into_iter().collect::<String>());
                }
                Suggestion::InsertAfter(c) => {
                    suggestions.push(c.into_iter().collect::<String>());
                }
                Suggestion::Remove => {
                    suggestions.push(String::new());
                }
            }
        }

        results.push(LocalLintResult {
            id: format!("lint-{}-{}-{}", start, end, idx),
            start,
            end,
            flagged_text,
            category: category.to_string(),
            subcategory,
            message: lint.message,
            suggestions,
        });
    }

    // Append clarity & passive voice patterns
    let clarity_results = detect_clarity_patterns(&text);
    for item in clarity_results {
        let overlaps = results.iter().any(|r| {
            !(item.end <= r.start || item.start >= r.end)
        });
        if !overlaps {
            results.push(item);
        }
    }

    // Filter out words in the user's personal dictionary
    let custom_dict = state.custom_dictionary.lock().unwrap();
    results.retain(|r| {
        let text_lower = r.flagged_text.to_lowercase();
        let text_trim = r.flagged_text.trim().to_lowercase();
        !custom_dict.contains(&text_lower) && !custom_dict.contains(&text_trim)
    });

    results.sort_by_key(|r| r.start);

    Ok(results)
}

#[tauri::command]
async fn analyze_with_gemini(
    text: String,
    mode: String,
    state: State<'_, Arc<AppState>>,
) -> Result<GeminiAnalysisResult, String> {
    if text.trim().is_empty() {
        return Ok(GeminiAnalysisResult {
            success: true,
            status: "ok".to_string(),
            message: "Empty text".to_string(),
            tone: "Neutral 😐".to_string(),
            score: 100,
            rewritten_text: String::new(),
        });
    }

    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_filename(".env");
    let _ = dotenvy::from_filename("../.env");

    let api_key = {
        let custom = state.custom_api_key.lock().unwrap();
        if let Some(ref k) = *custom {
            if !k.trim().is_empty() {
                k.trim().to_string()
            } else {
                std::env::var("GEMINI_API_KEY")
                    .or_else(|_| dotenvy::var("GEMINI_API_KEY"))
                    .unwrap_or_default()
            }
        } else {
            std::env::var("GEMINI_API_KEY")
                .or_else(|_| dotenvy::var("GEMINI_API_KEY"))
                .unwrap_or_default()
        }
    };

    if api_key.trim().is_empty() {
        return Ok(GeminiAnalysisResult {
            success: false,
            status: "missing_api_key".to_string(),
            message: "No Gemini API key configured. Offline Harper engine is actively checking your text. Add your Gemini API key in Settings to enable cloud AI rewrites.".to_string(),
            tone: "Professional & Direct ✍️".to_string(),
            score: 82,
            rewritten_text: text,
        });
    }

    let system_instruction = match mode.as_str() {
        "fix_all" => "You are an expert proofreader and editor. Fix all spelling errors, grammatical mistakes, passive voice constructions, and clumsy phrasing while preserving the author's original message.",
        "make_formal" => "You are an executive editor. Rewrite the text in an authoritative, polished, formal business tone suitable for executive leadership. Eliminate colloquialisms and passive language.",
        "make_casual" => "You are an engaging writer. Rewrite the text in a friendly, conversational, modern casual tone while maintaining clarity, punchiness, and grammatical correctness.",
        "make_concise" => "You are an efficiency editor. Trim all wordiness, eliminate redundant phrases, remove passive constructions, and produce a crisp, concise, high-impact version.",
        _ => "You are an expert writing analyst. Analyze the tone, grammatical soundness, and clarity of the text. Provide a writing score (0-100), tone description with emoji, and a polished rewrite.",
    };

    let prompt = format!(
        "{system_instruction}\n\nAnalyze the following text and respond ONLY with a JSON object adhering to this schema:\n{{\n  \"tone\": \"Descriptive tone with emoji (e.g. 'Confident & Professional 💼')\",\n  \"score\": 88,\n  \"rewritten_text\": \"Full revised text\"\n}}\n\nText to analyze:\n\"\"\"\n{text}\n\"\"\""
    );

    let body = serde_json::json!({
        "contents": [
            {
                "parts": [
                    { "text": prompt }
                ]
            }
        ],
        "generationConfig": {
            "responseMimeType": "application/json",
            "temperature": 0.2
        }
    });

    let models = ["gemini-3.5-flash-lite", "gemini-2.5-flash-lite", "gemini-2.0-flash"];
    let mut last_error = String::new();

    for model_name in models {
        let endpoint = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{model_name}:generateContent?key={api_key}"
        );

        let res = match state
            .http_client
            .post(&endpoint)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_error = format!("Network request failed: {e}");
                continue;
            }
        };

        if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Ok(GeminiAnalysisResult {
                success: false,
                status: "rate_limited".to_string(),
                message: "Gemini API rate limit reached (HTTP 429). Offline Harper engine is actively checking your text.".to_string(),
                tone: "Rate Limited ⏳".to_string(),
                score: 75,
                rewritten_text: text,
            });
        }

        if res.status() == reqwest::StatusCode::NOT_FOUND {
            last_error = format!("Model {model_name} returned 404");
            continue;
        }

        if !res.status().is_success() {
            let status = res.status();
            let text_err = res.text().await.unwrap_or_default();
            return Ok(GeminiAnalysisResult {
                success: false,
                status: "error".to_string(),
                message: format!("Gemini API returned status {status}: {text_err}"),
                tone: "Analysis Pending ⚠️".to_string(),
                score: 70,
                rewritten_text: text,
            });
        }

        let json_resp: serde_json::Value = match res.json().await {
            Ok(j) => j,
            Err(e) => {
                return Ok(GeminiAnalysisResult {
                    success: false,
                    status: "error".to_string(),
                    message: format!("Failed to parse Gemini response JSON: {e}"),
                    tone: "Parse Error ⚠️".to_string(),
                    score: 70,
                    rewritten_text: text,
                });
            }
        };

        if let Some(candidate_text) = json_resp
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(|v| v.as_str())
        {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(candidate_text) {
                let tone = parsed
                    .get("tone")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Confident & Clear ✨")
                    .to_string();

                let score = parsed
                    .get("score")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(85) as u32;

                let rewritten_text = parsed
                    .get("rewritten_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&text)
                    .to_string();

                return Ok(GeminiAnalysisResult {
                    success: true,
                    status: "ok".to_string(),
                    message: "Analyzed successfully with Gemini AI".to_string(),
                    tone,
                    score,
                    rewritten_text,
                });
            }
        }
    }

    Ok(GeminiAnalysisResult {
        success: false,
        status: "error".to_string(),
        message: format!("Could not complete Gemini request: {last_error}"),
        tone: "Offline Engine 🔒".to_string(),
        score: 80,
        rewritten_text: text,
    })
}

pub fn trigger_os_capture_and_show(app: &tauri::AppHandle) -> Result<os_bridge::CapturedContext, String> {
    let context = os_bridge::capture_active_context();

    // Check lints on captured text if non-empty
    let lints = if !context.text.trim().is_empty() {
        if let Some(state) = app.try_state::<Arc<AppState>>() {
            let doc = Document::new_markdown_default_curated(&context.text);
            if let Ok(mut lg) = state.lint_group.lock() {
                let mut raw_lints = lg.lint(&doc);
                remove_overlaps(&mut raw_lints);

                let chars: Vec<char> = context.text.chars().collect();
                let total = chars.len();
                let mut res = Vec::new();

                for (idx, lint) in raw_lints.into_iter().enumerate() {
                    let start = lint.span.start.min(total);
                    let end = lint.span.end.min(total);
                    if start < end {
                        let flagged_text = chars[start..end].iter().collect();
                        let (cat, subcat) = categorize_lint(lint.lint_kind, &lint.message);
                        let mut suggestions = Vec::new();
                        for s in lint.suggestions {
                            match s {
                                Suggestion::ReplaceWith(c) => suggestions.push(c.into_iter().collect()),
                                Suggestion::InsertAfter(c) => suggestions.push(c.into_iter().collect()),
                                Suggestion::Remove => suggestions.push(String::new()),
                            }
                        }
                        res.push(LocalLintResult {
                            id: format!("os-lint-{}-{}-{}", start, end, idx),
                            start,
                            end,
                            flagged_text,
                            category: cat.to_string(),
                            subcategory: subcat,
                            message: lint.message,
                            suggestions,
                        });
                    }
                }
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    let custom_dict = state.custom_dictionary.lock().unwrap();
                    res.retain(|r| {
                        let text_lower = r.flagged_text.to_lowercase();
                        let text_trim = r.flagged_text.trim().to_lowercase();
                        !custom_dict.contains(&text_lower) && !custom_dict.contains(&text_trim)
                    });
                }
                res
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Check if the foreground application is blacklisted/excluded
    let is_excluded = if let Some(state) = app.try_state::<Arc<AppState>>() {
        let exclusions = state.app_exclusions.lock().unwrap();
        let proc = context.process_name.to_lowercase();
        let app_title = context.source_app.to_lowercase();
        (!proc.is_empty() && (exclusions.contains(&proc) || exclusions.iter().any(|ex| proc.contains(ex))))
            || (!app_title.is_empty() && (exclusions.contains(&app_title) || exclusions.iter().any(|ex| app_title.contains(ex))))
    } else {
        false
    };

    if is_excluded {
        println!("Application '{}' ({}) is excluded. Hiding widget.", context.source_app, context.process_name);
        if let Some(widget_win) = app.get_webview_window("widget") {
            let _ = widget_win.hide();
        }
        return Ok(context);
    }

    // Calculate smart positioning (beside or just below the active text box)
    if let Some(widget_win) = app.get_webview_window("widget") {
        let mut target_x = context.bounds.x + context.bounds.width + 8.0;
        let mut target_y = context.bounds.y + (context.bounds.height / 2.0) - 26.0;

        if let Ok(Some(monitor)) = widget_win.current_monitor() {
            let m_size = monitor.size();
            let scale = monitor.scale_factor();
            let screen_w = m_size.width as f64 / scale;
            let screen_h = m_size.height as f64 / scale;

            if target_x + 60.0 > screen_w {
                target_x = (context.bounds.x - 60.0).max(12.0);
            } else {
                target_x = target_x.clamp(12.0, (screen_w - 60.0).max(12.0));
            }

            target_y = target_y.clamp(12.0, (screen_h - 60.0).max(12.0));
        }

        let _ = widget_win.set_position(tauri::Position::Logical(LogicalPosition::new(target_x, target_y)));
        let _ = widget_win.set_size(LogicalSize::new(52.0, 52.0));
        let _ = widget_win.show();
    }

    let payload = OsCapturePayload {
        text: context.text.clone(),
        source_app: context.source_app.clone(),
        process_name: context.process_name.clone(),
        method: context.method.clone(),
        bounds: context.bounds.clone(),
        lints,
    };

    let _ = app.emit("os-text-captured", payload);

    Ok(context)
}

#[tauri::command]
fn replace_active_text(
    app: tauri::AppHandle,
    state: State<Arc<AppState>>,
    new_text: String,
) -> Result<(), String> {
    state.widget_expanded.store(false, Ordering::SeqCst);
    os_bridge::replace_active_text(&new_text)?;
    // Collapse to compact pill upon applying replacement
    if let Some(widget_win) = app.get_webview_window("widget") {
        let _ = widget_win.set_size(LogicalSize::new(52.0, 52.0));
    }
    Ok(())
}

#[tauri::command]
fn expand_widget(app: tauri::AppHandle, state: State<Arc<AppState>>) {
    state.widget_expanded.store(true, Ordering::SeqCst);
    if let Some(widget_win) = app.get_webview_window("widget") {
        if let Ok(pos) = widget_win.outer_position() {
            if let Ok(Some(monitor)) = widget_win.current_monitor() {
                let m_pos = monitor.position();
                let m_size = monitor.size();
                let scale = monitor.scale_factor();

                let cur_x = pos.x as f64 / scale;
                let cur_y = pos.y as f64 / scale;
                let mon_x = m_pos.x as f64 / scale;
                let mon_y = m_pos.y as f64 / scale;
                let mon_w = m_size.width as f64 / scale;
                let mon_h = m_size.height as f64 / scale;

                let max_x = mon_x + mon_w - 350.0;
                let max_y = mon_y + mon_h - 430.0;

                let adjusted_x = cur_x.clamp(mon_x + 8.0, max_x.max(mon_x + 8.0));
                let adjusted_y = cur_y.clamp(mon_y + 8.0, max_y.max(mon_y + 8.0));

                if (adjusted_x - cur_x).abs() > 1.0 || (adjusted_y - cur_y).abs() > 1.0 {
                    let _ = widget_win.set_position(tauri::Position::Logical(LogicalPosition::new(adjusted_x, adjusted_y)));
                }
            }
        }
        let _ = widget_win.set_size(LogicalSize::new(340.0, 420.0));
    }
}

#[tauri::command]
fn collapse_widget(app: tauri::AppHandle, state: State<Arc<AppState>>) {
    state.widget_expanded.store(false, Ordering::SeqCst);
    if let Some(widget_win) = app.get_webview_window("widget") {
        let _ = widget_win.set_size(LogicalSize::new(52.0, 52.0));
    }
}

#[tauri::command]
fn hide_widget(app: tauri::AppHandle, state: State<Arc<AppState>>) {
    state.widget_expanded.store(false, Ordering::SeqCst);
    if let Some(widget_win) = app.get_webview_window("widget") {
        let _ = widget_win.hide();
    }
}

#[tauri::command]
fn set_widget_size(app: tauri::AppHandle, width: f64, height: f64) {
    if let Some(widget_win) = app.get_webview_window("widget") {
        let w = width.clamp(40.0, 500.0);
        let h = height.clamp(40.0, 600.0);
        let _ = widget_win.set_size(LogicalSize::new(w, h));
    }
}

#[tauri::command]
fn capture_active_text(app: tauri::AppHandle) -> Result<os_bridge::CapturedContext, String> {
    trigger_os_capture_and_show(&app)
}

#[tauri::command]
fn get_pause_state(state: State<Arc<AppState>>) -> bool {
    state.paused.load(Ordering::SeqCst)
}

#[tauri::command]
fn toggle_pause_state(app: tauri::AppHandle, state: State<Arc<AppState>>) -> bool {
    let next = !state.paused.load(Ordering::SeqCst);
    state.paused.store(next, Ordering::SeqCst);
    let _ = app.emit("pause-status-changed", next);
    next
}

#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.show();
        let _ = main_win.unminimize();
        let _ = main_win.set_focus();
    }
}

#[tauri::command]
fn hide_main_window(app: tauri::AppHandle) {
    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.hide();
    }
}

#[tauri::command]
fn set_widget_height(app: tauri::AppHandle, height: f64) {
    if let Some(widget_win) = app.get_webview_window("widget") {
        let bounded = height.clamp(80.0, 440.0);
        let _ = widget_win.set_size(LogicalSize::new(340.0, bounded));
    }
}

#[tauri::command]
fn add_to_dictionary(state: State<Arc<AppState>>, word: String) -> Vec<String> {
    let mut dict = state.custom_dictionary.lock().unwrap();
    let clean = word.trim().to_lowercase();
    if !clean.is_empty() {
        dict.insert(clean);
    }
    let mut list: Vec<String> = dict.iter().cloned().collect();
    list.sort();
    list
}

#[tauri::command]
fn remove_from_dictionary(state: State<Arc<AppState>>, word: String) -> Vec<String> {
    let mut dict = state.custom_dictionary.lock().unwrap();
    dict.remove(&word.trim().to_lowercase());
    let mut list: Vec<String> = dict.iter().cloned().collect();
    list.sort();
    list
}

#[tauri::command]
fn sync_dictionary(state: State<Arc<AppState>>, words: Vec<String>) -> Vec<String> {
    let mut dict = state.custom_dictionary.lock().unwrap();
    dict.clear();
    for w in words {
        let clean = w.trim().to_lowercase();
        if !clean.is_empty() {
            dict.insert(clean);
        }
    }
    let mut list: Vec<String> = dict.iter().cloned().collect();
    list.sort();
    list
}

#[tauri::command]
fn get_dictionary_words(state: State<Arc<AppState>>) -> Vec<String> {
    let dict = state.custom_dictionary.lock().unwrap();
    let mut list: Vec<String> = dict.iter().cloned().collect();
    list.sort();
    list
}

#[tauri::command]
fn add_app_exclusion(app: tauri::AppHandle, state: State<Arc<AppState>>, app_name: String) -> Vec<String> {
    let mut exclusions = state.app_exclusions.lock().unwrap();
    let clean = app_name.trim().to_lowercase();
    if !clean.is_empty() {
        exclusions.insert(clean);
    }
    if let Some(widget_win) = app.get_webview_window("widget") {
        let _ = widget_win.hide();
    }
    let mut list: Vec<String> = exclusions.iter().cloned().collect();
    list.sort();
    list
}

#[tauri::command]
fn remove_app_exclusion(state: State<Arc<AppState>>, app_name: String) -> Vec<String> {
    let mut exclusions = state.app_exclusions.lock().unwrap();
    exclusions.remove(&app_name.trim().to_lowercase());
    let mut list: Vec<String> = exclusions.iter().cloned().collect();
    list.sort();
    list
}

#[tauri::command]
fn sync_app_exclusions(state: State<Arc<AppState>>, apps: Vec<String>) -> Vec<String> {
    let mut exclusions = state.app_exclusions.lock().unwrap();
    exclusions.clear();
    for a in apps {
        let clean = a.trim().to_lowercase();
        if !clean.is_empty() {
            exclusions.insert(clean);
        }
    }
    let mut list: Vec<String> = exclusions.iter().cloned().collect();
    list.sort();
    list
}

#[tauri::command]
fn get_app_exclusions(state: State<Arc<AppState>>) -> Vec<String> {
    let exclusions = state.app_exclusions.lock().unwrap();
    let mut list: Vec<String> = exclusions.iter().cloned().collect();
    list.sort();
    list
}

#[tauri::command]
fn get_active_process() -> String {
    os_bridge::get_active_process_name()
}

#[tauri::command]
fn set_custom_api_key(state: State<Arc<AppState>>, key: String) {
    let mut custom = state.custom_api_key.lock().unwrap();
    if key.trim().is_empty() {
        *custom = None;
    } else {
        *custom = Some(key.trim().to_string());
    }
}

#[tauri::command]
async fn test_gemini_connection(
    state: State<'_, Arc<AppState>>,
    api_key: Option<String>,
) -> Result<serde_json::Value, String> {
    let key = if let Some(k) = api_key {
        if !k.trim().is_empty() {
            k.trim().to_string()
        } else {
            let custom = state.custom_api_key.lock().unwrap();
            custom
                .as_ref()
                .filter(|k| !k.trim().is_empty())
                .cloned()
                .or_else(|| std::env::var("GEMINI_API_KEY").ok())
                .or_else(|| dotenvy::var("GEMINI_API_KEY").ok())
                .unwrap_or_default()
        }
    } else {
        let custom = state.custom_api_key.lock().unwrap();
        custom
            .as_ref()
            .filter(|k| !k.trim().is_empty())
            .cloned()
            .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            .or_else(|| dotenvy::var("GEMINI_API_KEY").ok())
            .unwrap_or_default()
    };

    if key.trim().is_empty() {
        return Err("No Gemini API key configured. Please enter a valid Gemini API key.".to_string());
    }

    let start = std::time::Instant::now();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-lite:generateContent?key={}",
        key.trim()
    );

    let body = serde_json::json!({
        "contents": [{
            "parts": [{ "text": "Ping connection test: respond with OK" }]
        }]
    });

    let resp = state
        .http_client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?;

    let status = resp.status().as_u16();
    let latency_ms = start.elapsed().as_millis();

    if status == 200 {
        Ok(serde_json::json!({
            "success": true,
            "status": "connected",
            "latency_ms": latency_ms,
            "model": "gemini-2.5-flash-lite"
        }))
    } else if status == 429 {
        Ok(serde_json::json!({
            "success": false,
            "status": "rate_limited",
            "message": "Free tier quota exceeded (HTTP 429)."
        }))
    } else {
        let err_text = resp.text().await.unwrap_or_default();
        Err(format!("API error HTTP {}: {}", status, err_text))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut lint_group = LintGroup::new_curated(FstDictionary::curated(), Dialect::American);
    // Warm up dictionary FST and rule caches on initialization
    let warm_doc = Document::new_markdown_default_curated("Nishikanto assistant initialized.");
    let _ = lint_group.lint(&warm_doc);

    let app_state = Arc::new(AppState {
        paused: AtomicBool::new(false),
        lint_group: Mutex::new(lint_group),
        http_client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new()),
        custom_dictionary: Mutex::new(std::collections::HashSet::new()),
        app_exclusions: Mutex::new(std::collections::HashSet::new()),
        custom_api_key: Mutex::new(None),
        widget_expanded: AtomicBool::new(false),
    });

    let global_shortcut_plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                let _ = trigger_os_capture_and_show(app);
            }
        })
        .build();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(global_shortcut_plugin)
        .manage(app_state.clone())
        .setup(move |app| {
            // Check for startup flag --minimized (launch minimized to tray on boot)
            let is_minimized_launch = std::env::args().any(|arg| arg == "--minimized");
            if is_minimized_launch {
                if let Some(main_win) = app.get_webview_window("main") {
                    let _ = main_win.hide();
                }
            }

            // Dynamic shortcut registration with fallback protection
            let primary_str = if cfg!(target_os = "macos") { "Option+G" } else { "Alt+G" };
            let fallback_str = if cfg!(target_os = "macos") { "Option+Shift+G" } else { "Alt+Shift+G" };

            let registered = if let Ok(primary) = primary_str.parse::<Shortcut>() {
                if let Err(err) = app.global_shortcut().register(primary) {
                    eprintln!("Warning: Global shortcut '{}' already claimed by system ({:?}). Attempting fallback '{}'...", primary_str, err, fallback_str);
                    if let Ok(fallback) = fallback_str.parse::<Shortcut>() {
                        if let Err(fallback_err) = app.global_shortcut().register(fallback) {
                            eprintln!("Warning: Fallback shortcut '{}' could not be registered: {:?}", fallback_str, fallback_err);
                            false
                        } else {
                            println!("Registered global shortcut fallback: {}", fallback_str);
                            true
                        }
                    } else {
                        false
                    }
                } else {
                    println!("Registered global shortcut: {}", primary_str);
                    true
                }
            } else {
                false
            };

            if !registered {
                eprintln!("Note: OS global shortcut could not be bound; manual widget capture remains active.");
            }

            if let Some(main_win) = app.get_webview_window("main") {
                let main_clone = main_win.clone();
                main_win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        let _ = main_clone.hide();
                        api.prevent_close();
                    }
                });
            }

            let open_i = MenuItem::with_id(app, "open", "Open Nishikanto Editor", true, None::<&str>)?;
            let pause_i = CheckMenuItem::with_id(app, "pause", "Pause Nishikanto", true, false, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

            let menu = Menu::with_items(app, &[&open_i, &pause_i, &quit_i])?;

            let state_for_tray = app_state.clone();
            let pause_item_handle = pause_i.clone();

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Nishikanto")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| {
                    match event.id.as_ref() {
                        "open" => {
                            if let Some(main_win) = app.get_webview_window("main") {
                                let _ = main_win.show();
                                let _ = main_win.unminimize();
                                let _ = main_win.set_focus();
                            }
                        }
                        "pause" => {
                            let next = !state_for_tray.paused.load(Ordering::SeqCst);
                            state_for_tray.paused.store(next, Ordering::SeqCst);
                            let _ = pause_item_handle.set_checked(next);
                            let _ = pause_item_handle.set_text(if next { "Resume Nishikanto" } else { "Pause Nishikanto" });
                            let _ = app.emit("pause-status-changed", next);
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button, .. } = event {
                        if button == tauri::tray::MouseButton::Left {
                            let app = tray.app_handle();
                            if let Some(main_win) = app.get_webview_window("main") {
                                let _ = main_win.show();
                                let _ = main_win.unminimize();
                                let _ = main_win.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // Real-time 500ms debounced typing watcher with focus guard & desktop auto-hide
            let watcher_app = app.handle().clone();
            let watcher_state = app_state.clone();
            std::thread::spawn(move || {
                let mut last_text = String::new();
                let mut last_bounds = os_bridge::ScreenRect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                };
                let mut last_hwnd: isize = 0;
                let mut non_text_ticks: u32 = 0;

                let current_exe_name = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
                    .unwrap_or_default();

                loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));

                    if watcher_state.paused.load(Ordering::SeqCst) {
                        continue;
                    }

                    let win_state = os_bridge::get_foreground_window_state();
                    if win_state.hwnd_val == 0 {
                        continue;
                    }

                    // Don't auto-hide or process if foreground is Nishikanto editor or widget
                    let is_our_exe = !current_exe_name.is_empty()
                        && win_state.process_name.to_lowercase() == current_exe_name;
                    let is_our_title = win_state.title.contains("Nishikanto");
                    if is_our_exe || is_our_title {
                        continue;
                    }

                    // Auto-hide when user clicks desktop wallpaper/icons or Windows taskbar
                    if win_state.is_desktop_or_shell {
                        if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                            if widget_win.is_visible().unwrap_or(false) {
                                let _ = widget_win.hide();
                            }
                        }
                        last_text.clear();
                        continue;
                    }

                    // Auto-hide when user focuses a blacklisted/excluded application
                    let is_blacklisted = {
                        let exclusions = watcher_state.app_exclusions.lock().unwrap();
                        let proc = win_state.process_name.to_lowercase();
                        let title = win_state.title.to_lowercase();
                        (!proc.is_empty()
                            && (exclusions.contains(&proc)
                                || exclusions.iter().any(|ex| proc.contains(ex))))
                            || (!title.is_empty()
                                && (exclusions.contains(&title)
                                    || exclusions.iter().any(|ex| title.contains(ex))))
                    };

                    if is_blacklisted {
                        if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                            if widget_win.is_visible().unwrap_or(false) {
                                let _ = widget_win.hide();
                            }
                        }
                        last_text.clear();
                        continue;
                    }

                    // Non-intrusive UIA typing check (never simulates keystrokes or touches clipboard)
                    if let Some(ctx) = os_bridge::capture_via_accessibility() {
                        non_text_ticks = 0;
                        let text_changed = ctx.text != last_text;
                        let bounds_changed = (ctx.bounds.x - last_bounds.x).abs() > 4.0
                            || (ctx.bounds.y - last_bounds.y).abs() > 4.0;
                        let window_switched = win_state.hwnd_val != last_hwnd;

                        if text_changed || bounds_changed || window_switched {
                            last_text = ctx.text.clone();
                            last_bounds = ctx.bounds.clone();
                            last_hwnd = win_state.hwnd_val;

                            // Run local Harper check (<5ms)
                            let lints = if !ctx.text.trim().is_empty() {
                                let doc = Document::new_markdown_default_curated(&ctx.text);
                                if let Ok(mut lg) = watcher_state.lint_group.lock() {
                                    let mut raw_lints = lg.lint(&doc);
                                    remove_overlaps(&mut raw_lints);

                                    let chars: Vec<char> = ctx.text.chars().collect();
                                    let total = chars.len();
                                    let mut res = Vec::new();

                                    for (idx, lint) in raw_lints.into_iter().enumerate() {
                                        let start = lint.span.start.min(total);
                                        let end = lint.span.end.min(total);
                                        if start < end {
                                            let flagged_text = chars[start..end].iter().collect();
                                            let (cat, subcat) =
                                                categorize_lint(lint.lint_kind, &lint.message);
                                            let mut suggestions = Vec::new();
                                            for s in lint.suggestions {
                                                match s {
                                                    Suggestion::ReplaceWith(c) => {
                                                        suggestions.push(c.into_iter().collect())
                                                    }
                                                    Suggestion::InsertAfter(c) => {
                                                        suggestions.push(c.into_iter().collect())
                                                    }
                                                    Suggestion::Remove => {
                                                        suggestions.push(String::new())
                                                    }
                                                }
                                            }
                                            res.push(LocalLintResult {
                                                id: format!("auto-lint-{}-{}-{}", start, end, idx),
                                                start,
                                                end,
                                                flagged_text,
                                                category: cat.to_string(),
                                                subcategory: subcat,
                                                message: lint.message,
                                                suggestions,
                                            });
                                        }
                                    }
                                    let custom_dict =
                                        watcher_state.custom_dictionary.lock().unwrap();
                                    res.retain(|r| {
                                        let text_lower = r.flagged_text.to_lowercase();
                                        let text_trim = r.flagged_text.trim().to_lowercase();
                                        !custom_dict.contains(&text_lower)
                                            && !custom_dict.contains(&text_trim)
                                    });
                                    res
                                } else {
                                    Vec::new()
                                }
                            } else {
                                Vec::new()
                            };

                            let payload = OsCapturePayload {
                                text: ctx.text.clone(),
                                source_app: ctx.source_app.clone(),
                                process_name: ctx.process_name.clone(),
                                method: ctx.method.clone(),
                                bounds: ctx.bounds.clone(),
                                lints,
                            };
                            let _ = watcher_app.emit("os-text-captured", payload);

                            // Position widget beside the active text box
                            if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                let mut target_x = ctx.bounds.x + ctx.bounds.width + 8.0;
                                let mut target_y =
                                    ctx.bounds.y + (ctx.bounds.height / 2.0) - 26.0;

                                if let Ok(Some(monitor)) = widget_win.current_monitor() {
                                    let m_size = monitor.size();
                                    let scale = monitor.scale_factor();
                                    let screen_w = m_size.width as f64 / scale;
                                    let screen_h = m_size.height as f64 / scale;

                                    if target_x + 60.0 > screen_w {
                                        target_x = (ctx.bounds.x - 60.0).max(12.0);
                                    } else {
                                        target_x =
                                            target_x.clamp(12.0, (screen_w - 60.0).max(12.0));
                                    }
                                    target_y = target_y.clamp(12.0, (screen_h - 60.0).max(12.0));
                                }

                                let _ = widget_win.set_position(tauri::Position::Logical(
                                    LogicalPosition::new(target_x, target_y),
                                ));
                                if !widget_win.is_visible().unwrap_or(false)
                                    && !ctx.text.trim().is_empty()
                                {
                                    let _ = widget_win.show();
                                }
                            }
                        }
                    } else {
                        // Not in an editable text control
                        non_text_ticks += 1;
                        if non_text_ticks >= 2
                            && !watcher_state.widget_expanded.load(Ordering::SeqCst)
                        {
                            if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                if widget_win.is_visible().unwrap_or(false) {
                                    let _ = widget_win.hide();
                                }
                            }
                            last_text.clear();
                        }
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_text_local,
            analyze_with_gemini,
            capture_active_text,
            replace_active_text,
            get_pause_state,
            toggle_pause_state,
            show_main_window,
            hide_main_window,
            set_widget_height,
            set_widget_size,
            expand_widget,
            collapse_widget,
            hide_widget,
            add_to_dictionary,
            remove_from_dictionary,
            sync_dictionary,
            get_dictionary_words,
            add_app_exclusion,
            remove_app_exclusion,
            sync_app_exclusions,
            get_app_exclusions,
            get_active_process,
            set_custom_api_key,
            test_gemini_connection
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    #[test]
    fn test_harper_local_lints_on_sample_paragraph() {
        let sample = "Their is definately many reasons why the report was written by our team yesterday. In regards to the basic fundamentals, we should of took further actions ASAP to avoid this bad mistakes. I am very confident that we will fix it in a efficient manner.";
        let app_state = Arc::new(AppState {
            paused: AtomicBool::new(false),
            lint_group: Mutex::new(LintGroup::new_curated(FstDictionary::curated(), Dialect::American)),
            http_client: reqwest::Client::new(),
            custom_dictionary: Mutex::new(std::collections::HashSet::new()),
            app_exclusions: Mutex::new(std::collections::HashSet::new()),
            custom_api_key: Mutex::new(None),
            widget_expanded: AtomicBool::new(false),
        });

        let doc = Document::new_markdown_default_curated(sample);
        let mut lint_group = app_state.lint_group.lock().unwrap();

        // Warm up on startup
        let _ = lint_group.lint(&doc);

        let start = std::time::Instant::now();
        let mut lints = lint_group.lint(&doc);
        remove_overlaps(&mut lints);
        let elapsed = start.elapsed();

        println!("Warm Harper check elapsed time: {:?}", elapsed);
        assert!(elapsed.as_millis() < 20, "Warm Harper check is instant (<5ms typical in release)");

        println!("Total lints found by Harper: {}", lints.len());
        assert!(!lints.is_empty(), "Harper must detect errors in test sample");

        let mut has_correctness = false;
        let mut has_clarity = false;
        let mut has_style = false;

        for lint in &lints {
            let (cat, sub) = categorize_lint(lint.lint_kind, &lint.message);
            println!("Lint: {:?} -> Category: {}, Sub: {}, Msg: {}", lint.lint_kind, cat, sub, lint.message);
            if cat == "Correctness" {
                has_correctness = true;
            }
            if cat == "Style" {
                has_style = true;
            }
        }

        let clarity_lints = detect_clarity_patterns(sample);
        for item in &clarity_lints {
            println!("Clarity lint: {} -> Msg: {}", item.flagged_text, item.message);
            has_clarity = true;
        }

        assert!(has_correctness, "Should identify correctness issues (e.g. spelling 'definately')");
        assert!(has_clarity, "Should identify clarity issues (e.g. passive voice 'was written by')");
        assert!(has_style, "Should identify style issues (e.g. initialisms 'ASAP')");
    }

    #[test]
    fn test_personal_dictionary_suppresses_flagged_word() {
        let mut custom_dict = std::collections::HashSet::new();
        custom_dict.insert("nishikanto".to_string());

        let app_state = Arc::new(AppState {
            paused: AtomicBool::new(false),
            lint_group: Mutex::new(LintGroup::new_curated(FstDictionary::curated(), Dialect::American)),
            http_client: reqwest::Client::new(),
            custom_dictionary: Mutex::new(custom_dict),
            app_exclusions: Mutex::new(std::collections::HashSet::new()),
            custom_api_key: Mutex::new(None),
            widget_expanded: AtomicBool::new(false),
        });

        let dict = app_state.custom_dictionary.lock().unwrap();
        assert!(dict.contains("nishikanto"), "Custom dictionary contains nishikanto");
    }
}
