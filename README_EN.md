# ctdozlite

**[Tiếng Việt](README.md)** · **[English](README_EN.md)** · **[한국어](README_KO.md)** · **[日本語](README_JA.md)** · **[简体中文](README_ZH.md)**

A pure Rust Zalo Desktop client (`eframe`/`egui` + `ratatui`) communicating directly with Zalo Web/PC APIs and WebSocket services without Electron or Chromium.

- **Project Homepage (Website & Download):** [https://ctdozlite.ctdo.net/](https://ctdozlite.ctdo.net/)
- **Prebuilt Packages (Releases):** [https://github.com/ctdoteam1110/ctdozlite/releases](https://github.com/ctdoteam1110/ctdozlite/releases)
- **Feature Matrix & Guides:** [Features & Limits](docs/TINH_NANG.md) · [Known Issues](docs/DA_BIET.md) · [Troubleshooting](docs/KHAC_PHUC_SU_CO.md)

---

## ⚡ Highlights

- **Pure Rust Performance:** Startup in milliseconds, minimal RAM footprint (~20-50MB), near-zero idle CPU usage.
- **Cross-Platform:** Native builds for Windows 10/11 (MSVC & GNU), macOS (Apple Silicon arm64 & Intel x86_64), and Linux (Debian, Ubuntu, Fedora Wayland/X11).
- **Dual Mode:** Full-featured GUI (Atom One Dark theme) and terminal-based TUI (`ratatui`).
- **VoIP Audio & Video Calls:** 100% Rust VoIP engine supporting Annex-B H.264 video decoding, RTP/RTCP protocols, and Opus audio resampling.
- **Security & Privacy:** Local SQLite storage (`zalo_history.db`), configurable data directory via `--data-dir`, zero telemetry.

---

## 📥 Download & Installation

Prebuilt installers and portable binaries are available on [GitHub Releases](https://github.com/ctdoteam1110/ctdozlite/releases):

| OS | Package | Instructions |
| :--- | :--- | :--- |
| **Windows** | `.exe` Installer / Portable `.zip` | Run installer or extract and run `zalo-rust.exe` |
| **macOS** | Universal `.dmg` (Apple Silicon & Intel) | Open `.dmg`, drag `ctdozlite.app` to `/Applications` |
| **Linux** | `.deb` / `.rpm` / `.AppImage` / `.tar.gz` | Install via package manager or run directly with `chmod +x` |

### CLI Options

| Option | Description | Example |
| :--- | :--- | :--- |
| `--data-dir <PATH>` | Set custom directory for session, database, cache, and logs | `zalo-rust --data-dir ./my-data --gui` |
| `--print-data-dir` | Print resolved data directory and exit | `zalo-rust --print-data-dir` |
| `--gui` | Launch graphical user interface (default) | `zalo-rust --gui` |
| `--tui` | Launch terminal user interface | `zalo-rust --tui` |
| `--qr` | Request login QR code immediately | `zalo-rust --tui --qr` |
| `--logout` | Clear saved session and exit | `zalo-rust --logout` |
| `--version` / `-V` | Print version and exit without initializing storage | `zalo-rust --version` |

---

## 🛠️ Building from Source

### Prerequisites
- **Rust Toolchain:** Stable Rust (1.80+ recommended) via `rustup`.
- **System Dependencies:**
  - **Linux:** `libasound2-dev`, `libfontconfig1-dev`, `libx11-dev`, `libxcursor-dev`, `libxi-dev`, `libgl1-mesa-dev`, `ffmpeg`.
  - **Windows / macOS:** Standard C toolchain and `ffmpeg` (for media/video calls).

```bash
# Clone the repository
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite

# Build optimized release binary
cargo build --release

# Run GUI
./target/release/zalo-rust --gui
```

---

## 🏆 Key Contributors

Core contributors with representative code commits in the repository:

| Contributor | GitHub | Key Role | Code Volume |
| :--- | :--- | :--- | :---: |
| **TIPC1110** | [@TIPC1110](https://github.com/TIPC1110) | Core architecture, VoIP call engine, egui framework, cross-platform | **> 73,000+ lines** |
| **thanhdo1110** | [@thanhdo1110](https://github.com/thanhdo1110) | Modular GUI refactoring, Atom One Dark design system | **> 14,000+ lines** |
| **ngxccc** | [@ngxccc](https://github.com/ngxccc) | Engineering standards, session safety, domain resolution | **> 3,000+ lines** |
| **Thuong Tin** | [@thuongtin](https://github.com/thuongtin) | OS local timezone patches, customizable macOS data directory | **~840+ lines** |
| **GrootTheDeveloper** | [@GrootTheDeveloper](https://github.com/GrootTheDeveloper) | SQLite startup speedup, chat virtualization, CJK font lazy loading | **~370+ lines** |

> *Note: The complete hall of fame honoring all contributors (bug reports, feature requests, pull requests) is published at [Contributors Wall](https://ctdozlite.ctdo.net/contributors.html).*

---

## 📜 License

Distributed under the dual **[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)** license. Developed strictly for educational and system-level engineering research purposes.
