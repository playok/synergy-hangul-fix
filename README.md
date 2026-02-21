# synergy-hangul-fix

A Windows system tray utility that fixes Korean (Hangul) input toggle when using Synergy3 from a Mac server to a Windows client.

[한국어 문서](README_kr.md)

## Problem

When sharing a keyboard from Mac to Windows via Synergy3 (or Deskflow), the Korean input toggle key does not work. The Mac sends `VK_CAPITAL` (Caps Lock) instead of `VK_HANGUL` (0x15), so Windows never receives the proper Hangul toggle signal.

This is a long-standing issue documented in multiple Deskflow/Synergy bug reports (#3071, #5242, #5578, #8575).

## Solution

This program installs a global low-level keyboard hook (`WH_KEYBOARD_LL`) that intercepts the trigger key and injects `VK_HANGUL` via `SendInput`, effectively translating Caps Lock presses into Korean IME toggle events.

## Features

- System tray icon with status indication (active/inactive)
- Left-click tray icon to toggle enable/disable
- Right-click context menu:
  - Enable/Disable toggle
  - Trigger key selection (Caps Lock, F13, Right Alt)
  - Exit
- No console window (runs as a native Windows GUI application)
- Lightweight (~250KB standalone executable, no dependencies)

## Installation

1. Download `synergy-hangul-fix.exe` from [Releases](../../releases)
2. Copy to any folder on the Windows client machine
3. Run the executable (no installation needed)

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
3. A `VK_HANGUL` key down + key up sequence is injected via `SendInput`
4. An atomic re-entry guard prevents the injected key from being intercepted again

## License

MIT
