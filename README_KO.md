# ctdozlite

**[Tiếng Việt](README.md)** · **[English](README_EN.md)** · **[한국어](README_KO.md)** · **[日本語](README_JA.md)** · **[简体中文](README_ZH.md)**

Electron이나 Chromium 없이 Rust(`eframe`/`egui` + `ratatui`)로 제작되어 Zalo Web/PC API 및 WebSocket과 직접 통신하는 순수 Rust 기반 Zalo 데스크톱 클라이언트입니다.

- **프로젝트 공식 웹사이트 (다운로드):** [https://ctdozlite.ctdo.net/](https://ctdozlite.ctdo.net/)
- **공식 릴리스 패키지:** [https://github.com/ctdoteam1110/ctdozlite/releases](https://github.com/ctdoteam1110/ctdozlite/releases)
- **문서 및 문제 해결:** [기능 및 제한 사항](docs/TINH_NANG.md) · [알려진 문제](docs/DA_BIET.md) · [문제 해결 가이드](docs/KHAC_PHUC_SU_CO.md)

---

## ⚡ 주요 특징

- **초고속 Rust 성능:** 수 밀리초 단위의 빠른 실행 속도, 초경량 메모리 사용량 (~20-50MB RAM), 유휴 상태 CPU 사용률 0%에 근접.
- **완벽한 크로스 플랫폼 지원:** Windows 10/11 (MSVC & GNU), macOS (Apple Silicon arm64 & Intel x86_64 Universal), Linux (Debian, Ubuntu, Fedora Wayland/X11).
- **듀얼 모드 지원:** 풀 피처 GUI (Atom One Dark 테마) 및 터미널 기반 TUI (`ratatui`).
- **VoIP 음성 및 영상 통화:** 순수 Rust로 구현된 VoIP 엔진 (Annex-B H.264 디코딩, RTP/RTCP 프로토콜, Opus 오디오 리샘플링 지원).
- **보안 및 프라이버시:** 로컬 SQLite 데이터베이스 (`zalo_history.db`) 저장, `--data-dir`를 통한 데이터 경로 지정 가능, 외부 원격 분석(Telemetry) 0%.

---

## 📥 다운로드 및 설치

[GitHub Releases](https://github.com/ctdoteam1110/ctdozlite/releases)에서 사전 빌드된 바이너리를 다운로드할 수 있습니다:

| 운영체제 | 패키지 형태 | 설치 방법 |
| :--- | :--- | :--- |
| **Windows** | `.exe` 설치 프로그램 / 포터블 `.zip` | 설치 파일 실행 또는 압축 해제 후 `zalo-rust.exe` 실행 |
| **macOS** | Universal `.dmg` (Apple Silicon & Intel) | `.dmg` 마운트 후 `ctdozlite.app`을 `/Applications`로 드래그 |
| **Linux** | `.deb` / `.rpm` / `.AppImage` / `.tar.gz` | 패키지 매니저로 설치하거나 `chmod +x` 실행 권한 부여 후 직접 실행 |

### CLI 명령어 옵션

| 옵션 | 설명 | 실행 예시 |
| :--- | :--- | :--- |
| `--data-dir <PATH>` | 세션, 데이터베이스, 캐시, 로그 저장 경로 직접 지정 | `zalo-rust --data-dir ./my-data --gui` |
| `--print-data-dir` | 사용 중인 데이터 디렉터리 경로 출력 후 즉시 종료 | `zalo-rust --print-data-dir` |
| `--gui` | 그래픽 사용자 인터페이스(GUI) 모드로 실행 (기본값) | `zalo-rust --gui` |
| `--tui` | 터미널 콘솔(TUI) 모드로 실행 | `zalo-rust --tui` |
| `--qr` | TUI 모드 시작 즉시 로그인 QR 코드 생성 요청 | `zalo-rust --tui --qr` |
| `--logout` | 저장된 세션 로그아웃 및 삭제 후 종료 | `zalo-rust --logout` |
| `--version` / `-V` | 버전 정보 출력 후 즉시 종료 (데이터 폴더 미생성) | `zalo-rust --version` |

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
./target/release/zalo-rust --gui
```

---

## 🏆 주요 기여자 (Key Contributors)

리포지토리에 핵심 코드를 기여한 대표 개발자 명단:

| 기여자 | GitHub | 주요 역할 | 기여 규모 |
| :--- | :--- | :--- | :---: |
| **TIPC1110** | [@TIPC1110](https://github.com/TIPC1110) | 핵심 아키텍처, VoIP 통화 엔진, egui 프레임워크, 크로스 플랫폼 | **> 73,000+ 줄** |
| **thanhdo1110** | [@thanhdo1110](https://github.com/thanhdo1110) | 모듈형 GUI 리팩터링, Atom One Dark 디자인 시스템 | **> 14,000+ 줄** |
| **ngxccc** | [@ngxccc](https://github.com/ngxccc) | 엔지니어링 표준 수립, 세션 안전성 확보, 네트워크 도메인 확인 | **> 3,000+ 줄** |
| **Thuong Tin** | [@thuongtin](https://github.com/thuongtin) | OS 로컬 시간대 패치, macOS 데이터 디렉터리 분리 | **~840+ 줄** |
| **GrootTheDeveloper** | [@GrootTheDeveloper](https://github.com/GrootTheDeveloper) | SQLite 빠른 시작 최적화, 채팅 가상화, CJK 폰트 지연 로딩 | **~370+ 줄** |

> *참고: 버그 제보, 기능 제안, PR을 포함한 모든 기여자의 전체 명단은 [기여자 명예의 전당](https://ctdozlite.ctdo.net/contributors.html)에서 확인하실 수 있습니다.*

---

## 📜 라이선스

이 프로젝트는 **[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)** 듀얼 라이선스 하에 배포됩니다. 본 프로젝트는 순수 학술 및 시스템 프로그래밍 연구 목적으로 개발되었습니다.
