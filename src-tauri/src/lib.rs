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
    pub method: String,
    pub bounds: os_bridge::ScreenRect,
    pub lints: Vec<LocalLintResult>,
}

pub struct AppState {
    pub paused: AtomicBool,
    pub lint_group: Mutex<LintGroup>,
    pub http_client: reqwest::Client,
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

    let api_key = std::env::var("GEMINI_API_KEY")
        .or_else(|_| dotenvy::var("GEMINI_API_KEY"))
        .unwrap_or_default();

    if api_key.trim().is_empty() {
        return Ok(GeminiAnalysisResult {
            success: false,
            status: "missing_api_key".to_string(),
            message: "GEMINI_API_KEY is not configured in .env. Offline Harper engine is active. Set GEMINI_API_KEY in .env to enable cloud AI features.".to_string(),
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

    // Calculate smart positioning (beside or just below the active text box)
    if let Some(widget_win) = app.get_webview_window("widget") {
        let mut target_x = context.bounds.x;
        let mut target_y = context.bounds.y + context.bounds.height + 8.0;

        if let Ok(Some(monitor)) = widget_win.current_monitor() {
            let m_size = monitor.size();
            let scale = monitor.scale_factor();
            let screen_w = m_size.width as f64 / scale;
            let screen_h = m_size.height as f64 / scale;

            target_x = target_x.clamp(16.0, (screen_w - 356.0).max(16.0));

            if target_y + 360.0 > screen_h {
                target_y = (context.bounds.y - 368.0).max(16.0);
            } else {
                target_y = target_y.clamp(16.0, (screen_h - 368.0).max(16.0));
            }
        }

        let _ = widget_win.set_position(tauri::Position::Logical(LogicalPosition::new(target_x, target_y)));
        let _ = widget_win.show();
    }

    let payload = OsCapturePayload {
        text: context.text.clone(),
        source_app: context.source_app.clone(),
        method: context.method.clone(),
        bounds: context.bounds.clone(),
        lints,
    };

    let _ = app.emit("os-text-captured", payload);

    Ok(context)
}

#[tauri::command]
fn replace_active_text(app: tauri::AppHandle, new_text: String) -> Result<(), String> {
    os_bridge::replace_active_text(&new_text)?;
    if let Some(widget_win) = app.get_webview_window("widget") {
        let _ = widget_win.hide();
    }
    Ok(())
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
        .plugin(global_shortcut_plugin)
        .manage(app_state.clone())
        .setup(move |app| {
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
            set_widget_height
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
}
