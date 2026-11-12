use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedContext {
    pub text: String,
    pub bounds: ScreenRect,
    pub source_app: String,
    pub process_name: String,
    pub method: String,
}

pub struct WindowStateInfo {
    pub is_desktop_or_shell: bool,
    pub process_name: String,
    pub title: String,
    pub hwnd_val: isize,
}

pub fn is_blacklisted_process(process_name: &str) -> bool {
    let lower = process_name.trim().to_lowercase();
    let name = if let Some(stripped) = lower.strip_suffix(".exe") {
        stripped
    } else {
        &lower
    };

    matches!(
        name,
        "antigravity"
            | "code"
            | "cursor"
            | "devenv"
            | "windowsterminal"
            | "powershell"
            | "cmd"
            | "bash"
            | "wt"
            | "conhost"
            | "alacritty"
            | "wezterm"
            | "hyper"
            | "idea"
            | "idea64"
            | "pycharm"
            | "pycharm64"
            | "webstorm"
            | "webstorm64"
            | "clion"
            | "clion64"
            | "rider"
            | "rider64"
            | "studio64"
            | "sublime_text"
            | "notepad++"
            | "nishikanto"
    )
}

pub fn is_placeholder_text(text: &str, name: &str, help_text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }

    if !name.trim().is_empty() && t.eq_ignore_ascii_case(name.trim()) {
        return true;
    }
    if !help_text.trim().is_empty() && t.eq_ignore_ascii_case(help_text.trim()) {
        return true;
    }

    let lower = t.to_lowercase();
    lower.contains("@ to mention")
        || lower.contains("/ for actions")
        || lower.contains("ask anything")
        || lower.contains("type a message")
        || lower.contains("search or enter")
        || lower.contains("search web")
        || lower.contains("search google")
        || lower.starts_with("search...")
        || lower.starts_with("type here")
        || lower.starts_with("enter text")
        || lower.starts_with("placeholder")
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::time::Duration;
    use windows::core::BSTR;
    use windows::Win32::System::Com::*;
    use windows::Win32::System::Ole::*;
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Variant::*;

    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::Threading::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;

    static LAST_FOREGROUND_HWND: AtomicIsize = AtomicIsize::new(0);
    pub static IS_REPLACING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub static USER_HAS_TYPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub static JUST_SCROLLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub static CLICK_OCCURRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub static LAST_TYPED_TIMESTAMP_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    #[allow(dead_code)]
    pub fn get_foreground_hwnd() -> isize {
        unsafe { GetForegroundWindow() as isize }
    }

    pub fn is_typing_key(vk: u32) -> bool {
        matches!(
            vk,
            0x08          // VK_BACK
            | 0x0D        // VK_RETURN
            | 0x20        // VK_SPACE
            | 0x2E        // VK_DELETE
            | 0x30..=0x39 // 0..9
            | 0x41..=0x5A // A..Z
            | 0x60..=0x6F // Numpad
            | 0xBA..=0xC0 // ;: =+ ,- ./ `~
            | 0xDB..=0xDF // [{ \| ]} '"
            | 0xE2        // OEM 102
        )
    }

    unsafe extern "system" fn low_level_keyboard_proc(
        n_code: i32,
        w_param: usize,
        l_param: isize,
    ) -> isize {
        if n_code >= 0 && (w_param == WM_KEYDOWN as usize || w_param == WM_SYSKEYDOWN as usize) {
            let kbd = unsafe { *(l_param as *const KBDLLHOOKSTRUCT) };
            if is_typing_key(kbd.vkCode) {
                USER_HAS_TYPED.store(true, Ordering::SeqCst);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                LAST_TYPED_TIMESTAMP_MS.store(now, Ordering::SeqCst);
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) }
    }

    unsafe extern "system" fn low_level_mouse_proc(
        n_code: i32,
        w_param: usize,
        l_param: isize,
    ) -> isize {
        if n_code >= 0 {
            let msg = w_param as u32;
            if msg == WM_MOUSEWHEEL || msg == WM_MOUSEHWHEEL {
                USER_HAS_TYPED.store(false, Ordering::SeqCst);
                JUST_SCROLLED.store(true, Ordering::SeqCst);
            } else if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN {
                CLICK_OCCURRED.store(true, Ordering::SeqCst);
            }
        }
        unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) }
    }

    pub fn init_input_event_hooks() {
        std::thread::spawn(|| {
            unsafe {
                let k_hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(low_level_keyboard_proc),
                    std::ptr::null_mut(),
                    0,
                );
                let m_hook = SetWindowsHookExW(
                    WH_MOUSE_LL,
                    Some(low_level_mouse_proc),
                    std::ptr::null_mut(),
                    0,
                );

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                if !k_hook.is_null() {
                    UnhookWindowsHookEx(k_hook);
                }
                if !m_hook.is_null() {
                    UnhookWindowsHookEx(m_hook);
                }
            }
        });
    }

    pub fn get_foreground_window_state() -> WindowStateInfo {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return WindowStateInfo {
                    is_desktop_or_shell: false,
                    process_name: String::new(),
                    title: String::new(),
                    hwnd_val: 0,
                };
            }

            let mut class_buf = [0u16; 256];
            let len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), 256);
            let is_desktop_or_shell = if len > 0 {
                let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
                let lower = class_name.to_lowercase();
                lower == "progman"
                    || lower == "workerw"
                    || lower == "shell_traywnd"
                    || lower == "shell_secondarytraywnd"
            } else {
                false
            };

            let process_name = get_process_name_for_hwnd(hwnd);

            let mut title_buf = [0u16; 512];
            let title_len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
            let title = if title_len > 0 {
                String::from_utf16_lossy(&title_buf[..title_len as usize])
            } else {
                String::new()
            };

            WindowStateInfo {
                is_desktop_or_shell,
                process_name,
                title,
                hwnd_val: hwnd as isize,
            }
        }
    }

    pub fn get_process_name_for_hwnd(hwnd: HWND) -> String {
        unsafe {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return String::new();
            }

            let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process_handle.is_null() {
                return String::new();
            }

            let mut path_buf = [0u16; 1024];
            let mut size = 1024u32;
            let success = QueryFullProcessImageNameW(process_handle, 0, path_buf.as_mut_ptr(), &mut size);
            CloseHandle(process_handle);

            if success != 0 && size > 0 {
                let full_path = String::from_utf16_lossy(&path_buf[..size as usize]);
                if let Some(exe_name) = std::path::Path::new(&full_path).file_name() {
                    return exe_name.to_string_lossy().to_string();
                }
            }
            String::new()
        }
    }

    pub fn simulate_key_combination(ctrl_or_cmd: u16, key: u16) {
        unsafe {
            let mut inputs: [INPUT; 4] = std::mem::zeroed();

            inputs[0].r#type = INPUT_KEYBOARD;
            inputs[0].Anonymous.ki.wVk = ctrl_or_cmd;

            inputs[1].r#type = INPUT_KEYBOARD;
            inputs[1].Anonymous.ki.wVk = key;

            inputs[2].r#type = INPUT_KEYBOARD;
            inputs[2].Anonymous.ki.wVk = key;
            inputs[2].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

            inputs[3].r#type = INPUT_KEYBOARD;
            inputs[3].Anonymous.ki.wVk = ctrl_or_cmd;
            inputs[3].Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;

            SendInput(4, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }

    #[allow(dead_code)]
    pub fn get_foreground_app_info() -> (String, String, ScreenRect) {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return (
                    "Unknown Application".to_string(),
                    String::new(),
                    ScreenRect {
                        x: 100.0,
                        y: 100.0,
                        width: 400.0,
                        height: 300.0,
                    },
                );
            }

            LAST_FOREGROUND_HWND.store(hwnd as isize, Ordering::SeqCst);
            let process_name = get_process_name_for_hwnd(hwnd);

            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
            let title = if len > 0 {
                String::from_utf16_lossy(&title_buf[..len as usize])
            } else if !process_name.is_empty() {
                process_name.clone()
            } else {
                "Active Window".to_string()
            };

            let mut rect: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut rect);

            let screen_rect = ScreenRect {
                x: rect.left as f64,
                y: rect.top as f64,
                width: (rect.right - rect.left).max(100) as f64,
                height: (rect.bottom - rect.top).max(50) as f64,
            };

            (title, process_name, screen_rect)
        }
    }

    pub fn capture_via_accessibility() -> Option<CapturedContext> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()? };
        capture_via_accessibility_internal(&automation)
    }

    pub fn capture_via_accessibility_with_automation(automation: &IUIAutomation) -> Option<CapturedContext> {
        capture_via_accessibility_internal(automation)
    }

    fn capture_via_accessibility_internal(automation: &IUIAutomation) -> Option<CapturedContext> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            LAST_FOREGROUND_HWND.store(hwnd as isize, Ordering::SeqCst);
            let pname = get_process_name_for_hwnd(hwnd);
            if is_blacklisted_process(&pname) {
                return None;
            }

            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
            let app_name = if len > 0 {
                String::from_utf16_lossy(&title_buf[..len as usize])
            } else if !pname.is_empty() {
                pname.clone()
            } else {
                "Active Application".to_string()
            };
            let process_name = pname;

            let focused = automation.GetFocusedElement().ok()?;

            // 1. STRICT Editable-Only Filter:
            // Must have keyboard focusable == true
            let is_kb_focusable = focused.CurrentIsKeyboardFocusable().map(|b| b.as_bool()).unwrap_or(false);
            if !is_kb_focusable {
                return None;
            }

            // Check ControlType
            let control_type = focused.CurrentControlType().unwrap_or(UIA_CONTROLTYPE_ID(0));
            let is_edit_or_doc = control_type == UIA_EditControlTypeId || control_type == UIA_DocumentControlTypeId;

            // Check HTML input/textarea/contenteditable="true"
            let aria_role = focused.CurrentAriaRole().map(|b| b.to_string().to_lowercase()).unwrap_or_default();
            let class_name = focused.CurrentClassName().map(|b| b.to_string().to_lowercase()).unwrap_or_default();
            let item_type = focused.CurrentItemType().map(|b| b.to_string().to_lowercase()).unwrap_or_default();
            let item_status = focused.CurrentItemStatus().map(|b| b.to_string().to_lowercase()).unwrap_or_default();

            let is_html_editable = aria_role == "textbox"
                || aria_role == "searchbox"
                || aria_role == "combobox"
                || item_type.contains("input")
                || item_type.contains("textarea")
                || item_type.contains("contenteditable")
                || item_status.contains("contenteditable")
                || class_name.contains("edit")
                || class_name.contains("input")
                || class_name.contains("textarea");

            if !is_edit_or_doc && !is_html_editable {
                return None;
            }

            let val_pattern = focused.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).ok();
            let text_pattern = focused.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok();

            // Check Read-Only state:
            // Check IUIAutomationValuePattern::CurrentIsReadOnly == false or UIA_IsReadOnlyAttributeId == false
            let mut is_read_only = false;
            let mut checked_readonly = false;

            if let Some(ref vp) = val_pattern {
                if let Ok(ro) = vp.CurrentIsReadOnly() {
                    checked_readonly = true;
                    if ro.as_bool() {
                        is_read_only = true;
                    }
                }
            }

            if let Some(ref tp) = text_pattern {
                if let Ok(doc_range) = tp.DocumentRange() {
                    if let Ok(attr_val) = doc_range.GetAttributeValue(UIA_IsReadOnlyAttributeId) {
                        if let Ok(ro) = VariantToBoolean(&attr_val) {
                            checked_readonly = true;
                            if ro.as_bool() {
                                is_read_only = true;
                            }
                        }
                    }
                }
            }

            if !checked_readonly {
                if let Ok(val) = focused.GetCurrentPropertyValue(UIA_ValueIsReadOnlyPropertyId) {
                    if let Ok(ro) = VariantToBoolean(&val) {
                        if ro.as_bool() {
                            is_read_only = true;
                        }
                    }
                }
            }

            if is_read_only {
                return None;
            }

            let name = focused.CurrentName().map(|b| b.to_string()).unwrap_or_default();
            let help_text = focused.CurrentHelpText().map(|b| b.to_string()).unwrap_or_default();

            // Global virtual-screen physical coordinates
            let rect = focused.CurrentBoundingRectangle().ok()?;
            let bounds = ScreenRect {
                x: rect.left as f64,
                y: rect.top as f64,
                width: (rect.right - rect.left).max(10) as f64,
                height: (rect.bottom - rect.top).max(10) as f64,
            };

            // 1. Try TextPattern for selected text or full text
            if let Some(ref tp) = text_pattern {
                if let Ok(selections) = tp.GetSelection() {
                    if let Ok(count) = selections.Length() {
                        if count > 0 {
                            if let Ok(range) = selections.GetElement(0) {
                                if let Ok(bstr) = range.GetText(-1) {
                                    let s = bstr.to_string();
                                    if !is_placeholder_text(&s, &name, &help_text) {
                                        return Some(CapturedContext {
                                            text: s,
                                            bounds,
                                            source_app: app_name,
                                            process_name: process_name.clone(),
                                            method: "accessibility_selection_pattern".to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                // If no selection, get document range text
                if let Ok(doc_range) = tp.DocumentRange() {
                    if let Ok(bstr) = doc_range.GetText(-1) {
                        let s = bstr.to_string();
                        if !is_placeholder_text(&s, &name, &help_text) {
                            return Some(CapturedContext {
                                text: s,
                                bounds,
                                source_app: app_name,
                                process_name: process_name.clone(),
                                method: "accessibility_doc_pattern".to_string(),
                            });
                        }
                    }
                }
            }

            // 2. Try ValuePattern (standard text boxes, inputs)
            if let Some(ref vp) = val_pattern {
                if let Ok(bstr) = vp.CurrentValue() {
                    let s = bstr.to_string();
                    if !is_placeholder_text(&s, &name, &help_text) {
                        return Some(CapturedContext {
                            text: s,
                            bounds,
                            source_app: app_name,
                            process_name: process_name.clone(),
                            method: "accessibility_value_pattern".to_string(),
                        });
                    }
                }
            }

            None
        }
    }

    pub fn extract_lint_screen_rects(
        lints: &mut [crate::LocalLintResult],
        box_bounds: &ScreenRect,
        scale_factor: f64,
    ) {
        if lints.is_empty() {
            return;
        }

        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if let Ok(automation) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                extract_lint_screen_rects_with_automation(&automation, lints, box_bounds, scale_factor);
            } else {
                fallback_lint_screen_rects(lints, box_bounds, scale_factor);
            }
        }
    }

    pub fn extract_lint_screen_rects_with_automation(
        automation: &IUIAutomation,
        lints: &mut [crate::LocalLintResult],
        box_bounds: &ScreenRect,
        scale_factor: f64,
    ) {
        if lints.is_empty() {
            return;
        }

        let sf = if scale_factor > 0.0 { scale_factor } else { 1.0 };

        unsafe {
            if let Ok(focused) = automation.GetFocusedElement() {
                if let Ok(text_pattern) = focused.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                    if let Ok(doc_range) = text_pattern.DocumentRange() {
                        let full_text: Option<String> = doc_range.GetText(-1).ok().map(|b| b.to_string());

                        for lint in lints.iter_mut() {
                            let len = lint.end.saturating_sub(lint.start);
                            if len == 0 {
                                continue;
                            }

                            // Calculate line index and column index in full text
                            let (line_idx, col_idx) = if let Some(ref text) = full_text {
                                let safe_start = lint.start.min(text.len());
                                let text_before = &text[..safe_start];
                                let line_start = text_before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                                let col = safe_start.saturating_sub(line_start);
                                let line = text_before.matches('\n').count();
                                (line, col)
                            } else {
                                (0, 0)
                            };

                            let avg_char_w = 8.0 * sf;
                            let est_w = (len as f64 * avg_char_w).max(20.0 * sf);
                            let max_plausible_w = (len as f64 * 24.0 * sf).max(120.0 * sf);

                            if let Ok(word_range) = doc_range.Clone() {
                                let _ = word_range.Move(TextUnit_Character, lint.start as i32);
                                let _ = word_range.MoveEndpointByUnit(
                                    TextPatternRangeEndpoint_End,
                                    TextUnit_Character,
                                    len as i32,
                                );

                                let mut got_exact_bounds = false;
                                if let Ok(sa) = word_range.GetBoundingRectangles() {
                                    let lbound = SafeArrayGetLBound(sa, 1).unwrap_or(0);
                                    let ubound = SafeArrayGetUBound(sa, 1).unwrap_or(-1);
                                    let count = if ubound >= lbound {
                                        (ubound - lbound + 1) as usize
                                    } else {
                                        0
                                    };

                                    if count >= 4 {
                                        let mut data_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
                                        if SafeArrayAccessData(sa, &mut data_ptr).is_ok() {
                                            let slice = std::slice::from_raw_parts(data_ptr as *const f64, count);
                                            let phys_x = slice[0];
                                            let phys_y = slice[1];
                                            let phys_w = slice[2];
                                            let phys_h = slice[3];
                                            let _ = SafeArrayUnaccessData(sa);

                                            let visible = phys_y + phys_h >= box_bounds.y - 12.0
                                                && phys_y <= box_bounds.y + box_bounds.height + 12.0;

                                            if phys_h > 0.0 && visible {
                                                // CRITICAL NOTEPAD FIX: If phys_w is suspiciously wide (e.g. whole line or whole box),
                                                // do NOT highlight the whole line! Estimate horizontal position & width instead.
                                                if phys_w > max_plausible_w || phys_w >= box_bounds.width * 0.75 {
                                                    let line_start_x = phys_x;
                                                    let est_x = line_start_x + (col_idx as f64 * avg_char_w);
                                                    lint.screen_bounds = Some(ScreenRect {
                                                        x: est_x,
                                                        y: phys_y,
                                                        width: est_w,
                                                        height: phys_h,
                                                    });
                                                } else if phys_w > 0.0 {
                                                    lint.screen_bounds = Some(ScreenRect {
                                                        x: phys_x,
                                                        y: phys_y,
                                                        width: phys_w,
                                                        height: phys_h,
                                                    });
                                                } else {
                                                    lint.screen_bounds = Some(ScreenRect {
                                                        x: phys_x,
                                                        y: phys_y,
                                                        width: est_w,
                                                        height: phys_h,
                                                    });
                                                }
                                                got_exact_bounds = true;
                                            }
                                        }
                                    }
                                    let _ = SafeArrayDestroy(sa);
                                }

                                // If word_range.GetBoundingRectangles() returned empty array (typical in standard Win32 Edit / Notepad):
                                if !got_exact_bounds {
                                    // Query line bounding rectangles to get starting line X and Y
                                    let mut line_coords: Option<(f64, f64, f64)> = None;
                                    if let Ok(line_range) = word_range.Clone() {
                                        let _ = line_range.ExpandToEnclosingUnit(TextUnit_Line);
                                        if let Ok(lsa) = line_range.GetBoundingRectangles() {
                                            let lb = SafeArrayGetLBound(lsa, 1).unwrap_or(0);
                                            let ub = SafeArrayGetUBound(lsa, 1).unwrap_or(-1);
                                            if ub >= lb + 3 {
                                                let mut lptr: *mut std::ffi::c_void = std::ptr::null_mut();
                                                if SafeArrayAccessData(lsa, &mut lptr).is_ok() {
                                                    let lslice = std::slice::from_raw_parts(lptr as *const f64, (ub - lb + 1) as usize);
                                                    if lslice[2] > 0.0 && lslice[3] > 0.0 {
                                                        line_coords = Some((lslice[0], lslice[1], lslice[3]));
                                                    }
                                                    let _ = SafeArrayUnaccessData(lsa);
                                                }
                                            }
                                            let _ = SafeArrayDestroy(lsa);
                                        }
                                    }

                                    let default_line_h = 20.0 * sf;
                                    let fallback_line_x = box_bounds.x + 8.0 * sf;
                                    let fallback_line_y = box_bounds.y + 6.0 * sf + (line_idx as f64 * default_line_h);
                                    let (line_x, line_y, line_h) = line_coords.unwrap_or((fallback_line_x, fallback_line_y, default_line_h));

                                    let est_x = line_x + (col_idx as f64 * avg_char_w);
                                    let visible = line_y + line_h >= box_bounds.y - 12.0
                                        && line_y <= box_bounds.y + box_bounds.height + 12.0;

                                    if visible {
                                        lint.screen_bounds = Some(ScreenRect {
                                            x: est_x,
                                            y: line_y,
                                            width: est_w,
                                            height: line_h,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Fallback: If still any lint missing bounds, anchor to textbox (never 0,0)
            if lints.iter().any(|l| l.screen_bounds.is_none()) {
                fallback_lint_screen_rects(lints, box_bounds, sf);
            }
        }
    }

    pub fn fallback_lint_screen_rects(
        lints: &mut [crate::LocalLintResult],
        box_bounds: &ScreenRect,
        sf: f64,
    ) {
        let default_x = box_bounds.x + 16.0;
        let default_y = box_bounds.y + box_bounds.height + 8.0;
        for lint in lints.iter_mut() {
            if lint.screen_bounds.is_none() {
                let word_len = lint.end.saturating_sub(lint.start).max(1);
                let est_w = (word_len as f64 * 8.0 * sf).max(24.0 * sf);
                lint.screen_bounds = Some(ScreenRect {
                    x: default_x,
                    y: default_y,
                    width: est_w,
                    height: 20.0 * sf,
                });
            }
        }
    }

    pub fn capture_active_context() -> CapturedContext {
        // First try UI Automation
        if let Some(ctx) = capture_via_accessibility() {
            return ctx;
        }

        // If the focused control is NOT an editable element, do NOT simulate Ctrl+C or scan static text
        CapturedContext {
            text: String::new(),
            bounds: ScreenRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            source_app: String::new(),
            process_name: String::new(),
            method: "non_editable".to_string(),
        }
    }

    pub fn replace_active_text(new_text: &str) -> Result<(), String> {
        // Concurrency lock: never run twice concurrently
        if IS_REPLACING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Ok(());
        }

        let result = replace_active_text_impl(new_text);

        // Temporarily pause background text scanning for 400ms after a replacement
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(400));
            IS_REPLACING.store(false, Ordering::SeqCst);
        });

        result
    }

    fn replace_active_text_impl(new_text: &str) -> Result<(), String> {
        let saved_hwnd = LAST_FOREGROUND_HWND.load(Ordering::SeqCst);
        if saved_hwnd != 0 {
            unsafe {
                SetForegroundWindow(saved_hwnd as HWND);
                std::thread::sleep(Duration::from_millis(30));
            }
        }

        // Step A: Try IUIAutomationValuePattern::SetValue with complete updated text string first.
        // Immediately read back CurrentValue to verify if it changed. If it succeeded, return immediately!
        let mut step_a_succeeded = false;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if let Ok(automation) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                if let Ok(focused) = automation.GetFocusedElement() {
                    if let Ok(val_pattern) = focused.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                        let bstr = BSTR::from(new_text);
                        if val_pattern.SetValue(&bstr).is_ok() {
                            if let Ok(current_val) = val_pattern.CurrentValue() {
                                let val_str = current_val.to_string();
                                if val_str == new_text {
                                    step_a_succeeded = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        if step_a_succeeded {
            return Ok(());
        }

        // Step B: ONLY if ValuePattern is unsupported (such as in Chrome or Electron apps):
        // Focus the target window, select all text inside the active input (Ctrl+A),
        // paste updated full text via clipboard (Ctrl+V) once, and restore user's clipboard after 150ms.
        if saved_hwnd != 0 {
            unsafe {
                SetForegroundWindow(saved_hwnd as HWND);
                std::thread::sleep(Duration::from_millis(40));
            }
        }

        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let backup_text = clipboard.get_text().ok();

        // 1. Put replacement text on clipboard
        clipboard.set_text(new_text).map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(20));

        // 2. Select all text inside active input (Ctrl+A)
        simulate_key_combination(VK_CONTROL, b'A' as u16);
        std::thread::sleep(Duration::from_millis(30));

        // 3. Paste updated full text via clipboard (Ctrl+V) once
        simulate_key_combination(VK_CONTROL, b'V' as u16);

        // 4. Restore the user's clipboard after 150ms
        std::thread::sleep(Duration::from_millis(150));
        if let Some(orig) = backup_text {
            let _ = clipboard.set_text(orig);
        }

        Ok(())
    }

    pub fn find_closest_occurrence(
        text: &str,
        span: &str,
        expected_char_idx: usize,
    ) -> Option<(usize, usize)> {
        let clean_span = span.trim();
        if clean_span.is_empty() {
            return None;
        }

        let text_chars: Vec<char> = text.chars().collect();
        let span_chars: Vec<char> = clean_span.chars().collect();

        if span_chars.len() > text_chars.len() {
            return None;
        }

        let mut best_match: Option<(usize, usize, usize)> = None;

        for i in 0..=(text_chars.len() - span_chars.len()) {
            if text_chars[i..i + span_chars.len()] == span_chars[..] {
                let distance = (i as isize - expected_char_idx as isize).abs() as usize;
                match best_match {
                    None => {
                        best_match = Some((i, i + span_chars.len(), distance));
                    }
                    Some((_, _, best_dist)) => {
                        if distance < best_dist {
                            best_match = Some((i, i + span_chars.len(), distance));
                        }
                    }
                }
            }
        }

        if best_match.is_none() {
            let span_lower: Vec<char> = clean_span.to_lowercase().chars().collect();
            for i in 0..=(text_chars.len() - span_chars.len()) {
                let window: Vec<char> = text_chars[i..i + span_chars.len()]
                    .iter()
                    .flat_map(|c| c.to_lowercase())
                    .collect();
                if window == span_lower {
                    let distance = (i as isize - expected_char_idx as isize).abs() as usize;
                    match best_match {
                        None => {
                            best_match = Some((i, i + span_chars.len(), distance));
                        }
                        Some((_, _, best_dist)) => {
                            if distance < best_dist {
                                best_match = Some((i, i + span_chars.len(), distance));
                            }
                        }
                    }
                }
            }
        }

        best_match.map(|(s, e, _)| (s, e))
    }

    pub fn get_active_input_text() -> Option<String> {
        capture_via_accessibility().map(|c| c.text).filter(|t| !t.is_empty())
    }

    pub fn replace_suggestion(
        original_span: &str,
        replacement: &str,
        expected_char_idx: usize,
        full_text_fallback: &str,
    ) -> Result<String, String> {
        let raw_text = get_active_input_text().unwrap_or_else(|| full_text_fallback.to_string());
        let had_crlf = raw_text.contains("\r\n");

        // Normalize \r\n to \n
        let norm_text = raw_text.replace("\r\n", "\n");
        let norm_span = original_span.replace("\r\n", "\n");
        let norm_rep = replacement.replace("\r\n", "\n");

        let new_full_text = if let Some((start_idx, end_idx)) =
            find_closest_occurrence(&norm_text, &norm_span, expected_char_idx)
        {
            let chars: Vec<char> = norm_text.chars().collect();
            let before: String = chars[..start_idx].iter().collect();
            let after: String = chars[end_idx..].iter().collect();
            format!("{}{}{}", before, norm_rep, after)
        } else if norm_span.trim() == norm_text.trim() || norm_span.trim().is_empty() {
            norm_rep.clone()
        } else {
            let chars: Vec<char> = norm_text.chars().collect();
            let span_len = norm_span.chars().count();
            let start_idx = expected_char_idx.min(chars.len());
            let end_idx = (start_idx + span_len).min(chars.len());
            let before: String = chars[..start_idx].iter().collect();
            let after: String = chars[end_idx..].iter().collect();
            format!("{}{}{}", before, norm_rep, after)
        };

        let text_to_write = if had_crlf {
            new_full_text.replace("\r\n", "\n").replace("\n", "\r\n")
        } else {
            new_full_text.clone()
        };

        replace_active_text(&text_to_write)?;

        Ok(new_full_text)
    }

    pub fn replace_single_word(
        start: usize,
        end: usize,
        replacement: &str,
        full_text_fallback: &str,
    ) -> Result<(), String> {
        let chars: Vec<char> = full_text_fallback.chars().collect();
        let span = if start < chars.len() && end <= chars.len() && start < end {
            chars[start..end].iter().collect::<String>()
        } else {
            String::new()
        };
        let _ = replace_suggestion(&span, replacement, start, full_text_fallback)?;
        Ok(())
    }

    pub fn get_cursor_position() -> Option<(f64, f64)> {
        unsafe {
            let mut pt = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
            if GetCursorPos(&mut pt) != 0 {
                Some((pt.x as f64, pt.y as f64))
            } else {
                None
            }
        }
    }

    pub fn get_caret_position_physical() -> Option<(f64, f64)> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let tid = GetWindowThreadProcessId(hwnd, std::ptr::null_mut());
            if tid == 0 {
                return None;
            }

            let mut gui: GUITHREADINFO = std::mem::zeroed();
            gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            if GetGUIThreadInfo(tid, &mut gui) == 0 {
                return None;
            }
            let target = if gui.hwndCaret.is_null() { hwnd } else { gui.hwndCaret };
            let mut pt = POINT {
                x: gui.rcCaret.left,
                y: gui.rcCaret.top,
            };
            ClientToScreen(target, &mut pt);
            Some((pt.x as f64, pt.y as f64))
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    use std::time::Duration;

    pub static IS_REPLACING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub static USER_HAS_TYPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub static JUST_SCROLLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub static CLICK_OCCURRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub static LAST_TYPED_TIMESTAMP_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    pub fn init_input_event_hooks() {}

    pub fn find_closest_occurrence(
        text: &str,
        span: &str,
        expected_char_idx: usize,
    ) -> Option<(usize, usize)> {
        let clean_span = span.trim();
        if clean_span.is_empty() {
            return None;
        }

        let chars: Vec<char> = text.chars().collect();
        let span_chars: Vec<char> = clean_span.chars().collect();
        let span_len = span_chars.len();
        if span_len == 0 || span_len > chars.len() {
            return None;
        }

        let mut matches = Vec::new();
        for i in 0..=(chars.len() - span_len) {
            if chars[i..i + span_len] == span_chars[..] {
                matches.push((i, i + span_len));
            }
        }

        if matches.is_empty() {
            return None;
        }

        matches.sort_by_key(|&(start, _)| {
            (start as isize - expected_char_idx as isize).abs()
        });

        Some(matches[0])
    }

    pub fn get_active_input_text() -> Option<String> {
        None
    }

    pub fn replace_suggestion(
        _original_span: &str,
        replacement: &str,
        _expected_char_idx: usize,
        full_text_fallback: &str,
    ) -> Result<String, String> {
        replace_active_text(replacement)?;
        Ok(replacement.to_string())
    }

    pub fn get_foreground_window_state() -> WindowStateInfo {
        WindowStateInfo {
            is_desktop_or_shell: false,
            process_name: String::new(),
            title: String::new(),
            hwnd_val: 0,
        }
    }

    pub fn capture_via_accessibility() -> Option<CapturedContext> {
        None
    }

    pub fn capture_active_context() -> CapturedContext {
        let mut clipboard = arboard::Clipboard::new().ok();
        let backup_text = clipboard.as_mut().and_then(|c| c.get_text().ok());

        // macOS: Simulate Cmd+C via osascript
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("osascript")
                .args(["-e", "tell application \"System Events\" to keystroke \"c\" using {command down}"])
                .output();
        }

        std::thread::sleep(Duration::from_millis(60));

        let copied_text = clipboard
            .as_mut()
            .and_then(|c| c.get_text().ok())
            .unwrap_or_default();

        if let Some(orig) = backup_text {
            let _ = clipboard.as_mut().map(|c| c.set_text(orig));
        }

        CapturedContext {
            text: copied_text,
            bounds: ScreenRect {
                x: 200.0,
                y: 200.0,
                width: 340.0,
                height: 120.0,
            },
            source_app: "Active Application".to_string(),
            process_name: "App".to_string(),
            method: "clipboard_fallback".to_string(),
        }
    }

    pub fn extract_lint_screen_rects(
        _lints: &mut [crate::LocalLintResult],
        _box_bounds: &ScreenRect,
        _scale_factor: f64,
    ) {
    }

    pub fn replace_active_text(new_text: &str) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let backup_text = clipboard.get_text().ok();

        clipboard.set_text(new_text).map_err(|e| e.to_string())?;

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("osascript")
                .args(["-e", "tell application \"System Events\" to keystroke \"v\" using {command down}"])
                .output();
        }

        std::thread::sleep(Duration::from_millis(70));

        if let Some(orig) = backup_text {
            let _ = clipboard.set_text(orig);
        }

        Ok(())
    }

    pub fn get_cursor_position() -> Option<(f64, f64)> {
        None
    }

    pub fn get_caret_position_physical() -> Option<(f64, f64)> {
        None
    }

    pub fn replace_single_word(
        _start: usize,
        _end: usize,
        _replacement: &str,
        full_text_fallback: &str,
    ) -> Result<(), String> {
        replace_active_text(full_text_fallback)
    }

    pub fn capture_via_accessibility_with_automation<T>(_auto: &T) -> Option<CapturedContext> {
        None
    }

    pub fn extract_lint_screen_rects_with_automation<T>(
        _auto: &T,
        _lints: &mut [crate::LocalLintResult],
        _box_bounds: &ScreenRect,
        _scale_factor: f64,
    ) {
    }

    pub fn fallback_lint_screen_rects(
        lints: &mut [crate::LocalLintResult],
        box_bounds: &ScreenRect,
        sf: f64,
    ) {
        let default_x = box_bounds.x + 16.0;
        let default_y = box_bounds.y + box_bounds.height + 8.0;
        for lint in lints.iter_mut() {
            if lint.screen_bounds.is_none() {
                let word_len = lint.end.saturating_sub(lint.start).max(1);
                let est_w = (word_len as f64 * 8.0 * sf).max(24.0 * sf);
                lint.screen_bounds = Some(ScreenRect {
                    x: default_x,
                    y: default_y,
                    width: est_w,
                    height: 20.0 * sf,
                });
            }
        }
    }
}

pub use platform::{
    capture_active_context,
    capture_via_accessibility,
    capture_via_accessibility_with_automation,
    extract_lint_screen_rects,
    extract_lint_screen_rects_with_automation,
    fallback_lint_screen_rects,
    find_closest_occurrence,
    get_active_input_text,
    get_caret_position_physical,
    get_cursor_position,
    get_foreground_window_state,
    init_input_event_hooks,
    replace_active_text,
    replace_single_word,
    replace_suggestion,
    CLICK_OCCURRED,
    IS_REPLACING,
    JUST_SCROLLED,
    LAST_TYPED_TIMESTAMP_MS,
    USER_HAS_TYPED,
};

pub fn get_active_process_name() -> String {
    #[cfg(windows)]
    {
        unsafe {
            let hwnd = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
            if !hwnd.is_null() {
                platform::get_process_name_for_hwnd(hwnd)
            } else {
                String::new()
            }
        }
    }
    #[cfg(not(windows))]
    {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_fallback_lint_screen_rects_anchors_to_textbox_and_never_zero() {
        let mut lints = vec![crate::LocalLintResult {
            id: "test-lint-1".to_string(),
            start: 0,
            end: 5,
            flagged_text: "hello".to_string(),
            category: "Correctness".to_string(),
            subcategory: "Spelling".to_string(),
            message: "Spelling error".to_string(),
            suggestions: vec!["world".to_string()],
            screen_bounds: None,
            is_sentence_rewrite: false,
            reason: None,
        }];

        // Secondary monitor at positive X: x = 2560, y = 300, width = 400, height = 50
        let box_bounds = ScreenRect {
            x: 2560.0,
            y: 300.0,
            width: 400.0,
            height: 50.0,
        };

        fallback_lint_screen_rects(&mut lints, &box_bounds, 1.25);

        let sb = lints[0].screen_bounds.as_ref().unwrap();
        assert_eq!(sb.x, 2560.0 + 16.0);
        assert_eq!(sb.y, 300.0 + 50.0 + 8.0);
        assert_ne!(sb.x, 0.0);
        assert_ne!(sb.y, 0.0);
        // Verify width is estimated per word and not whole line or whole textbox
        assert_eq!(sb.width, 50.0);
    }

    #[test]
    fn test_fallback_lint_screen_rects_preserves_negative_virtual_screen_coords() {
        let mut lints = vec![crate::LocalLintResult {
            id: "test-lint-neg".to_string(),
            start: 0,
            end: 4,
            flagged_text: "test".to_string(),
            category: "Correctness".to_string(),
            subcategory: "Spelling".to_string(),
            message: "Spelling error".to_string(),
            suggestions: vec!["best".to_string()],
            screen_bounds: None,
            is_sentence_rewrite: false,
            reason: None,
        }];

        // Secondary monitor at negative X: x = -1920 + 200 = -1720, y = 150, width = 350, height = 45
        let box_bounds = ScreenRect {
            x: -1720.0,
            y: 150.0,
            width: 350.0,
            height: 45.0,
        };

        fallback_lint_screen_rects(&mut lints, &box_bounds, 1.0);

        let sb = lints[0].screen_bounds.as_ref().unwrap();
        assert_eq!(sb.x, -1720.0 + 16.0);
        assert_eq!(sb.y, 150.0 + 45.0 + 8.0);
        assert!(sb.x < 0.0, "Must preserve negative coordinate for secondary monitor");
        assert_eq!(sb.width, 32.0);
    }

    #[test]
    fn test_non_editable_context_skips_activation() {
        let non_editable = CapturedContext {
            text: String::new(),
            bounds: ScreenRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            source_app: String::new(),
            process_name: String::new(),
            method: "non_editable".to_string(),
        };

        assert_eq!(non_editable.method, "non_editable");
        assert!(non_editable.text.is_empty());
    }

    #[test]
    fn test_notepad_estimated_bounds_never_exceeds_word_width() {
        let mut lints = vec![crate::LocalLintResult {
            id: "test-lint-notepad".to_string(),
            start: 12,
            end: 18, // "simple" (6 chars)
            flagged_text: "simple".to_string(),
            category: "Correctness".to_string(),
            subcategory: "Spelling".to_string(),
            message: "Spelling error".to_string(),
            suggestions: vec!["sample".to_string()],
            screen_bounds: None,
            is_sentence_rewrite: false,
            reason: None,
        }];

        // Full Notepad window width is 1200px
        let box_bounds = ScreenRect {
            x: 100.0,
            y: 100.0,
            width: 1200.0,
            height: 700.0,
        };

        fallback_lint_screen_rects(&mut lints, &box_bounds, 1.0);

        let sb = lints[0].screen_bounds.as_ref().unwrap();
        // Width must be estimated based on word length, NEVER whole line (1200px)
        assert!(sb.width < 100.0, "Word bound width ({}) must be proportional to word length, not whole line width", sb.width);
        assert_eq!(sb.width, 48.0);
    }

    #[test]
    fn test_find_closest_occurrence_exact_and_duplicate() {
        let text = "the quick brown fox jumps over the lazy dog";
        // Target second "the" at index 31
        let res = platform::find_closest_occurrence(text, "the", 30);
        assert_eq!(res, Some((31, 34)));

        // Target first "the" at index 0
        let res1 = platform::find_closest_occurrence(text, "the", 2);
        assert_eq!(res1, Some((0, 3)));
    }

    #[test]
    fn test_find_closest_occurrence_crlf_normalization() {
        let raw = "Line one\r\nLine two with error\r\nLine three";
        let norm = raw.replace("\r\n", "\n");
        let res = platform::find_closest_occurrence(&norm, "with error", 15);
        assert!(res.is_some());
        let (s, e) = res.unwrap();
        assert_eq!(&norm[s..e], "with error");
    }

    #[test]
    fn test_is_replacing_lock_rejects_concurrent_execution() {
        assert!(!IS_REPLACING.load(Ordering::SeqCst));
        // Acquire lock
        let acquired = IS_REPLACING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok();
        assert!(acquired);

        // Second concurrent acquisition must fail
        let second = IS_REPLACING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok();
        assert!(!second, "Concurrent replacement lock acquisition must be rejected");

        // Release lock
        IS_REPLACING.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_is_blacklisted_process() {
        assert!(is_blacklisted_process("code.exe"));
        assert!(is_blacklisted_process("Cursor.exe"));
        assert!(is_blacklisted_process("antigravity.exe"));
        assert!(is_blacklisted_process("windowsterminal.exe"));
        assert!(is_blacklisted_process("powershell.exe"));
        assert!(is_blacklisted_process("cmd.exe"));
        assert!(is_blacklisted_process("devenv.exe"));
        assert!(is_blacklisted_process("wt.exe"));
        assert!(is_blacklisted_process("idea64.exe"));
        assert!(is_blacklisted_process("notepad++.exe"));
        assert!(!is_blacklisted_process("notepad.exe"));
        assert!(!is_blacklisted_process("chrome.exe"));
        assert!(!is_blacklisted_process("slack.exe"));
    }

    #[test]
    fn test_is_placeholder_text() {
        assert!(is_placeholder_text("", "", ""));
        assert!(is_placeholder_text("   ", "", ""));
        assert!(is_placeholder_text("Search", "Search", ""));
        assert!(is_placeholder_text("Type a message...", "", "Type a message..."));
        assert!(is_placeholder_text("Ask anything, @ to mention, / for actions", "", ""));
        assert!(is_placeholder_text("Search Google or enter a URL", "", ""));
        assert!(!is_placeholder_text("This is an actual user typed sentence.", "Editor", "Type here"));
    }
}
