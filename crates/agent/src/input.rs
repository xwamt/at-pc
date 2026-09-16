//! Native remote desktop input event simulation module.
//! Supports mouse movement, clicks, scrolling, key presses, and Unicode text typing.
//! Zero heavy dependencies: uses native OS APIs (Win32 SendInput / macOS CoreGraphics).

use at_pc_protocol::models::DesktopInputEvent;

#[cfg(windows)]
pub fn inject_input_event(event: DesktopInputEvent) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SetCursorPos, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    unsafe {
        match event {
            DesktopInputEvent::MouseMovePixel { x, y } => {
                let px = x as i32;
                let py = y as i32;

                let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

                // 1. Direct hardware cursor positioning via SetCursorPos
                SetCursorPos(px, py);

                // 2. Dispatch standard SendInput for WM_MOUSEMOVE window message generation
                let norm_x = if vw > 0 {
                    (((px - vx) as i64 * 65535) / vw as i64).clamp(0, 65535) as i32
                } else {
                    px
                };
                let norm_y = if vh > 0 {
                    (((py - vy) as i64 * 65535) / vh as i64).clamp(0, 65535) as i32
                } else {
                    py
                };

                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dx: norm_x,
                            dy: norm_y,
                            mouseData: 0,
                            dwFlags: MOUSEEVENTF_ABSOLUTE
                                | MOUSEEVENTF_MOVE
                                | MOUSEEVENTF_VIRTUALDESK,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                let ret = SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
                if ret == 0 {
                    let err = windows_sys::Win32::Foundation::GetLastError();
                    tracing::debug!(
                        "SendInput MouseMovePixel failed ({}); falling back to mouse_event",
                        err
                    );
                    mouse_event(
                        MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE | MOUSEEVENTF_VIRTUALDESK,
                        norm_x,
                        norm_y,
                        0,
                        0,
                    );
                }
            }
            DesktopInputEvent::MouseMove { x, y } => {
                let norm_x = (x.min(65535)) as i64;
                let norm_y = (y.min(65535)) as i64;

                let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
                if vw > 0 && vh > 0 {
                    let px = vx + ((norm_x * vw as i64) / 65535) as i32;
                    let py = vy + ((norm_y * vh as i64) / 65535) as i32;
                    SetCursorPos(px, py);
                }

                // 2. Dispatch standard SendInput for WM_MOUSEMOVE window message generation
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dx: norm_x as i32,
                            dy: norm_y as i32,
                            mouseData: 0,
                            dwFlags: MOUSEEVENTF_ABSOLUTE
                                | MOUSEEVENTF_MOVE
                                | MOUSEEVENTF_VIRTUALDESK,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                let ret = SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
                if ret == 0 {
                    let err = windows_sys::Win32::Foundation::GetLastError();
                    tracing::debug!(
                        "SendInput MouseMove failed ({}); falling back to mouse_event",
                        err
                    );
                    mouse_event(
                        MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE | MOUSEEVENTF_VIRTUALDESK,
                        norm_x as i32,
                        norm_y as i32,
                        0,
                        0,
                    );
                }
            }
            DesktopInputEvent::MouseDown { button } => {
                let flags = match button {
                    0 => MOUSEEVENTF_LEFTDOWN,
                    1 => MOUSEEVENTF_MIDDLEDOWN,
                    2 => MOUSEEVENTF_RIGHTDOWN,
                    _ => MOUSEEVENTF_LEFTDOWN,
                };
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dx: 0,
                            dy: 0,
                            mouseData: 0,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                let ret = SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
                if ret == 0 {
                    let err = windows_sys::Win32::Foundation::GetLastError();
                    tracing::debug!(
                        "SendInput MouseDown failed ({}); falling back to mouse_event",
                        err
                    );
                    mouse_event(flags, 0, 0, 0, 0);
                }
            }
            DesktopInputEvent::MouseUp { button } => {
                let flags = match button {
                    0 => MOUSEEVENTF_LEFTUP,
                    1 => MOUSEEVENTF_MIDDLEUP,
                    2 => MOUSEEVENTF_RIGHTUP,
                    _ => MOUSEEVENTF_LEFTUP,
                };
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dx: 0,
                            dy: 0,
                            mouseData: 0,
                            dwFlags: flags,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                let ret = SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
                if ret == 0 {
                    let err = windows_sys::Win32::Foundation::GetLastError();
                    tracing::debug!(
                        "SendInput MouseUp failed ({}); falling back to mouse_event",
                        err
                    );
                    mouse_event(flags, 0, 0, 0, 0);
                }
            }
            DesktopInputEvent::MouseClick { button, count } => {
                for _ in 0..count.max(1) {
                    inject_input_event(DesktopInputEvent::MouseDown { button })?;
                    std::thread::sleep(std::time::Duration::from_millis(15));
                    inject_input_event(DesktopInputEvent::MouseUp { button })?;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
            DesktopInputEvent::MouseWheel { delta_y } => {
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dx: 0,
                            dy: 0,
                            mouseData: delta_y as u32,
                            dwFlags: MOUSEEVENTF_WHEEL,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
            }
            DesktopInputEvent::KeyDown { key_code, .. } => {
                let input = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: key_code as u16,
                            wScan: 0,
                            dwFlags: 0,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
            }
            DesktopInputEvent::KeyUp { key_code, .. } => {
                let input = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: key_code as u16,
                            wScan: 0,
                            dwFlags: KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                };
                SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
            }
            DesktopInputEvent::TypeText { text } => {
                for c in text.encode_utf16() {
                    let down = INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: 0,
                                wScan: c,
                                dwFlags: KEYEVENTF_UNICODE,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    };
                    let up = INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: 0,
                                wScan: c,
                                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    };
                    SendInput(1, &down, std::mem::size_of::<INPUT>() as i32);
                    SendInput(1, &up, std::mem::size_of::<INPUT>() as i32);
                }
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
extern "C" {
    fn CGEventCreateScrollWheelEvent(
        source: *const std::ffi::c_void,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
    ) -> *mut std::ffi::c_void;
    fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
    fn CFRelease(cf: *const std::ffi::c_void);
}

#[cfg(target_os = "macos")]
static LAST_POINT: std::sync::Mutex<core_graphics::geometry::CGPoint> =
    std::sync::Mutex::new(core_graphics::geometry::CGPoint { x: 0.0, y: 0.0 });
#[cfg(target_os = "macos")]
static DOWN_BUTTON: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

#[cfg(target_os = "macos")]
pub fn inject_input_event(event: DesktopInputEvent) -> Result<(), String> {
    use core_graphics::display::CGDisplay;
    use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use core_graphics::geometry::CGPoint;
    use std::sync::atomic::Ordering;

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| "Failed to create CGEventSource".to_string())?;

    match event {
        DesktopInputEvent::MouseMovePixel { x, y } => {
            let point = CGPoint::new(x as f64, y as f64);

            if let Ok(mut lock) = LAST_POINT.lock() {
                *lock = point;
            }

            let btn = DOWN_BUTTON.load(Ordering::SeqCst);
            let (event_type, mouse_btn) = match btn {
                0 => (CGEventType::LeftMouseDragged, CGMouseButton::Left),
                2 => (CGEventType::RightMouseDragged, CGMouseButton::Right),
                1 | 3.. => (CGEventType::OtherMouseDragged, CGMouseButton::Center),
                _ => (CGEventType::MouseMoved, CGMouseButton::Left),
            };

            if let Ok(cg_ev) = CGEvent::new_mouse_event(source, event_type, point, mouse_btn) {
                cg_ev.post(CGEventTapLocation::HID);
            }
        }
        DesktopInputEvent::MouseMove { x, y } => {
            let (origin_x, origin_y, total_w, total_h) =
                if let Ok(displays) = CGDisplay::active_displays() {
                    if !displays.is_empty() {
                        let mut min_x = f64::MAX;
                        let mut min_y = f64::MAX;
                        let mut max_x = f64::MIN;
                        let mut max_y = f64::MIN;
                        for d_id in displays {
                            let b = CGDisplay::new(d_id).bounds();
                            min_x = min_x.min(b.origin.x);
                            min_y = min_y.min(b.origin.y);
                            max_x = max_x.max(b.origin.x + b.size.width);
                            max_y = max_y.max(b.origin.y + b.size.height);
                        }
                        (
                            min_x,
                            min_y,
                            (max_x - min_x).max(1.0),
                            (max_y - min_y).max(1.0),
                        )
                    } else {
                        let bounds = CGDisplay::main().bounds();
                        (
                            bounds.origin.x,
                            bounds.origin.y,
                            bounds.size.width.max(1.0),
                            bounds.size.height.max(1.0),
                        )
                    }
                } else {
                    let bounds = CGDisplay::main().bounds();
                    (
                        bounds.origin.x,
                        bounds.origin.y,
                        bounds.size.width.max(1.0),
                        bounds.size.height.max(1.0),
                    )
                };

            let norm_x = (x.min(65535)) as f64;
            let norm_y = (y.min(65535)) as f64;
            let px = origin_x + (norm_x * total_w) / 65535.0;
            let py = origin_y + (norm_y * total_h) / 65535.0;
            let point = CGPoint::new(px, py);

            if let Ok(mut lock) = LAST_POINT.lock() {
                *lock = point;
            }

            let btn = DOWN_BUTTON.load(Ordering::SeqCst);
            let (event_type, mouse_btn) = match btn {
                0 => (CGEventType::LeftMouseDragged, CGMouseButton::Left),
                2 => (CGEventType::RightMouseDragged, CGMouseButton::Right),
                1 | 3.. => (CGEventType::OtherMouseDragged, CGMouseButton::Center),
                _ => (CGEventType::MouseMoved, CGMouseButton::Left),
            };

            if let Ok(cg_ev) = CGEvent::new_mouse_event(source, event_type, point, mouse_btn) {
                cg_ev.post(CGEventTapLocation::HID);
            }
        }
        DesktopInputEvent::MouseDown { button } => {
            DOWN_BUTTON.store(button as i8, Ordering::SeqCst);

            let point = if let Ok(lock) = LAST_POINT.lock() {
                let p = *lock;
                if p.x == 0.0 && p.y == 0.0 {
                    if let Ok(cur_ev) = CGEvent::new(source.clone()) {
                        cur_ev.location()
                    } else {
                        p
                    }
                } else {
                    p
                }
            } else {
                CGPoint::new(0.0, 0.0)
            };

            let (event_type, mouse_btn) = match button {
                0 => (CGEventType::LeftMouseDown, CGMouseButton::Left),
                2 => (CGEventType::RightMouseDown, CGMouseButton::Right),
                _ => (CGEventType::OtherMouseDown, CGMouseButton::Center),
            };

            if let Ok(cg_ev) = CGEvent::new_mouse_event(source, event_type, point, mouse_btn) {
                cg_ev.post(CGEventTapLocation::HID);
            }
        }
        DesktopInputEvent::MouseUp { button } => {
            DOWN_BUTTON.store(-1, Ordering::SeqCst);

            let point = if let Ok(lock) = LAST_POINT.lock() {
                *lock
            } else {
                CGPoint::new(0.0, 0.0)
            };

            let (event_type, mouse_btn) = match button {
                0 => (CGEventType::LeftMouseUp, CGMouseButton::Left),
                2 => (CGEventType::RightMouseUp, CGMouseButton::Right),
                _ => (CGEventType::OtherMouseUp, CGMouseButton::Center),
            };

            if let Ok(cg_ev) = CGEvent::new_mouse_event(source, event_type, point, mouse_btn) {
                cg_ev.post(CGEventTapLocation::HID);
            }
        }
        DesktopInputEvent::MouseClick { button, count } => {
            for _ in 0..count.max(1) {
                inject_input_event(DesktopInputEvent::MouseDown { button })?;
                std::thread::sleep(std::time::Duration::from_millis(15));
                inject_input_event(DesktopInputEvent::MouseUp { button })?;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        DesktopInputEvent::MouseWheel { delta_y } => unsafe {
            let ev = CGEventCreateScrollWheelEvent(std::ptr::null(), 1, 1, delta_y);
            if !ev.is_null() {
                CGEventPost(0, ev);
                CFRelease(ev);
            }
        },
        DesktopInputEvent::KeyDown { key_code, .. } => {
            if let Ok(cg_ev) = CGEvent::new_keyboard_event(source, key_code as u16, true) {
                cg_ev.post(CGEventTapLocation::HID);
            }
        }
        DesktopInputEvent::KeyUp { key_code, .. } => {
            if let Ok(cg_ev) = CGEvent::new_keyboard_event(source, key_code as u16, false) {
                cg_ev.post(CGEventTapLocation::HID);
            }
        }
        DesktopInputEvent::TypeText { text } => {
            for ch in text.chars() {
                let mut utf16_buf = [0u16; 2];
                let encoded = ch.encode_utf16(&mut utf16_buf);
                if let Ok(cg_ev) = CGEvent::new_keyboard_event(source.clone(), 0, true) {
                    cg_ev.set_string_from_utf16_unchecked(encoded);
                    cg_ev.post(CGEventTapLocation::HID);
                }
                if let Ok(cg_ev) = CGEvent::new_keyboard_event(source.clone(), 0, false) {
                    cg_ev.set_string_from_utf16_unchecked(encoded);
                    cg_ev.post(CGEventTapLocation::HID);
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn inject_input_event(_event: DesktopInputEvent) -> Result<(), String> {
    tracing::warn!("Remote input simulation is not supported on this platform");
    Ok(())
}

/// Translates a key string identifier into an optional platform virtual key code.
/// Returns `None` if the key name is unrecognized or unsupported on this platform.
pub fn try_key_name_to_code(name: &str) -> Option<u32> {
    let lower = name.trim().to_lowercase();
    #[cfg(windows)]
    {
        let code = match lower.as_str() {
            "enter" | "return" => 0x0D,
            "tab" => 0x09,
            "backspace" | "back" => 0x08,
            "esc" | "escape" => 0x1B,
            "space" => 0x20,
            "delete" | "del" => 0x2E,
            "insert" | "ins" => 0x2D,
            "up" => 0x26,
            "down" => 0x28,
            "left" => 0x25,
            "right" => 0x27,
            "home" => 0x24,
            "end" => 0x23,
            "pageup" | "pgup" => 0x21,
            "pagedown" | "pgdn" => 0x22,
            "shift" => 0x10,
            "ctrl" | "control" => 0x11,
            "alt" | "opt" | "option" | "menu" => 0x12,
            "win" | "cmd" | "command" | "super" | "meta" => 0x5B,
            "capslock" | "caps" => 0x14,
            "numlock" => 0x90,
            "scrolllock" => 0x91,
            "printscreen" | "prtscn" => 0x2C,
            "pause" => 0x13,
            "f1" => 0x70,
            "f2" => 0x71,
            "f3" => 0x72,
            "f4" => 0x73,
            "f5" => 0x74,
            "f6" => 0x75,
            "f7" => 0x76,
            "f8" => 0x77,
            "f9" => 0x78,
            "f10" => 0x79,
            "f11" => 0x7A,
            "f12" => 0x7B,
            "." => 0xBE,        // VK_OEM_PERIOD
            "," => 0xBC,        // VK_OEM_COMMA
            "-" | "_" => 0xBD,  // VK_OEM_MINUS
            "=" | "+" => 0xBB,  // VK_OEM_PLUS
            "/" | "?" => 0xBF,  // VK_OEM_2
            ";" | ":" => 0xBA,  // VK_OEM_1
            "'" | "\"" => 0xDE, // VK_OEM_7
            "[" | "{" => 0xDB,  // VK_OEM_4
            "]" | "}" => 0xDD,  // VK_OEM_6
            "\\" | "|" => 0xDC, // VK_OEM_5
            "`" | "~" => 0xC0,  // VK_OEM_3
            s if s.len() == 1 => {
                let ch = s.chars().next().unwrap();
                if ch.is_ascii_digit() {
                    0x30 + (ch as u32 - '0' as u32)
                } else if ch.is_ascii_alphabetic() {
                    0x41 + (ch.to_ascii_uppercase() as u32 - 'A' as u32)
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        Some(code)
    }

    #[cfg(target_os = "macos")]
    {
        let code = match lower.as_str() {
            "return" | "enter" => 36,
            "tab" => 48,
            "space" => 49,
            "backspace" | "back" => 51,
            "delete" | "del" => 117,
            "insert" | "ins" => 114,
            "esc" | "escape" => 53,
            "cmd" | "command" | "win" | "meta" | "super" => 55,
            "shift" => 56,
            "capslock" | "caps" => 57,
            "alt" | "opt" | "option" => 58,
            "ctrl" | "control" => 59,
            "rightshift" => 60,
            "rightoption" | "rightalt" => 61,
            "rightcontrol" | "rightctrl" => 62,
            "left" => 123,
            "right" => 124,
            "down" => 125,
            "up" => 126,
            "home" => 115,
            "end" => 119,
            "pageup" | "pgup" => 116,
            "pagedown" | "pgdn" => 121,
            "f1" => 122,
            "f2" => 120,
            "f3" => 99,
            "f4" => 118,
            "f5" => 96,
            "f6" => 97,
            "f7" => 98,
            "f8" => 100,
            "f9" => 101,
            "f10" => 109,
            "f11" => 103,
            "f12" => 111,
            "a" => 0,
            "b" => 11,
            "c" => 8,
            "d" => 2,
            "e" => 14,
            "f" => 3,
            "g" => 5,
            "h" => 4,
            "i" => 34,
            "j" => 38,
            "k" => 40,
            "l" => 37,
            "m" => 46,
            "n" => 45,
            "o" => 31,
            "p" => 35,
            "q" => 12,
            "r" => 15,
            "s" => 1,
            "t" => 17,
            "u" => 32,
            "v" => 9,
            "w" => 13,
            "x" => 7,
            "y" => 16,
            "z" => 6,
            "0" => 29,
            "1" => 18,
            "2" => 19,
            "3" => 20,
            "4" => 21,
            "5" => 23,
            "6" => 22,
            "7" => 26,
            "8" => 28,
            "9" => 25,
            "." => 47,
            "," => 43,
            "-" | "_" => 27,
            "=" | "+" => 24,
            "/" | "?" => 44,
            ";" | ":" => 41,
            "'" | "\"" => 39,
            "[" | "{" => 33,
            "]" | "}" => 30,
            "\\" | "|" => 42,
            "`" | "~" => 50,
            _ => return None,
        };
        Some(code)
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        match lower.as_str() {
            "enter" | "return" => Some(10),
            "tab" => Some(9),
            "space" => Some(32),
            "backspace" | "back" => Some(8),
            "esc" | "escape" => Some(27),
            s if s.len() == 1 => Some(s.chars().next().unwrap() as u32),
            _ => None,
        }
    }
}

/// Translates a key string identifier into the platform virtual key code, or 0 if unknown.
pub fn key_name_to_code(name: &str) -> u32 {
    try_key_name_to_code(name).unwrap_or(0)
}
