#![cfg(target_os = "windows")]
#![windows_subsystem = "windows"]

use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::Ime::{
    ImmGetContext, ImmGetConversionStatus, ImmReleaseContext, ImmSetConversionStatus,
    IME_CMODE_NATIVE, IME_CONVERSION_MODE, IME_SENTENCE_MODE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CAPITAL, VK_F13,
    VK_RMENU, VIRTUAL_KEY,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

// ── 상수 ──────────────────────────────────────────────────────────────────

const VK_HANGUL: u16 = 0x15;
const WM_TRAYICON: u32 = WM_USER + 1;
const WM_DEBUG_LOG: u32 = WM_USER + 2;
const TRAY_ICON_ID: u32 = 1;

const IDM_TOGGLE: u32 = 1001;
const IDM_KEY_CAPSLOCK: u32 = 1010;
const IDM_KEY_F13: u32 = 1011;
const IDM_KEY_RALT: u32 = 1012;
const IDM_KEY_LEARN: u32 = 1013;
const IDM_DEBUG: u32 = 1020;
const IDM_EXIT: u32 = 1099;

const IDC_DEBUG_EDIT: i32 = 2001;

// Edit control 메시지 (Win32 상수)
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SCROLLCARET: u32 = 0x00B7;

// ── 전역 상태 ─────────────────────────────────────────────────────────────

static ENABLED: AtomicBool = AtomicBool::new(true);
static SENDING: AtomicBool = AtomicBool::new(false);
static TRIGGER_KEY: AtomicU32 = AtomicU32::new(VK_CAPITAL.0 as u32);
static HOOK_HANDLE: AtomicU32 = AtomicU32::new(0);
static MAIN_HWND: AtomicU32 = AtomicU32::new(0);

/// 키 학습 모드
static LEARNING: AtomicBool = AtomicBool::new(false);

/// 디버그 윈도우 핸들
static DEBUG_HWND: AtomicU32 = AtomicU32::new(0);
/// 디버그 에디트 컨트롤 핸들
static DEBUG_EDIT_HWND: AtomicU32 = AtomicU32::new(0);
/// 디버그 윈도우 표시 여부
static DEBUG_VISIBLE: AtomicBool = AtomicBool::new(false);

/// 디버그 로그 버퍼 (훅 콜백 → 메인 스레드 전달용)
static LOG_BUFFER: Mutex<Vec<String>> = Mutex::new(Vec::new());

// ── 디버그 로깅 ───────────────────────────────────────────────────────────

fn debug_log(msg: &str) {
    if let Ok(mut buf) = LOG_BUFFER.lock() {
        buf.push(msg.to_string());
    }
    // 메인 윈도우에 로그 플러시 요청
    let hwnd_val = MAIN_HWND.load(Ordering::SeqCst);
    if hwnd_val != 0 {
        unsafe {
            let hwnd = HWND(hwnd_val as isize as *mut _);
            let _ = PostMessageW(hwnd, WM_DEBUG_LOG, WPARAM(0), LPARAM(0));
        }
    }
}

/// 디버그 에디트 컨트롤에 로그 플러시
fn flush_debug_log() {
    let edit_val = DEBUG_EDIT_HWND.load(Ordering::SeqCst);
    if edit_val == 0 {
        return;
    }

    let messages: Vec<String> = {
        if let Ok(mut buf) = LOG_BUFFER.lock() {
            buf.drain(..).collect()
        } else {
            return;
        }
    };

    if messages.is_empty() {
        return;
    }

    unsafe {
        let edit_hwnd = HWND(edit_val as isize as *mut _);
        for msg in &messages {
            let line = format!("{}\r\n", msg);
            let wide: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
            // 텍스트 끝으로 이동
            let len = GetWindowTextLengthW(edit_hwnd);
            SendMessageW(edit_hwnd, EM_SETSEL, WPARAM(len as usize), LPARAM(len as isize));
            // 텍스트 추가
            SendMessageW(
                edit_hwnd,
                EM_REPLACESEL,
                WPARAM(0),
                LPARAM(wide.as_ptr() as isize),
            );
        }
        // 스크롤을 맨 아래로
        SendMessageW(
            edit_hwnd,
            EM_SCROLLCARET,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

// ── 키보드 훅 ─────────────────────────────────────────────────────────────

unsafe extern "system" fn keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if SENDING.load(Ordering::SeqCst) {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    if n_code >= 0 {
        let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
        let msg = w_param.0 as u32;

        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            // 키 학습 모드: 다음 키를 트리거로 설정
            if LEARNING.load(Ordering::SeqCst) {
                LEARNING.store(false, Ordering::SeqCst);
                TRIGGER_KEY.store(kb.vkCode, Ordering::Relaxed);
                debug_log(&format!(
                    "[LEARN] captured vk=0x{:02X} scan=0x{:04X} → set as trigger",
                    kb.vkCode, kb.scanCode
                ));
                // 메인 윈도우에 트레이 업데이트 요청
                let hwnd_val = MAIN_HWND.load(Ordering::SeqCst);
                if hwnd_val != 0 {
                    let hwnd = HWND(hwnd_val as isize as *mut _);
                    let _ = PostMessageW(hwnd, WM_COMMAND,
                        WPARAM(IDM_KEY_LEARN as usize), LPARAM(kb.vkCode as isize));
                }
                return LRESULT(1); // 이 키 이벤트는 소비
            }

            let enabled = ENABLED.load(Ordering::SeqCst);
            let trigger = TRIGGER_KEY.load(Ordering::Relaxed);

            debug_log(&format!(
                "[KEY] vk=0x{:02X} scan=0x{:04X} flags=0x{:08X} | trigger=0x{:02X} enabled={} match={}",
                kb.vkCode, kb.scanCode, kb.flags.0, trigger, enabled, kb.vkCode == trigger
            ));

            if enabled && kb.vkCode == trigger {
                debug_log("[ACTION] trigger matched → send_hangul_toggle()");
                send_hangul_toggle();
                return LRESULT(1);
            }
        } else if msg == WM_KEYUP || msg == WM_SYSKEYUP {
            // 학습 모드 중이면 키업도 소비
            if LEARNING.load(Ordering::SeqCst) {
                return LRESULT(1);
            }
            // 키 업도 차단해야 원래 키 동작 방지
            let enabled = ENABLED.load(Ordering::SeqCst);
            let trigger = TRIGGER_KEY.load(Ordering::Relaxed);
            if enabled && kb.vkCode == trigger {
                return LRESULT(1);
            }
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

/// 한글 IME 토글 - IMM API 직접 제어 + SendInput 폴백
fn send_hangul_toggle() {
    SENDING.store(true, Ordering::SeqCst);

    unsafe {
        let fg_hwnd = GetForegroundWindow();
        debug_log(&format!("[IMM] GetForegroundWindow → HWND={:?}", fg_hwnd.0));

        if fg_hwnd.0 as usize != 0 {
            let himc = ImmGetContext(fg_hwnd);
            debug_log(&format!("[IMM] ImmGetContext → HIMC={:?}", himc.0));

            if himc.0 as usize != 0 {
                let mut conversion = IME_CONVERSION_MODE::default();
                let mut sentence = IME_SENTENCE_MODE::default();
                let ok = ImmGetConversionStatus(himc, Some(&mut conversion), Some(&mut sentence));
                debug_log(&format!(
                    "[IMM] ImmGetConversionStatus → ok={} conversion=0x{:08X} sentence=0x{:08X}",
                    ok.as_bool(), conversion.0, sentence.0
                ));

                if ok.as_bool() {
                    let new_conversion = IME_CONVERSION_MODE(conversion.0 ^ IME_CMODE_NATIVE.0);
                    let set_ok = ImmSetConversionStatus(himc, new_conversion, sentence);
                    debug_log(&format!(
                        "[IMM] ImmSetConversionStatus → ok={} new_conversion=0x{:08X}",
                        set_ok.as_bool(), new_conversion.0
                    ));
                }
                let _ = ImmReleaseContext(fg_hwnd, himc);
                SENDING.store(false, Ordering::SeqCst);
                return;
            }
        }

        // 폴백: VK_HANGUL SendInput
        debug_log("[FALLBACK] IMM failed → SendInput VK_HANGUL");
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(VK_HANGUL),
                        wScan: 0,
                        dwFlags: Default::default(),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(VK_HANGUL),
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }

    SENDING.store(false, Ordering::SeqCst);
}

// ── 트레이 아이콘 관리 ────────────────────────────────────────────────────

fn trigger_key_name_static(vk: u16) -> Option<&'static str> {
    if vk == VK_CAPITAL.0 {
        Some("Caps Lock")
    } else if vk == VK_F13.0 {
        Some("F13")
    } else if vk == VK_RMENU.0 {
        Some("Right Alt")
    } else if vk == 0xA4 {
        Some("Left Alt")
    } else {
        None
    }
}

fn trigger_key_display(vk: u16) -> String {
    match trigger_key_name_static(vk) {
        Some(name) => name.to_string(),
        None => format!("0x{:02X}", vk),
    }
}

fn make_tooltip() -> [u16; 128] {
    let enabled = ENABLED.load(Ordering::SeqCst);
    let trigger = TRIGGER_KEY.load(Ordering::Relaxed) as u16;
    let status = if enabled { "ON" } else { "OFF" };
    let key_name = trigger_key_display(trigger);

    let text = format!("synergy-hangul-fix [{}] - {}", status, key_name);
    let mut tip: [u16; 128] = [0; 128];
    for (i, c) in text.encode_utf16().take(127).enumerate() {
        tip[i] = c;
    }
    tip
}

fn get_status_icon() -> HICON {
    let enabled = ENABLED.load(Ordering::SeqCst);
    unsafe {
        if enabled {
            LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
        } else {
            LoadIconW(None, IDI_WARNING).unwrap_or_default()
        }
    }
}

fn add_tray_icon(hwnd: HWND) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ICON_ID;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = get_status_icon();
        nid.szTip = make_tooltip();
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

fn update_tray_icon(hwnd: HWND) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ICON_ID;
        nid.uFlags = NIF_ICON | NIF_TIP;
        nid.hIcon = get_status_icon();
        nid.szTip = make_tooltip();
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn remove_tray_icon(hwnd: HWND) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ICON_ID;
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

// ── 디버그 윈도우 ─────────────────────────────────────────────────────────

fn create_debug_window(hinstance: HINSTANCE) {
    unsafe {
        let class_name = wide_string("synergy_hangul_fix_debug");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(debug_wndproc),
            hInstance: hinstance,
            lpszClassName: wptr(&class_name),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let title = wide_string("synergy-hangul-fix [DEBUG]");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            wptr(&class_name),
            wptr(&title),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT, CW_USEDEFAULT,
            600, 400,
            None,
            None,
            Some(&hinstance),
            None,
        )
        .unwrap();

        DEBUG_HWND.store(hwnd.0 as usize as u32, Ordering::SeqCst);

        // 멀티라인 에디트 컨트롤 생성 (읽기 전용)
        let edit_class = wide_string("EDIT");
        let edit_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0x200), // WS_EX_CLIENTEDGE
            wptr(&edit_class),
            PCWSTR::null(),
            WINDOW_STYLE(
                WS_CHILD.0
                    | WS_VISIBLE.0
                    | WS_VSCROLL.0
                    | WS_HSCROLL.0
                    | ES_MULTILINE as u32
                    | ES_READONLY as u32
                    | ES_AUTOVSCROLL as u32
                    | ES_AUTOHSCROLL as u32,
            ),
            0, 0, 600, 400,
            hwnd,
            HMENU(IDC_DEBUG_EDIT as isize as *mut _),
            Some(&hinstance),
            None,
        )
        .unwrap();

        DEBUG_EDIT_HWND.store(edit_hwnd.0 as usize as u32, Ordering::SeqCst);
    }
}

fn toggle_debug_window() {
    let hwnd_val = DEBUG_HWND.load(Ordering::SeqCst);
    if hwnd_val == 0 {
        return;
    }
    unsafe {
        let hwnd = HWND(hwnd_val as isize as *mut _);
        let visible = DEBUG_VISIBLE.load(Ordering::SeqCst);
        if visible {
            let _ = ShowWindow(hwnd, SW_HIDE);
            DEBUG_VISIBLE.store(false, Ordering::SeqCst);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            DEBUG_VISIBLE.store(true, Ordering::SeqCst);
        }
    }
}

unsafe extern "system" fn debug_wndproc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_SIZE => {
            // 에디트 컨트롤을 윈도우 크기에 맞춤
            let width = (l_param.0 & 0xFFFF) as i32;
            let height = ((l_param.0 >> 16) & 0xFFFF) as i32;
            let edit_val = DEBUG_EDIT_HWND.load(Ordering::SeqCst);
            if edit_val != 0 {
                let edit_hwnd = HWND(edit_val as isize as *mut _);
                let _ = MoveWindow(edit_hwnd, 0, 0, width, height, true);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // X 버튼 클릭 시 숨기기 (파괴하지 않음)
            let _ = ShowWindow(hwnd, SW_HIDE);
            DEBUG_VISIBLE.store(false, Ordering::SeqCst);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}

// ── 컨텍스트 메뉴 ────────────────────────────────────────────────────────

fn show_context_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu().unwrap();
        let enabled = ENABLED.load(Ordering::SeqCst);
        let trigger = TRIGGER_KEY.load(Ordering::Relaxed) as u16;

        // 활성/비활성 토글
        let toggle_text = if enabled {
            wide_string("비활성화(&D)")
        } else {
            wide_string("활성화(&E)")
        };
        AppendMenuW(menu, MF_STRING, IDM_TOGGLE as usize, wptr(&toggle_text)).ok();

        // 구분선
        AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()).ok();

        // 트리거 키 서브메뉴
        let submenu = CreatePopupMenu().unwrap();

        let caps_flags =
            MF_STRING | if trigger == VK_CAPITAL.0 { MF_CHECKED } else { MF_UNCHECKED };
        let f13_flags = MF_STRING | if trigger == VK_F13.0 { MF_CHECKED } else { MF_UNCHECKED };
        let ralt_flags =
            MF_STRING | if trigger == VK_RMENU.0 { MF_CHECKED } else { MF_UNCHECKED };

        let caps_text = wide_string("Caps Lock");
        let f13_text = wide_string("F13");
        let ralt_text = wide_string("Right Alt");

        AppendMenuW(submenu, caps_flags, IDM_KEY_CAPSLOCK as usize, wptr(&caps_text)).ok();
        AppendMenuW(submenu, f13_flags, IDM_KEY_F13 as usize, wptr(&f13_text)).ok();
        AppendMenuW(submenu, ralt_flags, IDM_KEY_RALT as usize, wptr(&ralt_text)).ok();

        // 현재 커스텀 키가 프리셋에 없으면 표시
        let is_preset = trigger == VK_CAPITAL.0
            || trigger == VK_F13.0
            || trigger == VK_RMENU.0;
        if !is_preset {
            let current_text = wide_string(&format!("현재: {} (0x{:02X})", trigger_key_display(trigger), trigger));
            AppendMenuW(submenu, MF_STRING | MF_CHECKED, 0, wptr(&current_text)).ok();
        }

        // 구분선 + 키 감지
        AppendMenuW(submenu, MF_SEPARATOR, 0, PCWSTR::null()).ok();
        let learn_label = if LEARNING.load(Ordering::SeqCst) {
            wide_string("키를 누르세요...")
        } else {
            wide_string("키 감지(&L)...")
        };
        AppendMenuW(submenu, MF_STRING, IDM_KEY_LEARN as usize, wptr(&learn_label)).ok();

        let key_menu_text = wide_string("트리거 키(&K)");
        AppendMenuW(menu, MF_STRING | MF_POPUP, submenu.0 as usize, wptr(&key_menu_text)).ok();

        // 구분선
        AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()).ok();

        // 디버그
        let debug_text = if DEBUG_VISIBLE.load(Ordering::SeqCst) {
            wide_string("디버그 닫기(&B)")
        } else {
            wide_string("디버그(&B)")
        };
        AppendMenuW(menu, MF_STRING, IDM_DEBUG as usize, wptr(&debug_text)).ok();

        // 구분선
        AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()).ok();

        // 종료
        let exit_text = wide_string("종료(&X)");
        AppendMenuW(menu, MF_STRING, IDM_EXIT as usize, wptr(&exit_text)).ok();

        // 메뉴 표시
        let mut pt = POINT::default();
        GetCursorPos(&mut pt).ok();
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTALIGN | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
        PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0)).ok();
        let _ = DestroyMenu(menu);
    }
}

