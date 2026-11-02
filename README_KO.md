# ctdozlite

**[Tiếng Việt](README.md)** · **[English](README_EN.md)** · **[한국어](README_KO.md)** · **[日本語](README_JA.md)** · **[简体中文](README_ZH.md)**

Electron이나 Chromium 없이 Rust(`eframe`/`egui` + `ratatui`)로 제작되어 Zalo Web/PC API 및 WebSocket과 직접 통신하는 순수 Rust 기반 Zalo 데스크톱 클라이언트입니다.

- **프로젝트 공식 웹사이트 (다운로드):** [https://ctdozlite.ctdo.net/](https://ctdozlite.ctdo.net/)
- **공식 릴리스 패키지:** [https://github.com/ctdoteam1110/ctdozlite/releases](https://github.com/ctdoteam1110/ctdozlite/releases)
- **문서 및 문제 해결:** [기능 및 제한 사항](docs/TINH_NANG.md) · [알려진 문제](docs/DA_BIET.md) · [문제 해결 가이드](docs/KHAC_PHUC_SU_CO.md)

---

## ⚡ 주요 특징

- **초고속 Rust 성능:** ~0.18초 초고속 콜드 스타트 (<200ms), 대기 상태 RAM 약 149.5 MB (공식 Electron 앱 1.8~1.9 GB 대비 92% 이상 절감), 1개 단일 네이티브 프로세스, 유휴 CPU 0%에 근접.
- **완벽한 크로스 플랫폼 지원:** Windows 10/11 (MSVC & GNU), macOS (Apple Silicon arm64 & Intel x86_64 Universal), Linux (Debian, Ubuntu, Fedora Wayland/X11).
- **듀얼 모드 지원:** 풀 피처 GUI (Atom One Dark 테마) 및 터미널 기반 TUI (`ratatui`).
- **VoIP 음성 및 영상 통화:** 순수 Rust로 구현된 VoIP 엔진 (Annex-B H.264 디코딩, RTP/RTCP 프로토콜, Opus 오디오 리샘플링 지원).
- **보안 및 프라이버시:** 로컬 SQLite 데이터베이스 (`zalo_history.db`) 저장, `--data-dir`를 통한 데이터 경로 지정 가능, 외부 원격 분석(Telemetry) 0%.

---

## 📥 다운로드 및 설치

[GitHub Releases](https://github.com/ctdoteam1110/ctdozlite/releases)에서 사전 빌드된 바이너리를 다운로드할 수 있습니다:

| 운영체제 | 패키지 형태 | 설치 방법 |
| :--- | :--- | :--- |
| **Windows** | `.exe` 설치 프로그램 / 포터블 `.zip` | 설치 파일 실행 또는 압축 해제 후 `ctdozlite.exe` 실행 |
| **macOS** | Universal `.dmg` (Apple Silicon & Intel) | `.dmg` 마운트 후 `ctdozlite.app`을 `/Applications`로 드래그 |
| **Linux** | `.deb` / `.rpm` / `.AppImage` / `.tar.gz` | 패키지 매니저로 설치하거나 `chmod +x` 실행 권한 부여 후 직접 실행 |

### CLI 명령어 옵션

| 옵션 | 설명 | 실행 예시 |
| :--- | :--- | :--- |
| `--data-dir <PATH>` | 세션, 데이터베이스, 캐시, 로그 저장 경로 직접 지정 | `ctdozlite --data-dir ./my-data --gui` |
| `--print-data-dir` | 사용 중인 데이터 디렉터리 경로 출력 후 즉시 종료 | `ctdozlite --print-data-dir` |
| `--gui` | 그래픽 사용자 인터페이스(GUI) 모드로 실행 (기본값) | `ctdozlite --gui` |
| `--tui` | 터미널 콘솔(TUI) 모드로 실행 | `ctdozlite --tui` |
| `--qr` | TUI 모드 시작 즉시 로그인 QR 코드 생성 요청 | `ctdozlite --tui --qr` |
| `--logout` | 저장된 세션 로그아웃 및 삭제 후 종료 | `ctdozlite --logout` |
| `--version` / `-V` | 버전 정보 출력 후 즉시 종료 (데이터 폴더 미생성) | `ctdozlite --version` |

---

## 🛠️ 소스 코드 빌드 방법

### 사전 요구사항
- **Rust 툴체인:** Stable Rust (1.80+ 권장, `rustup`으로 설치).
- **시스템 종속성:**
  - **Linux:** `libasound2-dev`, `libfontconfig1-dev`, `libx11-dev`, `libxcursor-dev`, `libxi-dev`, `libgl1-mesa-dev`, `ffmpeg`.
  - **Windows / macOS:** 표준 C 툴체인 및 `ffmpeg` (영상/음성 통화용).

```bash
# 리포지토리 클론
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite

# 릴리스 바이너리 빌드
cargo build --release

# GUI 실행
./target/release/ctdozlite --gui
```

---

## 🏆 주요 기여자 (Key Contributors)

리포지토리에 핵심 코드를 기여한 대표 개발자 명단:

| 기여자 | GitHub | 주요 역할 | 기여 규모 |
| :--- | :--- | :--- | :---: |
| **TIPC1110** | [@TIPC1110](https://github.com/TIPC1110) | 핵심 아키텍처, VoIP 통화 엔진, egui 프레임워크, 소셜 피드, 크로스 플랫폼 | **> 77,000+ 줄** |
| **thanhdo1110** | [@thanhdo1110](https://github.com/thanhdo1110) | 모듈형 GUI 리팩터링, Atom One Dark 디자인 시스템, MCP/REST API 및 예약 메시지 | **> 20,000+ 줄** |
| **Thuong Tin** | [@thuongtin](https://github.com/thuongtin) | 7개 PR: OS 타임존, 데이터 디렉터리, macOS 헬퍼, 세션 원자적 저장, 미디어 캐시, 업데이터, 메시지 모델 | **> 5,950+ 줄** 🏆 |
| **ngxccc** | [@ngxccc](https://github.com/ngxccc) | 5개 PR: 엔지니어링 표준 수립, 세션 안전성 확보, 네트워크 도메인 확인 | **> 3,090+ 줄** |
| **GrootTheDeveloper** | [@GrootTheDeveloper](https://github.com/GrootTheDeveloper) | 8개 PR: SQLite 빠른 시작 최적화, 그룹/클라우드 기록 동기화(#45), 워커 폴 타이머(#46), 채팅 가상화, CJK 폰트 지연 로딩 | **> 760+ 줄** |

> *참고: 버그 제보, 기능 제안, PR을 포함한 모든 기여자의 전체 명단은 [기여자 명예의 전당](https://ctdozlite.ctdo.net/contributors.html)에서 확인하실 수 있습니다.*

---


---

## 🙏 감사의 말

`ctdozlite`는 베트남 수천만 사용자에게 서비스를 제공하는 뛰어난 메신저 플랫폼을 만든 **VNG Corporation**과 **Zalo** 개발팀에게 깊은 감사를 표합니다.

### 프로젝트 목적

- 본 프로젝트는 **단 하나의 목적**을 위해 존재합니다: 학습 및 시스템 엔지니어링 연구, 즉 현대 메신저 애플리케이션이 네트워크 프로토콜, 암호화, 네이티브 데스크톱 아키텍처 계층에서 어떻게 동작하는지 이해하는 것입니다.
- 모든 소스 코드는 Zalo Web/PC와 서버 간의 공개 데이터 흐름을 관찰하여 **Rust로 처음부터 작성**되었습니다. 본 프로젝트는 Zalo의 독점 자산(원본 아이콘, 스티커, 오디오, 이미지, JS/WASM 코드)을 **디컴파일하거나 추출하거나 재배포하지 않습니다**.
- 당사는 `Zalo` 상표, 로고 또는 VNG의 지식재산권을 **소유하지 않으며 소유권을 주장하지 않습니다**. `Zalo`라는 이름은 본 소프트웨어가 **어떤 서비스의 프로토콜과 호환되는지**를 설명하기 위한 서술적 용도로만 사용됩니다.
- 본 프로젝트는 **완전히 비상업적**입니다: 100% 오픈 소스, 요금 없음, 광고 없음, 데이터 판매 없음.

### 데이터 프라이버시

- 모든 메시지와 사용자 데이터는 **사용자 컴퓨터에 로컬로** 저장됩니다 (`zalo_history.db`, `session.json`, `threads_cache.json`).
- 애플리케이션은 관리자 서버나 제3자에게 **어떤 데이터도 전송하지 않습니다**. 유일한 네트워크 연결은 브라우저로 `chat.zalo.me`에 접속할 때와 동일하게 공식 Zalo 시스템으로 향합니다.

### 삭제 요청 정책

VNG Corporation 또는 Zalo의 권한 있는 대리인이 본 프로젝트와 관련하여 어떠한 요청(삭제, 이름 변경, 내용 조정 등)을 하시려면 **GitHub Issues** 또는 관리자 이메일로 연락해 주시기 바랍니다. 당사는 **72시간 이내에 답변**하고 **모든 정당한 요청을 준수**할 것을 약속합니다.

> **요약:** VNG와 Zalo 팀이 훌륭한 제품을 만들어 주신 것에 깊이 감사드립니다. 본 프로젝트는 Rust를 사랑하고 현대 메신저 시스템을 더 깊이 이해하고자 하는 소규모 개발자 그룹의 학습 노력일 뿐입니다.

## 📜 라이선스

이 프로젝트는 **[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)** 듀얼 라이선스 하에 배포됩니다. 본 프로젝트는 순수 학술 및 시스템 프로그래밍 연구 목적으로 개발되었습니다.
