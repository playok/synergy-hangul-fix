# synergy-hangul-fix

Synergy3로 Mac에서 Windows로 키보드를 공유할 때 한글 전환이 안 되는 문제를 해결하는 Windows 시스템 트레이 유틸리티입니다.

[English](README.md)

## 문제

Synergy3 (또는 Deskflow)를 통해 Mac의 키보드를 Windows와 공유하면 한/영 전환 키가 동작하지 않습니다. Mac은 `VK_CAPITAL` (Caps Lock)을 전송하지만 Windows에서 한글 전환에 필요한 `VK_HANGUL` (0x15)은 전달되지 않습니다.

이 문제는 Deskflow/Synergy 이슈 (#3071, #5242, #5578, #8575)에 오랫동안 보고되어 왔지만 공식 수정이 없는 상태입니다.

## 해결 방법

글로벌 저수준 키보드 훅(`WH_KEYBOARD_LL`)을 설치하여 트리거 키를 가로채고, `SendInput`으로 `VK_HANGUL`을 주입하여 Caps Lock 입력을 한글 IME 전환 이벤트로 변환합니다.

## 기능

- 시스템 트레이 아이콘으로 상태 표시 (활성/비활성)
- 트레이 아이콘 좌클릭으로 활성/비활성 즉시 전환
- 우클릭 컨텍스트 메뉴:
  - 활성화/비활성화 토글
  - 트리거 키 선택 (Caps Lock, F13, Right Alt)
  - 종료
- 콘솔 창 없이 순수 Windows GUI 앱으로 동작
- 경량 (~250KB 독립 실행 파일, 외부 의존성 없음)

## 설치

1. [Releases](../../releases)에서 `synergy-hangul-fix.exe` 다운로드
2. Windows 클라이언트 컴퓨터의 원하는 폴더에 복사
3. 실행 (별도 설치 불필요)

> 키보드 훅 설치에 실패하면 관리자 권한으로 실행해 보세요.

## 소스에서 빌드

### 사전 요구사항

- Rust 툴체인 (1.70+)
- macOS에서 크로스 컴파일하는 경우:
  - `x86_64-pc-windows-gnu` 타겟: `rustup target add x86_64-pc-windows-gnu`
  - MinGW-w64: `brew install mingw-w64`

### 빌드

```bash
# Windows에서 네이티브 빌드
cargo build --release

# macOS에서 크로스 컴파일
cargo build --release --target x86_64-pc-windows-gnu
```

바이너리 위치: `target/release/synergy-hangul-fix.exe` (크로스 컴파일 시 `target/x86_64-pc-windows-gnu/release/synergy-hangul-fix.exe`)

## 동작 원리

1. `WH_KEYBOARD_LL` 훅으로 모든 키보드 이벤트를 시스템 전역에서 가로채기
2. 설정된 트리거 키가 감지되면 원래 키 이벤트를 차단
3. `SendInput`으로 `VK_HANGUL` 키 다운 + 키 업 시퀀스를 주입
4. 원자적(atomic) 재진입 가드로 주입된 키가 다시 잡히는 것을 방지

## 라이선스

MIT
