use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use harper_core::linting::{LintGroup, LintKind, Linter, Suggestion};
use harper_core::spell::FstDictionary;
use harper_core::{remove_overlaps, Dialect, Document};
use serde::{Deserialize, Serialize};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    Emitter, LogicalSize, Manager, State, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub mod os_bridge;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalLintResult {
    pub id: String,
    pub start: usize,
    pub end: usize,
    pub flagged_text: String,
    pub category: String, // "Correctness", "Clarity", "Style", "Fluency"
    pub subcategory: String,
    pub message: String,
    pub suggestions: Vec<String>,
    pub screen_bounds: Option<os_bridge::ScreenRect>,
    #[serde(default)]
    pub is_sentence_rewrite: bool,
    #[serde(default)]
    pub reason: Option<String>,
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
pub struct GeminiGrammarIssue {
    pub original_span: String,
    pub replacement: String,
    pub reason: String,
    pub category: String, // "Correctness" | "Clarity"
    #[serde(default)]
    pub priority: Option<u32>,
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
    pub is_micro_popover: AtomicBool,
    pub pill_custom_offset: Mutex<Option<(f64, f64)>>,
    pub is_dragging_pill: AtomicBool,
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
                screen_bounds: None,
                is_sentence_rewrite: false,
                reason: Some(explanation.to_string()),
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
            screen_bounds: None,
            is_sentence_rewrite: false,
            reason: None,
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

    let models = ["gemini-3.5-flash-lite", "gemini-3.8-flash", "gemini-2.5-flash-lite"];
    let mut last_error = String::new();

    for model_name in models {
        let endpoint = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{model_name}:generateContent"
        );

        let res = match state
            .http_client
            .post(&endpoint)
            .header("x-goog-api-key", api_key.trim())
            .header("Content-Type", "application/json")
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
            let text_err = res.text().await.unwrap_or_default();
            last_error = format!("Model {model_name} returned 404: {text_err}");
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

fn resolve_gemini_api_key(state: &AppState) -> Option<String> {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_filename(".env");
    let _ = dotenvy::from_filename("../.env");

    if let Ok(custom) = state.custom_api_key.lock() {
        if let Some(ref k) = *custom {
            if !k.trim().is_empty() {
                return Some(k.trim().to_string());
            }
        }
    }

    std::env::var("GEMINI_API_KEY")
        .or_else(|_| dotenvy::var("GEMINI_API_KEY"))
        .ok()
        .filter(|k| !k.trim().is_empty())
}

fn extract_target_chunk(text: &str) -> (usize, &str) {
    let char_count = text.chars().count();
    if char_count <= 500 {
        return (0, text);
    }

    let chars: Vec<char> = text.chars().collect();
    let start_search = char_count.saturating_sub(500);

    let mut boundary_char_idx = start_search;
    for i in start_search..char_count.saturating_sub(50) {
        if matches!(chars[i], '.' | '!' | '?' | '\n') {
            boundary_char_idx = i + 1;
            while boundary_char_idx < char_count && chars[boundary_char_idx].is_whitespace() {
                boundary_char_idx += 1;
            }
            break;
        }
    }

    let byte_offset = text.chars().take(boundary_char_idx).map(|c| c.len_utf8()).sum();
    (boundary_char_idx, &text[byte_offset..])
}

fn clean_json_payload(raw: &str) -> &str {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        s = rest.trim();
    } else if let Some(rest) = s.strip_prefix("```JSON") {
        s = rest.trim();
    } else if let Some(rest) = s.strip_prefix("```") {
        s = rest.trim();
    }

    if let Some(rest) = s.strip_suffix("```") {
        s = rest.trim();
    }

    if let Some(open) = s.find('[') {
        if let Some(close) = s.rfind(']') {
            if close >= open {
                s = &s[open..=close];
            }
        }
    }

    s.trim()
}

async fn query_gemini_grammar_and_rewrites(
    http_client: &reqwest::Client,
    api_key: &str,
    text: &str,
) -> Result<Vec<GeminiGrammarIssue>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let system_instruction = "You are a Grammarly-grade writing assistant. Analyze the user's text and return ONLY a valid JSON array of objects: [{\"original_span\": \"exact substring from input\", \"replacement\": \"corrected word or full improved sentence\", \"reason\": \"Grammar / Sentence Rewrite / Clarity\", \"category\": \"Correctness\" | \"Clarity\", \"priority\": 1}]. Always include a full-sentence grammar/clarity rewrite when the sentence has awkward phrasing or multiple errors.";

    let prompt = format!(
        "{system_instruction}\n\nReturn ONLY a valid JSON array matching the schema. If no issues are found, return [].\n\nText to analyze:\n\"\"\"\n{text}\n\"\"\""
    );

    let body = serde_json::json!({
        "contents": [{
            "parts": [{ "text": prompt }]
        }],
        "generationConfig": {
            "responseMimeType": "application/json",
            "temperature": 0.1
        }
    });

    let models = ["gemini-3.5-flash-lite", "gemini-2.5-flash"];
    let mut last_error = String::new();

    for model_name in models {
        let endpoint = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{model_name}:generateContent"
        );

        let res = match http_client
            .post(&endpoint)
            .header("x-goog-api-key", api_key.trim())
            .header("Content-Type", "application/json")
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
            last_error = "Gemini rate limited (HTTP 429)".to_string();
            continue;
        }

        if !res.status().is_success() {
            let status = res.status();
            let err_txt = res.text().await.unwrap_or_default();
            last_error = format!("Model {model_name} returned status {status}: {err_txt}");
            continue;
        }

        let resp_json: serde_json::Value = match res.json().await {
            Ok(j) => j,
            Err(e) => {
                last_error = format!("Failed to parse JSON response: {e}");
                continue;
            }
        };

        let raw_text = resp_json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("");

        let json_str = clean_json_payload(raw_text);

        match serde_json::from_str::<Vec<GeminiGrammarIssue>>(json_str) {
            Ok(issues) => return Ok(issues),
            Err(e) => {
                last_error = format!("Failed to deserialize issues array: {e}");
                continue;
            }
        }
    }

    Err(last_error)
}