// ── 윈도우 프로시저 ───────────────────────────────────────────────────────

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAYICON => {
            let event = (l_param.0 & 0xFFFF) as u32;
            match event {
                WM_LBUTTONUP => {
                    ENABLED.fetch_xor(true, Ordering::SeqCst);
                    update_tray_icon(hwnd);
                    let state = if ENABLED.load(Ordering::SeqCst) {
                        "ON"
                    } else {
                        "OFF"
                    };
                    debug_log(&format!("[STATE] enabled toggled → {}", state));
                }
                WM_RBUTTONUP => {
                    show_context_menu(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }

        WM_DEBUG_LOG => {
            flush_debug_log();
            LRESULT(0)
        }

        WM_COMMAND => {
            let cmd = (w_param.0 & 0xFFFF) as u32;
            match cmd {
                IDM_TOGGLE => {
                    ENABLED.fetch_xor(true, Ordering::SeqCst);
                    update_tray_icon(hwnd);
                    let state = if ENABLED.load(Ordering::SeqCst) {
                        "ON"
                    } else {
                        "OFF"
                    };
                    debug_log(&format!("[STATE] enabled toggled → {}", state));
                }
                IDM_KEY_CAPSLOCK => {
                    TRIGGER_KEY.store(VK_CAPITAL.0 as u32, Ordering::Relaxed);
                    update_tray_icon(hwnd);
                    debug_log("[CONFIG] trigger key → Caps Lock (0x14)");
                }
                IDM_KEY_F13 => {
                    TRIGGER_KEY.store(VK_F13.0 as u32, Ordering::Relaxed);
                    update_tray_icon(hwnd);
                    debug_log("[CONFIG] trigger key → F13 (0x7C)");
                }
                IDM_KEY_RALT => {
                    TRIGGER_KEY.store(VK_RMENU.0 as u32, Ordering::Relaxed);
                    update_tray_icon(hwnd);
                    debug_log("[CONFIG] trigger key → Right Alt (0xA5)");
                }
                IDM_KEY_LEARN => {
                    // lParam에 vkCode가 있으면 훅에서 캡처된 것 (학습 완료)
                    if l_param.0 != 0 {
                        // 훅 콜백에서 PostMessage로 전달된 경우
                        update_tray_icon(hwnd);
                        debug_log(&format!(
                            "[CONFIG] trigger key → {} (0x{:02X}) via learn",
                            trigger_key_display(TRIGGER_KEY.load(Ordering::Relaxed) as u16),
                            TRIGGER_KEY.load(Ordering::Relaxed)
                        ));
                    } else {
                        // 메뉴에서 클릭: 학습 모드 진입
                        LEARNING.store(true, Ordering::SeqCst);
                        debug_log("[LEARN] waiting for key press...");
                    }
                }
                IDM_DEBUG => {
                    toggle_debug_window();
                }
                IDM_EXIT => {
                    DestroyWindow(hwnd).ok();
                }
                _ => {}
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            remove_tray_icon(hwnd);

            let raw = HOOK_HANDLE.load(Ordering::SeqCst);
            if raw != 0 {
                let hook = HHOOK(raw as isize as *mut _);
                let _ = UnhookWindowsHookEx(hook);
            }

            // 디버그 윈도우도 파괴
            let dbg = DEBUG_HWND.load(Ordering::SeqCst);
            if dbg != 0 {
                let _ = DestroyWindow(HWND(dbg as isize as *mut _));
            }

            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}

// ── 유틸리티 ──────────────────────────────────────────────────────────────

fn wide_string(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wptr(s: &[u16]) -> PCWSTR {
    PCWSTR(s.as_ptr())
}

// ── 메인 ──────────────────────────────────────────────────────────────────

fn main() {
    unsafe {
        let hmodule = GetModuleHandleW(None).unwrap();
        let hinstance: HINSTANCE = hmodule.into();

        // 메인 히든 윈도우 등록
        let class_name = wide_string("synergy_hangul_fix_wnd");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: wptr(&class_name),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            wptr(&class_name),
            wptr(&wide_string("synergy-hangul-fix")),
            WS_OVERLAPPED,
            0, 0, 0, 0,
            HWND_MESSAGE,
            None,
            Some(&hinstance),
            None,
        )
        .unwrap();

        MAIN_HWND.store(hwnd.0 as usize as u32, Ordering::SeqCst);

        // 디버그 윈도우 생성 (숨김 상태)
        create_debug_window(hinstance);

        // 키보드 훅 설치
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0);
        match hook {
            Ok(hook) => {
                HOOK_HANDLE.store(hook.0 as usize as u32, Ordering::SeqCst);
                debug_log("[INIT] keyboard hook installed OK");
            }
            Err(e) => {
                MessageBoxW(
                    None,
                    wptr(&wide_string(
                        "키보드 훅 설치에 실패했습니다.\n관리자 권한으로 실행해 주세요.",
                    )),
                    wptr(&wide_string("synergy-hangul-fix 오류")),
                    MB_ICONERROR | MB_OK,
                );
                debug_log(&format!("[INIT] keyboard hook FAILED: {:?}", e));
                return;
            }
        }

        // 트레이 아이콘 추가
        add_tray_icon(hwnd);
        debug_log(&format!(
            "[INIT] started | trigger=0x{:02X} ({}) | enabled=true",
            TRIGGER_KEY.load(Ordering::Relaxed),
            trigger_key_display(TRIGGER_KEY.load(Ordering::Relaxed) as u16)
        ));

        // 메시지 루프
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
