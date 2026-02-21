#![cfg(target_os = "windows")]
#![windows_subsystem = "windows"]

use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

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

/// VK_HANGUL = 0x15
const VK_HANGUL: u16 = 0x15;

/// 트레이 아이콘 콜백 메시지
const WM_TRAYICON: u32 = WM_USER + 1;

/// 트레이 아이콘 ID
const TRAY_ICON_ID: u32 = 1;

/// 메뉴 명령 ID
const IDM_TOGGLE: u32 = 1001;
const IDM_KEY_CAPSLOCK: u32 = 1010;
const IDM_KEY_F13: u32 = 1011;
const IDM_KEY_RALT: u32 = 1012;
const IDM_EXIT: u32 = 1099;

// ── 전역 상태 ─────────────────────────────────────────────────────────────

/// 훅 활성/비활성 플래그
static ENABLED: AtomicBool = AtomicBool::new(true);

/// 재진입 방지 플래그
static SENDING: AtomicBool = AtomicBool::new(false);

/// 트리거 키 (기본: Caps Lock)
static TRIGGER_KEY: AtomicU32 = AtomicU32::new(VK_CAPITAL.0 as u32);

/// 훅 핸들
static HOOK_HANDLE: AtomicU32 = AtomicU32::new(0);

/// 메인 히든 윈도우 핸들
static MAIN_HWND: AtomicU32 = AtomicU32::new(0);

// ── 키보드 훅 ─────────────────────────────────────────────────────────────