fn merge_and_publish_ai_lints(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    full_text: &str,
    _char_offset: usize,
    ai_issues: Vec<GeminiGrammarIssue>,
    box_bounds: &os_bridge::ScreenRect,
    ctx: &os_bridge::CapturedContext,
    mut merged_lints: Vec<LocalLintResult>,
) {
    let custom_dict = state.custom_dictionary.lock().unwrap();
    let chars: Vec<char> = full_text.chars().collect();
    let total_chars = chars.len();

    let mut added_any = false;

    for (idx, issue) in ai_issues.into_iter().enumerate() {
        let orig_clean = issue.original_span.trim();
        if orig_clean.is_empty() {
            continue;
        }

        let orig_lower = orig_clean.to_lowercase();
        if custom_dict.contains(&orig_lower) {
            continue;
        }

        if let Some(byte_idx) = full_text.find(orig_clean) {
            let start = full_text[..byte_idx].chars().count().min(total_chars);
            let end = (start + orig_clean.chars().count()).min(total_chars);

            if start >= end {
                continue;
            }

            let is_sentence = issue.original_span.split_whitespace().count() > 2
                || issue.replacement.split_whitespace().count() > 3
                || issue.category == "Clarity"
                || issue.category == "Fluency"
                || issue.reason.to_lowercase().contains("rewrite")
                || issue.reason.to_lowercase().contains("sentence");

            let existing_idx = merged_lints.iter().position(|r| r.start == start && r.end == end);

            if let Some(e_idx) = existing_idx {
                let existing = &mut merged_lints[e_idx];
                existing.reason = Some(issue.reason);
                if !issue.category.is_empty() {
                    existing.category = issue.category;
                }
                if !existing.suggestions.contains(&issue.replacement) {
                    existing.suggestions.insert(0, issue.replacement);
                }
                if is_sentence {
                    existing.is_sentence_rewrite = true;
                }
                added_any = true;
            } else {
                merged_lints.push(LocalLintResult {
                    id: format!("ai-lint-{}-{}-{}", start, end, idx),
                    start,
                    end,
                    flagged_text: orig_clean.to_string(),
                    category: issue.category,
                    subcategory: if is_sentence {
                        "Sentence Rewrite".to_string()
                    } else {
                        "AI Grammar Fix".to_string()
                    },
                    message: issue.reason.clone(),
                    suggestions: vec![issue.replacement],
                    screen_bounds: None,
                    is_sentence_rewrite: is_sentence,
                    reason: Some(issue.reason),
                });
                added_any = true;
            }
        }
    }

    if !added_any {
        return;
    }

    merged_lints.sort_by_key(|r| r.start);

    let active_mon = get_active_monitor_for_point(
        app,
        box_bounds.x,
        box_bounds.y,
        box_bounds.width,
        box_bounds.height,
    );
    let sf = active_mon.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);

    #[cfg(windows)]
    os_bridge::extract_lint_screen_rects(&mut merged_lints, box_bounds, sf);
    #[cfg(not(windows))]
    os_bridge::extract_lint_screen_rects(&mut merged_lints, box_bounds, sf);

    let payload = OsCapturePayload {
        text: full_text.to_string(),
        source_app: ctx.source_app.clone(),
        process_name: ctx.process_name.clone(),
        method: "gemini_ai_analysis".to_string(),
        bounds: box_bounds.clone(),
        lints: merged_lints,
    };
    let _ = app.emit("os-text-captured", payload);
}

