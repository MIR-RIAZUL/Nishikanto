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

#[cfg(windows)]
mod platform {
    use super::*;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::time::Duration;
    use windows::core::BSTR;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Accessibility::*;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::Threading::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    static LAST_FOREGROUND_HWND: AtomicIsize = AtomicIsize::new(0);

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
        unsafe {
            let hwnd = GetForegroundWindow();
            if !hwnd.is_null() {
                LAST_FOREGROUND_HWND.store(hwnd as isize, Ordering::SeqCst);
            }
            let process_name = if !hwnd.is_null() {
                get_process_name_for_hwnd(hwnd)
            } else {
                String::new()
            };

            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let focused = automation.GetFocusedElement().ok()?;

            let rect = focused.CurrentBoundingRectangle().ok()?;
            let bounds = ScreenRect {
                x: rect.left as f64,
                y: rect.top as f64,
                width: (rect.right - rect.left).max(20) as f64,
                height: (rect.bottom - rect.top).max(20) as f64,
            };

            let app_name = focused.CurrentName().map(|b| b.to_string()).unwrap_or_else(|_| {
                if !process_name.is_empty() {
                    process_name.clone()
                } else {
                    "Focused Application".to_string()
                }
            });

            // 1. Try TextPattern for selected text or full text
            if let Ok(text_pattern) = focused.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                if let Ok(selections) = text_pattern.GetSelection() {
                    if let Ok(count) = selections.Length() {
                        if count > 0 {
                            if let Ok(range) = selections.GetElement(0) {
                                if let Ok(bstr) = range.GetText(-1) {
                                    let s = bstr.to_string();
                                    if !s.trim().is_empty() {
                                        return Some(CapturedContext {
                                            text: s,
                                            bounds,
                                            source_app: app_name,
                                            process_name: process_name.clone(),
                                            method: "accessibility_text_pattern".to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                // If no selection, get document range text
                if let Ok(doc_range) = text_pattern.DocumentRange() {
                    if let Ok(bstr) = doc_range.GetText(-1) {
                        let s = bstr.to_string();
                        if !s.trim().is_empty() {
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
            if let Ok(val_pattern) = focused.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                if let Ok(bstr) = val_pattern.CurrentValue() {
                    let s = bstr.to_string();
                    if !s.trim().is_empty() {
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

            // If bounds were found but text wasn't exposed by UIA (e.g. Chrome / Electron)
            None
        }
    }

    pub fn capture_active_context() -> CapturedContext {
        // First try UI Automation
        if let Some(ctx) = capture_via_accessibility() {
            return ctx;
        }

        let (source_app, process_name, window_bounds) = get_foreground_app_info();

        // Safe clipboard backup & restore fallback
        let mut clipboard = arboard::Clipboard::new().ok();
        let backup_text = clipboard.as_mut().and_then(|c| c.get_text().ok());

        // Simulate Ctrl+C
        simulate_key_combination(VK_CONTROL, b'C' as u16);
        std::thread::sleep(Duration::from_millis(50));

        let mut copied_text = clipboard
            .as_mut()
            .and_then(|c| c.get_text().ok())
            .unwrap_or_default();

        // Restore original clipboard content
        if let Some(orig) = backup_text {
            let _ = clipboard.as_mut().map(|c| c.set_text(orig));
        }

        if copied_text.trim().is_empty() {
            copied_text = String::new();
        }

        CapturedContext {
            text: copied_text,
            bounds: window_bounds,
            source_app,
            process_name,
            method: "clipboard_fallback".to_string(),
        }
    }

    pub fn replace_active_text(new_text: &str) -> Result<(), String> {
        let saved_hwnd = LAST_FOREGROUND_HWND.load(Ordering::SeqCst);
        if saved_hwnd != 0 {
            unsafe {
                SetForegroundWindow(saved_hwnd as HWND);
                std::thread::sleep(Duration::from_millis(30));
            }
        }

        // Try ValuePattern SetValue first
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if let Ok(automation) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                if let Ok(focused) = automation.GetFocusedElement() {
                    if let Ok(val_pattern) = focused.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                        let bstr = BSTR::from(new_text);
                        if val_pattern.SetValue(&bstr).is_ok() {
                            return Ok(());
                        }
                    }
                }
            }
        }

        // Safe clipboard paste fallback:
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let backup_text = clipboard.get_text().ok();

        // 1. Put replacement text on clipboard
        clipboard.set_text(new_text).map_err(|e| e.to_string())?;

        // 2. Simulate Ctrl+V paste
        simulate_key_combination(VK_CONTROL, b'V' as u16);

        // 3. Wait for target app message queue
        std::thread::sleep(Duration::from_millis(60));

        // 4. Restore original clipboard
        if let Some(orig) = backup_text {
            let _ = clipboard.set_text(orig);
        }

        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    use std::time::Duration;

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
}

pub use platform::{
    capture_active_context,
    capture_via_accessibility,
    get_foreground_window_state,
    replace_active_text,
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