unsafe extern "system" fn keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    // 비활성 상태면 패스스루
    if !ENABLED.load(Ordering::SeqCst) {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    // 재진입 방지
    if SENDING.load(Ordering::SeqCst) {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    if n_code >= 0 {
        let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
        let trigger = TRIGGER_KEY.load(Ordering::Relaxed);

        if kb.vkCode == trigger {
            let msg = w_param.0 as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                send_hangul_toggle();
            }
            return LRESULT(1);
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

/// 한글 IME 토글 - IMM API 직접 제어 + SendInput 폴백
fn send_hangul_toggle() {
    SENDING.store(true, Ordering::SeqCst);

    unsafe {
        // 1차: IMM API로 직접 전환 시도
        let fg_hwnd = GetForegroundWindow();
        if fg_hwnd.0 as usize != 0 {
            let himc = ImmGetContext(fg_hwnd);
            if himc.0 as usize != 0 {
                let mut conversion = IME_CONVERSION_MODE::default();
                let mut sentence = IME_SENTENCE_MODE::default();
                if ImmGetConversionStatus(himc, Some(&mut conversion), Some(&mut sentence)).as_bool() {
                    // IME_CMODE_NATIVE (=1) 비트 토글 → 한글↔영문 전환
                    let new_conversion = IME_CONVERSION_MODE(conversion.0 ^ IME_CMODE_NATIVE.0);
                    let _ = ImmSetConversionStatus(himc, new_conversion, sentence);
                }
                let _ = ImmReleaseContext(fg_hwnd, himc);
                SENDING.store(false, Ordering::SeqCst);
                return;
            }
        }

        // 2차 폴백: VK_HANGUL SendInput
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

fn trigger_key_name(vk: u16) -> &'static str {
    if vk == VK_CAPITAL.0 {
        "Caps Lock"
    } else if vk == VK_F13.0 {
        "F13"
    } else if vk == VK_RMENU.0 {
        "Right Alt"
    } else {
        "Unknown"
    }
}

fn make_tooltip() -> [u16; 128] {
    let enabled = ENABLED.load(Ordering::SeqCst);
    let trigger = TRIGGER_KEY.load(Ordering::Relaxed) as u16;
    let status = if enabled { "ON" } else { "OFF" };
    let key_name = trigger_key_name(trigger);

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

        let caps_flags = MF_STRING | if trigger == VK_CAPITAL.0 { MF_CHECKED } else { MF_UNCHECKED };
        let f13_flags = MF_STRING | if trigger == VK_F13.0 { MF_CHECKED } else { MF_UNCHECKED };
        let ralt_flags = MF_STRING | if trigger == VK_RMENU.0 { MF_CHECKED } else { MF_UNCHECKED };

        let caps_text = wide_string("Caps Lock");
        let f13_text = wide_string("F13");
        let ralt_text = wide_string("Right Alt");

        AppendMenuW(submenu, caps_flags, IDM_KEY_CAPSLOCK as usize, wptr(&caps_text)).ok();
        AppendMenuW(submenu, f13_flags, IDM_KEY_F13 as usize, wptr(&f13_text)).ok();
        AppendMenuW(submenu, ralt_flags, IDM_KEY_RALT as usize, wptr(&ralt_text)).ok();

        // 라디오 체크 스타일 적용
        CheckMenuRadioItem(submenu, IDM_KEY_CAPSLOCK, IDM_KEY_RALT,
            match trigger {
                x if x == VK_CAPITAL.0 => IDM_KEY_CAPSLOCK,
                x if x == VK_F13.0 => IDM_KEY_F13,
                x if x == VK_RMENU.0 => IDM_KEY_RALT,
                _ => IDM_KEY_CAPSLOCK,
            },
            MF_BYCOMMAND.0,
        ).ok();

        let key_menu_text = wide_string("트리거 키(&K)");
        AppendMenuW(menu, MF_STRING | MF_POPUP, submenu.0 as usize, wptr(&key_menu_text)).ok();

        // 구분선
        AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()).ok();

        // 종료
        let exit_text = wide_string("종료(&X)");
        AppendMenuW(menu, MF_STRING, IDM_EXIT as usize, wptr(&exit_text)).ok();

        // 메뉴 표시
        let mut pt = POINT::default();
        GetCursorPos(&mut pt).ok();

        // 포커스 설정 (메뉴 외부 클릭 시 닫히도록)
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTALIGN | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None);
        // WM_NULL 전송으로 메뉴 닫힘 보장
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
                // 좌클릭: 토글
                WM_LBUTTONUP => {
                    let was_enabled = ENABLED.fetch_xor(true, Ordering::SeqCst);
                    let _ = was_enabled; // ENABLED이 이미 토글됨
                    update_tray_icon(hwnd);
                }
                // 우클릭: 컨텍스트 메뉴
                WM_RBUTTONUP => {
                    show_context_menu(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }

        WM_COMMAND => {
            let cmd = (w_param.0 & 0xFFFF) as u32;
            match cmd {
                IDM_TOGGLE => {
                    ENABLED.fetch_xor(true, Ordering::SeqCst);
                    update_tray_icon(hwnd);
                }
                IDM_KEY_CAPSLOCK => {
                    TRIGGER_KEY.store(VK_CAPITAL.0 as u32, Ordering::Relaxed);
                    update_tray_icon(hwnd);
                }
                IDM_KEY_F13 => {
                    TRIGGER_KEY.store(VK_F13.0 as u32, Ordering::Relaxed);
                    update_tray_icon(hwnd);
                }
                IDM_KEY_RALT => {
                    TRIGGER_KEY.store(VK_RMENU.0 as u32, Ordering::Relaxed);
                    update_tray_icon(hwnd);
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

            // 키보드 훅 해제
            let raw = HOOK_HANDLE.load(Ordering::SeqCst);
            if raw != 0 {
                let hook = HHOOK(raw as isize as *mut _);
                let _ = UnhookWindowsHookEx(hook);
            }

            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}

// ── 유틸리티 ──────────────────────────────────────────────────────────────

/// Rust 문자열을 UTF-16 null-terminated 벡터로 변환
fn wide_string(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Vec<u16>을 PCWSTR로 변환
fn wptr(s: &[u16]) -> PCWSTR {
    PCWSTR(s.as_ptr())
}

// ── 메인 ──────────────────────────────────────────────────────────────────

fn main() {
    unsafe {
        let hmodule = GetModuleHandleW(None).unwrap();
        let hinstance: HINSTANCE = hmodule.into();

        // 윈도우 클래스 등록
        let class_name = wide_string("synergy_hangul_fix_wnd");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: wptr(&class_name),
            ..Default::default()
        };
        RegisterClassW(&wc);

        // 히든 윈도우 생성
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            wptr(&class_name),
            wptr(&wide_string("synergy-hangul-fix")),
            WS_OVERLAPPED,
            0, 0, 0, 0,
            HWND_MESSAGE, // 메시지 전용 윈도우
            None,
            Some(&hinstance),
            None,
        )
        .unwrap();

        MAIN_HWND.store(hwnd.0 as usize as u32, Ordering::SeqCst);

        // 키보드 훅 설치
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0);
        match hook {
            Ok(hook) => {
                HOOK_HANDLE.store(hook.0 as usize as u32, Ordering::SeqCst);
            }
            Err(_) => {
                MessageBoxW(
                    None,
                    wptr(&wide_string("키보드 훅 설치에 실패했습니다.\n관리자 권한으로 실행해 주세요.")),
                    wptr(&wide_string("synergy-hangul-fix 오류")),
                    MB_ICONERROR | MB_OK,
                );
                return;
            }
        }

        // 트레이 아이콘 추가
        add_tray_icon(hwnd);

        // 메시지 루프
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