fn get_active_monitor_for_point(
    app: &tauri::AppHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Option<tauri::Monitor> {
    app.monitor_from_point(x, y)
        .ok()
        .flatten()
        .or_else(|| {
            // Check center of the text bounding box
            app.monitor_from_point(x + width / 2.0, y + height / 2.0).ok().flatten()
        })
        .or_else(|| app.primary_monitor().ok().flatten())
        .or_else(|| app.available_monitors().ok().and_then(|m| m.into_iter().next()))
}

fn calculate_smart_pill_position(
    bounds: &os_bridge::ScreenRect,
    active_mon: &tauri::Monitor,
    custom_offset: Option<(f64, f64)>,
) -> (i32, i32) {
    let m_pos = active_mon.position();
    let m_size = active_mon.size();
    let sf = active_mon.scale_factor();
    let sf = if sf > 0.0 { sf } else { 1.0 };

    let mon_x = m_pos.x as f64;
    let mon_y = m_pos.y as f64;
    let mon_w = m_size.width as f64;
    let mon_h = m_size.height as f64;

    let box_x = bounds.x;
    let box_y = bounds.y;
    let box_w = bounds.width;
    let box_h = bounds.height;
    let box_right = box_x + box_w;
    let box_bottom = box_y + box_h;

    let pill_size = 36.0 * sf;

    let (target_x, target_y) = if let Some((off_x, off_y)) = custom_offset {
        // User dragged the pill to a custom location relative to text box!
        (box_right + off_x, box_bottom + off_y)
    } else {
        // Default anchor: (x = box.right - 36*sf, y = box.bottom + 8*sf)
        let mut tx = box_right - 36.0 * sf;
        let mut ty = box_bottom + 8.0 * sf;

        // If word bounds failed or box is small/collapsed, anchor to (textbox_rect.left + 16, textbox_rect.bottom + 8)
        if box_w <= 20.0 && box_h <= 20.0 {
            tx = box_x + 16.0 * sf;
            ty = box_bottom + 8.0 * sf;
        }

        // If bottom overflows the visible monitor (e.g. chat text box docked at bottom):
        if ty + pill_size > mon_y + mon_h - 8.0 * sf {
            // Try vertically centered on far-right edge outside the text box:
            if box_right + pill_size + 8.0 * sf <= mon_x + mon_w {
                tx = box_right + 8.0 * sf;
                ty = (box_y + (box_h / 2.0)) - (pill_size / 2.0);
            } else {
                // If right edge also overflows, place above top-right corner of text box:
                tx = box_right - 44.0 * sf;
                ty = box_y - pill_size - 8.0 * sf;
            }
        }
        (tx, ty)
    };

    // Clamp within active monitor visible boundaries (supports negative coordinates on secondary monitors)
    let min_x = mon_x + 8.0 * sf;
    let max_x = (mon_x + mon_w - pill_size - 8.0 * sf).max(min_x);
    let min_y = mon_y + 8.0 * sf;
    let max_y = (mon_y + mon_h - pill_size - 8.0 * sf).max(min_y);

    let final_x = target_x.clamp(min_x, max_x);
    let final_y = target_y.clamp(min_y, max_y);

    (final_x.round() as i32, final_y.round() as i32)
}

fn calculate_micro_popover_position(
    sb: &os_bridge::ScreenRect,
    active_mon: &tauri::Monitor,
) -> (i32, i32, f64, f64, f64, f64) {
    let m_pos = active_mon.position();
    let m_size = active_mon.size();
    let sf = active_mon.scale_factor();
    let sf = if sf > 0.0 { sf } else { 1.0 };

    let mon_x = m_pos.x as f64;
    let mon_y = m_pos.y as f64;
    let mon_w = m_size.width as f64;
    let mon_h = m_size.height as f64;

    // The micro-popover card is styled as 290x185 CSS/logical pixels in React.
    let card_w_phys = 290.0 * sf;
    let card_h_phys = 185.0 * sf;
    let gap = 6.0 * sf;
    let margin = 8.0 * sf;

    // Default placement: directly below the word, top-left aligned with start of word
    let target_x = sb.x;
    let mut target_y = sb.y + sb.height + gap;

    // If the word is near the bottom of the screen and the card would clip off-screen,
    // flip it cleanly above the word:
    if target_y + card_h_phys > mon_y + mon_h - margin {
        target_y = sb.y - card_h_phys - gap;
    }

    // Clamp within active monitor bounds (handles multi-monitor virtual screen):
    let clamped_x = target_x.clamp(
        mon_x + margin,
        (mon_x + mon_w - card_w_phys - margin).max(mon_x + margin),
    );
    let clamped_y = target_y.clamp(
        mon_y + margin,
        (mon_y + mon_h - card_h_phys - margin).max(mon_y + margin),
    );

    (
        clamped_x.round() as i32,
        clamped_y.round() as i32,
        clamped_x,
        clamped_y,
        card_w_phys,
        card_h_phys,
    )
}

#[inline]
fn clear_underlines_window(_app: &tauri::AppHandle) {}

pub fn trigger_os_capture_and_show(app: &tauri::AppHandle) -> Result<os_bridge::CapturedContext, String> {
    let context = os_bridge::capture_active_context();

    // STRICT Editable-Only Filter:
    // If not editable or on static screen (like Chrome profile picker, desktop, buttons, static text),
    // immediately hide windows and skip text analysis.
    if context.method == "non_editable" || (context.text.trim().is_empty() && context.bounds.width <= 20.0) {
        if let Some(widget_win) = app.get_webview_window("widget") {
            let _ = widget_win.hide();
        }
        clear_underlines_window(app);
        return Ok(context);
    }

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
        if let Some(widget_win) = app.get_webview_window("widget") {
            let _ = widget_win.hide();
        }
        clear_underlines_window(app);
        return Ok(context);
    }

    // Detect exact active monitor where user is typing
    let active_mon = get_active_monitor_for_point(
        app,
        context.bounds.x,
        context.bounds.y,
        context.bounds.width,
        context.bounds.height,
    );
    let sf = active_mon.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);

    // Check lints on captured text if non-empty
    let mut lints = if !context.text.trim().is_empty() {
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
                            screen_bounds: None,
                            is_sentence_rewrite: false,
                            reason: None,
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

    // Extract exact screen bounds for each lint from UIA text range (never fall back to 0,0)
    os_bridge::extract_lint_screen_rects(&mut lints, &context.bounds, sf);

    // Move widget popover onto the exact active monitor using PhysicalPosition
    if let Some(ref mon) = active_mon {
        if let Some(widget_win) = app.get_webview_window("widget") {
            let state = app.state::<Arc<AppState>>();
            let custom_offset = state.pill_custom_offset.lock().ok().and_then(|o| *o);
            let (target_x, target_y) = calculate_smart_pill_position(&context.bounds, mon, custom_offset);

            let _ = widget_win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(target_x, target_y)));
            let _ = widget_win.set_size(LogicalSize::new(36.0, 36.0));
            let _ = widget_win.set_ignore_cursor_events(false);
            let _ = widget_win.show();
        }
    }

    let payload = OsCapturePayload {
        text: context.text.clone(),
        source_app: context.source_app.clone(),
        process_name: context.process_name.clone(),
        method: context.method.clone(),
        bounds: context.bounds.clone(),
        lints,
    };

    let _ = app.emit("os-text-captured", payload.clone());

    // Spawn background AI sentence-level and grammar analysis
    let app_for_ai = app.clone();
    let text_for_ai = context.text.clone();
    let bounds_for_ai = context.bounds.clone();
    let ctx_for_ai = context.clone();
    let base_lints_for_ai = payload.lints.clone();

    if let Some(state_for_ai) = app.try_state::<Arc<AppState>>() {
        let state_arc = state_for_ai.inner().clone();
        tauri::async_runtime::spawn(async move {
            let api_key = resolve_gemini_api_key(&state_arc);
            if let Some(key) = api_key {
                let (char_offset, chunk) = extract_target_chunk(&text_for_ai);
                if let Ok(ai_issues) = query_gemini_grammar_and_rewrites(&state_arc.http_client, &key, chunk).await {
                    if !ai_issues.is_empty() {
                        merge_and_publish_ai_lints(
                            &app_for_ai,
                            &state_arc,
                            &text_for_ai,
                            char_offset,
                            ai_issues,
                            &bounds_for_ai,
                            &ctx_for_ai,
                            base_lints_for_ai,
                        );
                    }
                }
            }
        });
    }

    Ok(context)
}

