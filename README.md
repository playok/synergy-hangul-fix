# synergy-hangul-fix

A Windows system tray utility that fixes Korean (Hangul) input toggle when using Synergy3 from a Mac server to a Windows client.

[한국어 문서](README_kr.md)

## Problem

When sharing a keyboard from Mac to Windows via Synergy3 (or Deskflow), the Korean input toggle key does not work. Synergy remaps keys unpredictably — macOS may consume Caps Lock entirely, and Right Alt often arrives as Left Alt (0xA4). The `VK_HANGUL` (0x15) that Windows needs for Korean IME toggle is never sent.

This is a long-standing issue documented in multiple Deskflow/Synergy bug reports (#3071, #5242, #5578, #8575).

## Solution

This program installs a global low-level keyboard hook (`WH_KEYBOARD_LL`) that intercepts a configurable trigger key and toggles the Korean IME using the Windows IMM API (`ImmSetConversionStatus`), with `VK_HANGUL` via `SendInput` as a fallback.

## Features

- **System tray icon** with status indication (active: default icon / inactive: warning icon)
- **Left-click** tray icon to toggle enable/disable
- **Right-click** context menu:
  - Enable/Disable toggle
  - Trigger key selection (Caps Lock, F13, Right Alt, or custom)
  - **Key detect mode** — automatically captures whatever key Synergy actually sends
  - Debug log window
  - Exit
- **Key detect with confirmation** — shows "감지 중..." popup, then a confirm dialog with the detected key
- **Persistent config** — saves trigger key to `config.ini` next to the exe, auto-loaded on next startup
- **Debug window** — real-time log of all key events, IMM API calls, and trigger matches
- No console window (native Windows GUI application)
- Lightweight (~250KB standalone executable, no dependencies)

## Quick Start

1. Download `synergy-hangul-fix.exe` from [Releases](../../releases)
2. Copy to any folder on the Windows client machine
3. Run the executable
4. Right-click tray icon → Trigger Key → **Key Detect...** → press your desired key on Mac
5. Confirm → done! The setting is saved for next time.

> If the keyboard hook fails to install, try running as Administrator.

## Building from Source

### Prerequisites

- Rust toolchain (1.70+)
- For cross-compilation from macOS:
  - `x86_64-pc-windows-gnu` target: `rustup target add x86_64-pc-windows-gnu`
  - MinGW-w64: `brew install mingw-w64`

### Build

```bash
# Native Windows build
cargo build --release

# Cross-compile from macOS
cargo build --release --target x86_64-pc-windows-gnu
```

The binary will be at `target/release/synergy-hangul-fix.exe` (or `target/x86_64-pc-windows-gnu/release/synergy-hangul-fix.exe` for cross-compilation).

## How It Works

1. A `WH_KEYBOARD_LL` hook intercepts all keyboard events system-wide
2. When the configured trigger key is detected, the original event is suppressed
3. The IMM API (`ImmGetConversionStatus` / `ImmSetConversionStatus`) toggles `IME_CMODE_NATIVE` on the foreground window
4. If IMM context is unavailable, falls back to `VK_HANGUL` injection via `SendInput`
5. An atomic re-entry guard prevents the injected key from being intercepted again

## Configuration

Settings are stored in `config.ini` in the same directory as the executable:

```ini
trigger_key=0xA4
```

The file is automatically created/updated when you change the trigger key via the tray menu.

## License

MIT
