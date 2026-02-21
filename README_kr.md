# synergy-hangul-fix

Synergy3로 Mac에서 Windows로 키보드를 공유할 때 한글 전환이 안 되는 문제를 해결하는 Windows 시스템 트레이 유틸리티입니다.

[English](README.md)

## 문제

Synergy3 (또는 Deskflow)를 통해 Mac의 키보드를 Windows와 공유하면 한/영 전환 키가 동작하지 않습니다. macOS가 Caps Lock을 자체 소비하거나, Synergy가 Right Alt를 Left Alt(0xA4)로 변환하는 등 키 매핑이 예측 불가능합니다. Windows에서 한글 전환에 필요한 `VK_HANGUL` (0x15)은 전달되지 않습니다.

이 문제는 Deskflow/Synergy 이슈 (#3071, #5242, #5578, #8575)에 오랫동안 보고되어 왔지만 공식 수정이 없는 상태입니다.

## 해결 방법

글로벌 저수준 키보드 훅(`WH_KEYBOARD_LL`)을 설치하여 설정된 트리거 키를 가로채고, Windows IMM API(`ImmSetConversionStatus`)로 한글 IME를 직접 토글합니다. IMM 컨텍스트가 없는 경우 `SendInput`으로 `VK_HANGUL`을 주입하는 폴백을 사용합니다.

## 기능

- **시스템 트레이 아이콘**으로 상태 표시 (활성: 기본 아이콘 / 비활성: 경고 아이콘)
- 트레이 아이콘 **좌클릭**으로 활성/비활성 즉시 전환
- **우클릭** 컨텍스트 메뉴:
  - 활성화/비활성화 토글
  - 트리거 키 선택 (Caps Lock, F13, Right Alt, 또는 커스텀)
  - **키 감지 모드** — Synergy가 실제로 보내는 키코드를 자동 캡처
  - 디버그 로그 윈도우
  - 종료
- **키 감지 + 확인 다이얼로그** — "감지 중..." 팝업 후 감지된 키로 확인 질문
- **설정 파일 자동 저장** — exe 옆에 `config.ini`로 트리거 키 저장, 다음 실행 시 자동 로드
- **디버그 윈도우** — 키 이벤트, IMM API 호출, 트리거 매치 결과를 실시간 확인
- 콘솔 창 없이 순수 Windows GUI 앱으로 동작
- 경량 (~250KB 독립 실행 파일, 외부 의존성 없음)

## 빠른 시작

1. [Releases](../../releases)에서 `synergy-hangul-fix.exe` 다운로드
2. Windows 클라이언트 컴퓨터의 원하는 폴더에 복사
3. 실행
4. 트레이 아이콘 우클릭 → 트리거 키 → **키 감지(&L)...** → Mac에서 원하는 키 누르기
5. 확인 → 완료! 다음 실행 시 자동으로 같은 설정 사용

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
3. IMM API (`ImmGetConversionStatus` / `ImmSetConversionStatus`)로 포그라운드 윈도우의 `IME_CMODE_NATIVE` 비트 토글
4. IMM 컨텍스트가 없는 경우 `SendInput`으로 `VK_HANGUL` 키 다운 + 키 업 주입으로 폴백
5. 원자적(atomic) 재진입 가드로 주입된 키가 다시 잡히는 것을 방지

## 설정

설정은 실행 파일과 같은 디렉토리의 `config.ini`에 저장됩니다:

```ini
trigger_key=0xA4
```

트레이 메뉴에서 트리거 키를 변경하면 자동으로 생성/업데이트됩니다.

## 라이선스

MIT