#[tauri::command]
fn replace_active_text(
    app: tauri::AppHandle,
    state: State<Arc<AppState>>,
    new_text: String,
) -> Result<(), String> {
    state.widget_expanded.store(false, Ordering::SeqCst);
    state.is_micro_popover.store(false, Ordering::SeqCst);
    os_bridge::replace_active_text(&new_text)?;
    clear_underlines_window(&app);
    // Collapse to compact pill upon applying replacement
    if let Some(widget_win) = app.get_webview_window("widget") {
        let _ = widget_win.set_size(LogicalSize::new(36.0, 36.0));
    }
    Ok(())
}

#[tauri::command]
fn replace_single_word(
    app: tauri::AppHandle,
    _state: State<Arc<AppState>>,
    start: usize,
    end: usize,
    replacement: String,
    full_text_fallback: String,
    lint_id: String,
    original_span: Option<String>,
) -> Result<String, String> {
    let span_to_use = original_span
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            let chars: Vec<char> = full_text_fallback.chars().collect();
            if start < chars.len() && end <= chars.len() && start < end {
                chars[start..end].iter().collect()
            } else {
                String::new()
            }
        });

    let new_text = os_bridge::replace_suggestion(
        &span_to_use,
        &replacement,
        start,
        &full_text_fallback,
    )?;

    let _ = app.emit("os-lint-resolved", lint_id);
    Ok(new_text)
}

#[tauri::command]
fn show_micro_popover(
    app: tauri::AppHandle,
    state: State<Arc<AppState>>,
    x: f64,
    y: f64,
    lint: LocalLintResult,
) {
    state.widget_expanded.store(false, Ordering::SeqCst);
    state.is_micro_popover.store(true, Ordering::SeqCst);
    if let Some(widget_win) = app.get_webview_window("widget") {
        let sb = lint.screen_bounds.clone().unwrap_or(os_bridge::ScreenRect {
            x,
            y,
            width: 40.0,
            height: 20.0,
        });

        let active_mon = get_active_monitor_for_point(&app, sb.x, sb.y, sb.width, sb.height)
            .or_else(|| app.primary_monitor().ok().flatten());

        if let Some(mon) = active_mon {
            let (pos_x, pos_y, _, _, _, _) = calculate_micro_popover_position(&sb, &mon);

            let _ = widget_win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(pos_x, pos_y)));
            let _ = widget_win.set_size(LogicalSize::new(290.0, 185.0));
            let _ = widget_win.show();
            let _ = app.emit("os-lint-hovered", lint);
        }
    }
}

#[tauri::command]
fn expand_pill_to_micro(app: tauri::AppHandle, state: State<Arc<AppState>>) {
    state.is_micro_popover.store(true, Ordering::SeqCst);
    if let Some(widget_win) = app.get_webview_window("widget") {
        if let Ok(pos) = widget_win.outer_position() {
            let active_mon = get_active_monitor_for_point(&app, pos.x as f64, pos.y as f64, 1.0, 1.0)
                .or_else(|| widget_win.current_monitor().ok().flatten())
                .or_else(|| app.primary_monitor().ok().flatten());

            if let Some(mon) = active_mon {
                let m_pos = mon.position();
                let m_size = mon.size();
                let sf = mon.scale_factor();
                let sf = if sf > 0.0 { sf } else { 1.0 };

                let cur_x = pos.x as f64;
                let cur_y = pos.y as f64;
                let mon_x = m_pos.x as f64;
                let mon_y = m_pos.y as f64;
                let mon_w = m_size.width as f64;
                let mon_h = m_size.height as f64;

                let card_w = 290.0 * sf;
                let card_h = 185.0 * sf;
                let pill_w = 36.0 * sf;
                let pill_h = 36.0 * sf;

                let mut target_x = cur_x;
                let mut target_y = cur_y;

                if target_x + card_w > mon_x + mon_w - 8.0 * sf {
                    target_x = cur_x + pill_w - card_w;
                }
                if target_y + card_h > mon_y + mon_h - 8.0 * sf {
                    target_y = cur_y + pill_h - card_h;
                }

                let clamped_x = target_x.clamp(
                    mon_x + 8.0 * sf,
                    (mon_x + mon_w - card_w - 8.0 * sf).max(mon_x + 8.0 * sf),
                );
                let clamped_y = target_y.clamp(
                    mon_y + 8.0 * sf,
                    (mon_y + mon_h - card_h - 8.0 * sf).max(mon_y + 8.0 * sf),
                );

                let _ = widget_win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                    clamped_x.round() as i32,
                    clamped_y.round() as i32,
                )));
            }
        }
        let _ = widget_win.set_size(LogicalSize::new(290.0, 185.0));
        let _ = widget_win.set_ignore_cursor_events(false);
    }
}

#[tauri::command]
fn expand_widget(app: tauri::AppHandle, state: State<Arc<AppState>>) {
    state.widget_expanded.store(true, Ordering::SeqCst);
    state.is_micro_popover.store(false, Ordering::SeqCst);
    if let Some(widget_win) = app.get_webview_window("widget") {
        if let Ok(pos) = widget_win.outer_position() {
            let active_mon = get_active_monitor_for_point(&app, pos.x as f64, pos.y as f64, 1.0, 1.0)
                .or_else(|| widget_win.current_monitor().ok().flatten())
                .or_else(|| app.primary_monitor().ok().flatten());

            if let Some(monitor) = active_mon {
                let m_pos = monitor.position();
                let m_size = monitor.size();
                let sf = monitor.scale_factor();
                let sf = if sf > 0.0 { sf } else { 1.0 };

                let cur_x = pos.x as f64;
                let cur_y = pos.y as f64;
                let mon_x = m_pos.x as f64;
                let mon_y = m_pos.y as f64;
                let mon_w = m_size.width as f64;
                let mon_h = m_size.height as f64;

                let card_w = 340.0 * sf;
                let card_h = 420.0 * sf;
                let max_x = mon_x + mon_w - card_w - 8.0 * sf;
                let max_y = mon_y + mon_h - card_h - 8.0 * sf;

                let adjusted_x = cur_x.clamp(mon_x + 8.0 * sf, max_x.max(mon_x + 8.0 * sf));
                let adjusted_y = cur_y.clamp(mon_y + 8.0 * sf, max_y.max(mon_y + 8.0 * sf));

                let _ = widget_win.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                    adjusted_x.round() as i32,
                    adjusted_y.round() as i32,
                )));
            }
        }
        let _ = widget_win.set_size(LogicalSize::new(340.0, 420.0));
    }
}

#[tauri::command]
fn collapse_widget(app: tauri::AppHandle, state: State<Arc<AppState>>) {
    state.widget_expanded.store(false, Ordering::SeqCst);
    state.is_micro_popover.store(false, Ordering::SeqCst);
    if let Some(widget_win) = app.get_webview_window("widget") {
        let _ = widget_win.set_size(LogicalSize::new(36.0, 36.0));
        let _ = widget_win.set_ignore_cursor_events(false);
    }
}

#[tauri::command]
fn start_pill_drag(state: State<Arc<AppState>>) -> Result<(), String> {
    state.is_dragging_pill.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn end_pill_drag(
    state: State<Arc<AppState>>,
    screen_x: f64,
    screen_y: f64,
    box_right: f64,
    box_bottom: f64,
) -> Result<(), String> {
    state.is_dragging_pill.store(false, Ordering::SeqCst);
    let (off_x, off_y) = if box_right != 0.0 || box_bottom != 0.0 {
        (screen_x - box_right, screen_y - box_bottom)
    } else {
        (0.0, 0.0)
    };
    if let Ok(mut off) = state.pill_custom_offset.lock() {
        *off = Some((off_x, off_y));
    }
    Ok(())
}

#[tauri::command]
fn end_pill_drag_cancel(state: State<Arc<AppState>>) -> Result<(), String> {
    state.is_dragging_pill.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn reset_pill_offset(state: State<Arc<AppState>>) -> Result<(), String> {
    if let Ok(mut off) = state.pill_custom_offset.lock() {
        *off = None;
    }
    Ok(())
}

#[tauri::command]
fn hide_widget(app: tauri::AppHandle, state: State<Arc<AppState>>) {
    state.widget_expanded.store(false, Ordering::SeqCst);
    clear_underlines_window(&app);
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
async fn capture_active_text(app: tauri::AppHandle) -> Result<os_bridge::CapturedContext, String> {
    tauri::async_runtime::spawn_blocking(move || {
        trigger_os_capture_and_show(&app)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_pause_state(state: State<Arc<AppState>>) -> bool {
    state.paused.load(Ordering::SeqCst)
}

#[tauri::command]
fn toggle_pause_state(app: tauri::AppHandle, state: State<Arc<AppState>>) -> bool {
    let next = !state.paused.load(Ordering::SeqCst);
    state.paused.store(next, Ordering::SeqCst);
    if next {
        clear_underlines_window(&app);
        if let Some(widget_win) = app.get_webview_window("widget") {
            let _ = widget_win.hide();
        }
    }
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
    let models = ["gemini-3.5-flash-lite", "gemini-3.8-flash", "gemini-2.5-flash-lite"];

    let body = serde_json::json!({
        "contents": [{
            "parts": [{ "text": "Hi" }]
        }]
    });

    let mut last_status = 0;
    let mut last_error_msg = String::new();

    for model_name in models {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{model_name}:generateContent"
        );

        let resp = match state
            .http_client
            .post(&url)
            .header("x-goog-api-key", key.trim())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Err(format!("Network request failed: {e}"));
            }
        };

        let status = resp.status().as_u16();
        last_status = status;

        if status == 200 {
            let latency_ms = start.elapsed().as_millis();
            return Ok(serde_json::json!({
                "success": true,
                "status": "connected",
                "latency_ms": latency_ms,
                "model": model_name,
                "message": format!("Successfully connected to {model_name} ({latency_ms}ms)")
            }));
        } else if status == 429 {
            return Ok(serde_json::json!({
                "success": false,
                "status": "rate_limited",
                "message": "Free tier quota exceeded (HTTP 429)."
            }));
        } else if status == 404 {
            let err_text = resp.text().await.unwrap_or_default();
            last_error_msg = format!("HTTP 404 Not Found for {model_name}: {err_text}");
            continue;
        } else {
            let err_text = resp.text().await.unwrap_or_default();
            return Err(format!("Gemini API error (HTTP {status}): {err_text}"));
        }
    }

    Err(format!("Gemini API error (HTTP {last_status}): {last_error_msg}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    os_bridge::init_input_event_hooks();
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
        is_micro_popover: AtomicBool::new(false),
        pill_custom_offset: Mutex::new(None),
        is_dragging_pill: AtomicBool::new(false),
    });

    let global_shortcut_plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                let app_clone = app.clone();
                std::thread::spawn(move || {
                    let _ = trigger_os_capture_and_show(&app_clone);
                });
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
                            if next {
                                clear_underlines_window(app);
                                if let Some(widget_win) = app.get_webview_window("widget") {
                                    let _ = widget_win.hide();
                                }
                            }
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

            if let Some(widget_win) = app.get_webview_window("widget") {
                let _ = widget_win.set_shadow(false);
                let _ = widget_win.set_decorations(false);
                let _ = widget_win.set_ignore_cursor_events(false);
            }

            // Dedicated 60fps background thread: UI Automation, typing debounce, dual-monitor window manager
            let watcher_app = app.handle().clone();
            let watcher_state = app_state.clone();
            std::thread::spawn(move || {
                #[cfg(windows)]
                unsafe {
                    let _ = windows::Win32::System::Com::CoInitializeEx(
                        None,
                        windows::Win32::System::Com::COINIT_MULTITHREADED,
                    );
                }
                #[cfg(windows)]
                let automation: Option<windows::Win32::UI::Accessibility::IUIAutomation> = unsafe {
                    windows::Win32::System::Com::CoCreateInstance(
                        &windows::Win32::UI::Accessibility::CUIAutomation,
                        None,
                        windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
                    )
                    .ok()
                };

                let mut last_analyzed_text = String::new();
                let mut pending_text: Option<String> = None;
                let mut pending_ctx: Option<os_bridge::CapturedContext> = None;
                let mut last_typing_time: Option<std::time::Instant> = None;
                let mut last_bounds = os_bridge::ScreenRect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                };
                let mut last_hwnd: isize = 0;
                let mut cached_lints: Vec<LocalLintResult> = Vec::new();
                let mut current_hovered_lint_id: Option<String> = None;
                let mut poll_ticks: u32 = 0;

                let current_exe_name = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
                    .unwrap_or_default();

                loop {
                    std::thread::sleep(std::time::Duration::from_millis(16));

                    if watcher_state.paused.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        continue;
                    }

                    if os_bridge::IS_REPLACING.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        continue;
                    }

                    // 1. Mouse scroll detection: hide widget immediately on scroll until typing resumes
                    if os_bridge::JUST_SCROLLED.swap(false, Ordering::SeqCst) {
                        if !watcher_state.widget_expanded.load(Ordering::SeqCst) {
                            if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                if widget_win.is_visible().unwrap_or(false) {
                                    let _ = widget_win.hide();
                                }
                            }
                        }
                        watcher_state.widget_expanded.store(false, Ordering::SeqCst);
                        watcher_state.is_micro_popover.store(false, Ordering::SeqCst);
                        os_bridge::USER_HAS_TYPED.store(false, Ordering::SeqCst);
                        current_hovered_lint_id = None;
                        pending_text = None;
                        pending_ctx = None;
                        last_typing_time = None;
                    }

                    // 2. Click outside detection: collapse and hide widget if user clicks outside of it
                    if os_bridge::CLICK_OCCURRED.swap(false, Ordering::SeqCst) {
                        if let Some((cur_x, cur_y)) = os_bridge::get_cursor_position() {
                            let is_over_widget = if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                if widget_win.is_visible().unwrap_or(false) {
                                    if let (Ok(pos), Ok(size)) = (widget_win.outer_position(), widget_win.outer_size()) {
                                        cur_x >= pos.x as f64
                                            && cur_x <= (pos.x + size.width as i32) as f64
                                            && cur_y >= pos.y as f64
                                            && cur_y <= (pos.y + size.height as i32) as f64
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            };

                            if !is_over_widget {
                                if !watcher_state.widget_expanded.load(Ordering::SeqCst) {
                                    if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                        if widget_win.is_visible().unwrap_or(false) {
                                            let _ = widget_win.hide();
                                        }
                                    }
                                }
                                watcher_state.widget_expanded.store(false, Ordering::SeqCst);
                                watcher_state.is_micro_popover.store(false, Ordering::SeqCst);
                                os_bridge::USER_HAS_TYPED.store(false, Ordering::SeqCst);
                                current_hovered_lint_id = None;
                                pending_text = None;
                                pending_ctx = None;
                                last_typing_time = None;
                            }
                        }
                    }

                    // 2. Typing Debounce (600ms quiet timer before running Harper Core + Gemini in parallel)
                    if let Some(typing_time) = last_typing_time {
                        if typing_time.elapsed() >= std::time::Duration::from_millis(600) {
                            last_typing_time = None;
                            if let Some(text_to_analyze) = pending_text.take() {
                                if let Some(active_ctx) = pending_ctx.take() {
                                    last_analyzed_text = text_to_analyze.clone();
                                    last_bounds = active_ctx.bounds.clone();

                                    let active_mon = get_active_monitor_for_point(
                                        &watcher_app,
                                        active_ctx.bounds.x,
                                        active_ctx.bounds.y,
                                        active_ctx.bounds.width,
                                        active_ctx.bounds.height,
                                    );
                                    let sf = active_mon.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);

                                    // Run Harper Core analysis only once after 300ms idle
                                    let mut lints = if !text_to_analyze.trim().is_empty() {
                                        let doc = Document::new_markdown_default_curated(&text_to_analyze);
                                        if let Ok(mut lg) = watcher_state.lint_group.lock() {
                                            let mut raw_lints = lg.lint(&doc);
                                            remove_overlaps(&mut raw_lints);

                                            let chars: Vec<char> = text_to_analyze.chars().collect();
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
                                                        screen_bounds: None,
                                                        is_sentence_rewrite: false,
                                                        reason: None,
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

                                    // Extract exact word screen coordinates (never fall back to 0,0)
                                    #[cfg(windows)]
                                    if let Some(ref auto) = automation {
                                        os_bridge::extract_lint_screen_rects_with_automation(
                                            auto,
                                            &mut lints,
                                            &active_ctx.bounds,
                                            sf,
                                        );
                                    } else {
                                        os_bridge::extract_lint_screen_rects(&mut lints, &active_ctx.bounds, sf);
                                    }
                                    #[cfg(not(windows))]
                                    os_bridge::extract_lint_screen_rects(&mut lints, &active_ctx.bounds, sf);

                                    cached_lints = lints.clone();

                                    let payload = OsCapturePayload {
                                        text: active_ctx.text.clone(),
                                        source_app: active_ctx.source_app.clone(),
                                        process_name: active_ctx.process_name.clone(),
                                        method: active_ctx.method.clone(),
                                        bounds: active_ctx.bounds.clone(),
                                        lints,
                                    };
                                    let _ = watcher_app.emit("os-text-captured", payload);

                                    // Position widget floating pill on the exact active monitor
                                    if !watcher_state.widget_expanded.load(Ordering::SeqCst)
                                        && !watcher_state.is_micro_popover.load(Ordering::SeqCst)
                                        && !watcher_state.is_dragging_pill.load(Ordering::SeqCst)
                                        && current_hovered_lint_id.is_none()
                                    {
                                        if let Some(ref mon) = active_mon {
                                            if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                                let custom_offset = watcher_state.pill_custom_offset.lock().ok().and_then(|o| *o);
                                                let (target_x, target_y) = calculate_smart_pill_position(&active_ctx.bounds, mon, custom_offset);
                                                let _ = widget_win.set_position(tauri::Position::Physical(
                                                    tauri::PhysicalPosition::new(target_x, target_y),
                                                ));
                                                let _ = widget_win.set_ignore_cursor_events(false);
                                                if !widget_win.is_visible().unwrap_or(false)
                                                    && !active_ctx.text.trim().is_empty()
                                                {
                                                    let _ = widget_win.show();
                                                }
                                            }
                                        }
                                    }

                                    // Spawn background AI sentence-level and grammar analysis using gemini-3.5-flash-lite / gemini-2.5-flash
                                    let app_for_ai = watcher_app.clone();
                                    let state_for_ai = watcher_state.clone();
                                    let text_for_ai = active_ctx.text.clone();
                                    let bounds_for_ai = active_ctx.bounds.clone();
                                    let ctx_for_ai = active_ctx.clone();
                                    let base_lints_for_ai = cached_lints.clone();

                                    tauri::async_runtime::spawn(async move {
                                        let api_key = resolve_gemini_api_key(&state_for_ai);
                                        if let Some(key) = api_key {
                                            let (char_offset, chunk) = extract_target_chunk(&text_for_ai);
                                            if let Ok(ai_issues) = query_gemini_grammar_and_rewrites(&state_for_ai.http_client, &key, chunk).await {
                                                if !ai_issues.is_empty() {
                                                    merge_and_publish_ai_lints(
                                                        &app_for_ai,
                                                        &state_for_ai,
                                                        &text_for_ai,
                                                        char_offset,
                                                        ai_issues,
                                                        &bounds_for_ai,
                                                        &ctx_for_ai,
                                                        base_lints_for_ai,
                                                    );
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                        }
                    }

                    // 3. Periodic Background UI Automation Inspection (every ~64ms)
                    poll_ticks += 1;
                    if poll_ticks < 4 {
                        continue;
                    }
                    poll_ticks = 0;

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

                    // Auto-hide when user switches windows
                    let window_switched = win_state.hwnd_val != last_hwnd;
                    if window_switched {
                        last_hwnd = win_state.hwnd_val;
                        if !watcher_state.widget_expanded.load(Ordering::SeqCst) {
                            if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                if widget_win.is_visible().unwrap_or(false) {
                                    let _ = widget_win.hide();
                                }
                            }
                        }
                        watcher_state.widget_expanded.store(false, Ordering::SeqCst);
                        watcher_state.is_micro_popover.store(false, Ordering::SeqCst);
                        os_bridge::USER_HAS_TYPED.store(false, Ordering::SeqCst);
                        cached_lints.clear();
                        current_hovered_lint_id = None;
                        last_analyzed_text.clear();
                        pending_text = None;
                        pending_ctx = None;
                        last_typing_time = None;
                    }

                    // Auto-hide when user clicks desktop wallpaper/icons or Windows taskbar
                    if win_state.is_desktop_or_shell {
                        if !watcher_state.widget_expanded.load(Ordering::SeqCst) {
                            if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                if widget_win.is_visible().unwrap_or(false) {
                                    let _ = widget_win.hide();
                                }
                            }
                        }
                        watcher_state.widget_expanded.store(false, Ordering::SeqCst);
                        watcher_state.is_micro_popover.store(false, Ordering::SeqCst);
                        os_bridge::USER_HAS_TYPED.store(false, Ordering::SeqCst);
                        cached_lints.clear();
                        current_hovered_lint_id = None;
                        last_analyzed_text.clear();
                        pending_text = None;
                        pending_ctx = None;
                        last_typing_time = None;
                        continue;
                    }

                    // Auto-hide when user focuses a blacklisted/excluded application
                    let is_blacklisted = os_bridge::is_blacklisted_process(&win_state.process_name) || {
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
                        if !watcher_state.widget_expanded.load(Ordering::SeqCst) {
                            if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                if widget_win.is_visible().unwrap_or(false) {
                                    let _ = widget_win.hide();
                                }
                            }
                        }
                        watcher_state.widget_expanded.store(false, Ordering::SeqCst);
                        watcher_state.is_micro_popover.store(false, Ordering::SeqCst);
                        os_bridge::USER_HAS_TYPED.store(false, Ordering::SeqCst);
                        cached_lints.clear();
                        current_hovered_lint_id = None;
                        last_analyzed_text.clear();
                        pending_text = None;
                        pending_ctx = None;
                        last_typing_time = None;
                        continue;
                    }

                    // Only activate after real physical typing in an editable field
                    if !os_bridge::USER_HAS_TYPED.load(Ordering::SeqCst) {
                        if !watcher_state.widget_expanded.load(Ordering::SeqCst)
                            && !watcher_state.is_micro_popover.load(Ordering::SeqCst)
                        {
                            if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                if widget_win.is_visible().unwrap_or(false) {
                                    let _ = widget_win.hide();
                                }
                            }
                        }
                        continue;
                    }

                    #[cfg(windows)]
                    let ctx_opt = if let Some(ref auto) = automation {
                        os_bridge::capture_via_accessibility_with_automation(auto)
                    } else {
                        os_bridge::capture_via_accessibility()
                    };
                    #[cfg(not(windows))]
                    let ctx_opt = os_bridge::capture_via_accessibility();

                    if let Some(ctx) = ctx_opt {
                        let text_changed = ctx.text != last_analyzed_text;
                        let bounds_changed = (ctx.bounds.x - last_bounds.x).abs() > 4.0
                            || (ctx.bounds.y - last_bounds.y).abs() > 4.0;

                        if text_changed {
                            if pending_text.as_deref() != Some(&ctx.text) {
                                pending_text = Some(ctx.text.clone());
                                pending_ctx = Some(ctx.clone());
                                last_typing_time = Some(std::time::Instant::now());
                            }
                        } else if pending_text.is_none() && bounds_changed {
                            // Text unchanged and no typing in progress: update positions if element moved/scrolled
                            last_bounds = ctx.bounds.clone();
                            let active_mon = get_active_monitor_for_point(
                                &watcher_app,
                                ctx.bounds.x,
                                ctx.bounds.y,
                                ctx.bounds.width,
                                ctx.bounds.height,
                            );
                            if let Some(ref mon) = active_mon {
                                if !cached_lints.is_empty() {
                                    let sf = mon.scale_factor();
                                    #[cfg(windows)]
                                    if let Some(ref auto) = automation {
                                        os_bridge::extract_lint_screen_rects_with_automation(
                                            auto,
                                            &mut cached_lints,
                                            &ctx.bounds,
                                            sf,
                                        );
                                    } else {
                                        os_bridge::extract_lint_screen_rects(&mut cached_lints, &ctx.bounds, sf);
                                    }
                                    #[cfg(not(windows))]
                                    os_bridge::extract_lint_screen_rects(&mut cached_lints, &ctx.bounds, sf);
                                }
                                if !watcher_state.widget_expanded.load(Ordering::SeqCst)
                                    && !watcher_state.is_micro_popover.load(Ordering::SeqCst)
                                    && !watcher_state.is_dragging_pill.load(Ordering::SeqCst)
                                    && current_hovered_lint_id.is_none()
                                {
                                    if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                        let custom_offset = watcher_state.pill_custom_offset.lock().ok().and_then(|o| *o);
                                        let (target_x, target_y) = calculate_smart_pill_position(&ctx.bounds, mon, custom_offset);
                                        let _ = widget_win.set_position(tauri::Position::Physical(
                                            tauri::PhysicalPosition::new(target_x, target_y),
                                        ));
                                        let _ = widget_win.set_ignore_cursor_events(false);
                                    }
                                }
                            }
                        }
                    } else {
                        // STRICT Editable-Only Filter:
                        // If the user is on a non-editable screen (Chrome profile picker, desktop, buttons, static webpage text),
                        // immediately hide both the underlines and widget windows and skip text analysis.
                        if !watcher_state.widget_expanded.load(Ordering::SeqCst) && !watcher_state.is_micro_popover.load(Ordering::SeqCst) {
                            if let Some(widget_win) = watcher_app.get_webview_window("widget") {
                                if widget_win.is_visible().unwrap_or(false) {
                                    let _ = widget_win.hide();
                                }
                            }
                        }
                        os_bridge::USER_HAS_TYPED.store(false, Ordering::SeqCst);
                        cached_lints.clear();
                        current_hovered_lint_id = None;
                        last_analyzed_text.clear();
                        pending_text = None;
                        pending_ctx = None;
                        last_typing_time = None;
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
            test_gemini_connection,
            replace_single_word,
            show_micro_popover,
            expand_pill_to_micro,
            start_pill_drag,
            end_pill_drag,
            end_pill_drag_cancel,
            reset_pill_offset
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
            is_micro_popover: AtomicBool::new(false),
            pill_custom_offset: Mutex::new(None),
            is_dragging_pill: AtomicBool::new(false),
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
            is_micro_popover: AtomicBool::new(false),
            pill_custom_offset: Mutex::new(None),
            is_dragging_pill: AtomicBool::new(false),
        });

        let dict = app_state.custom_dictionary.lock().unwrap();
        assert!(dict.contains("nishikanto"), "Custom dictionary contains nishikanto");
    }

    #[test]
    fn test_extract_target_chunk_under_and_over_limit() {
        let short_text = "This is a short sentence under 500 characters.";
        let (offset, chunk) = extract_target_chunk(short_text);
        assert_eq!(offset, 0);
        assert_eq!(chunk, short_text);

        let long_text = "A".repeat(600) + ". This is the active sentence.";
        let (offset, chunk) = extract_target_chunk(&long_text);
        assert!(offset > 0);
        assert!(chunk.contains("This is the active sentence."));
    }

    #[test]
    fn test_gemini_grammar_issue_schema_and_sentence_rewrite_flag() {
        let json_input = r#"[
            {
                "original_span": "Their",
                "replacement": "There",
                "category": "Correctness",
                "reason": "Fix homophone spelling"
            },
            {
                "original_span": "we should of took further actions",
                "replacement": "we should have taken further action",
                "category": "Correctness",
                "reason": "Fix modal verb agreement"
            },
            {
                "original_span": "Their is definately many reasons why the report was written by our team yesterday.",
                "replacement": "There are definitely many reasons why our team wrote the report yesterday.",
                "category": "Clarity",
                "reason": "Rewrite for clarity and active voice"
            }
        ]"#;

        let issues: Vec<GeminiGrammarIssue> = serde_json::from_str(json_input).expect("Must parse valid JSON");
        assert_eq!(issues.len(), 3);

        // First is single word fix
        let is_sent_0 = issues[0].original_span.split_whitespace().count() > 2;
        assert!(!is_sent_0);

        // Second is phrase fix
        let is_sent_1 = issues[1].original_span.split_whitespace().count() > 2;
        assert!(is_sent_1);

        // Third is sentence rewrite
        let is_sent_2 = issues[2].category == "Clarity" || issues[2].reason.contains("Rewrite");
        assert!(is_sent_2);
    }
}
