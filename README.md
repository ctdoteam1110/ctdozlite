# ctdozlite

Client Zalo Desktop viết bằng Rust (`eframe`/`egui` + `ratatui`), giao tiếp trực tiếp với hệ thống API và WebSocket của Zalo Web/PC mà không qua Electron hay Chromium.

- **Trang chủ dự án (Website & Tải nhanh):** [https://ctdozlite.ctdo.net/](https://ctdozlite.ctdo.net/)
- **Gói cài đặt dựng sẵn (Releases):** [https://github.com/ctdoteam1110/ctdozlite/releases](https://github.com/ctdoteam1110/ctdozlite/releases)

[![Website](https://img.shields.io/badge/website-ctdozlite.ctdo.net-0068FF.svg)](https://ctdozlite.ctdo.net/)
[![Release](https://img.shields.io/badge/release-v0.0.1-success.svg)](https://github.com/ctdoteam1110/ctdozlite/releases)
[![CI & Release](https://github.com/ctdoteam1110/ctdozlite/actions/workflows/ci-release.yml/badge.svg)](https://github.com/ctdoteam1110/ctdozlite/actions/workflows/ci-release.yml)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue.svg)](https://github.com/ctdoteam1110/ctdozlite/releases)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

![Giao diện ctdozlite Native trên Windows](docs/assets/demo-windows.png)

---

## Tổng quan

Bản Zalo PC chính thức đóng gói trên Electron thường chiếm 350–600 MB RAM khi chạy nền. `ctdozlite` được viết lại từ đầu bằng Rust dựa trên phân tích luồng giao thức của Zalo Web/PC (`zpw_ver=694`, `zpw_type=30`), tập trung vào tốc độ phản hồi, mức tiêu thụ tài nguyên thấp và khả năng đóng gói thành một binary duy nhất.

| Thông số | Zalo PC (Electron) | ctdozlite (Rust) |
|---|---|---|
| Bộ nhớ RAM khi chạy nền | 350 – 550 MB | ~15 – 28 MB (tùy số lượng ảnh/sticker đang mở) |
| Dung lượng bản build | ~180 MB | ~11 MB (single binary) |
| Khởi động & nạp hội thoại | 2.5 – 4.0s | < 100ms (nạp từ cache cục bộ trước khi đồng bộ mạng) |
| Phụ thuộc runtime | Chromium, Node.js | Native binary (`ffmpeg` tùy chọn nếu gọi video) |

---

## Các tính năng chính

- **Xác thực & Quản lý phiên (`src/qr.rs`, `src/login.rs`, `src/session.rs`)**
  - Đăng nhập qua mã QR (`reqqr`, long-polling chờ quét và xác nhận trên điện thoại) hoặc mật khẩu (`PasswordCipher` + `ParamCipher v2`).
  - Lưu phiên tại `session.json`, tự động kiểm tra `zpw_sek` khi mở lại và chuyển về màn hình QR nếu phiên đã hết hạn trên server.
- **Nhắn tin thời gian thực (`src/chat.rs`, `src/socket.rs`, `src/store.rs`)**
  - Kết nối WebSocket (`zpw_ws` frame binary AES-GCM/deflate) kết hợp HTTP polling dự phòng (`get_last_msgs`).
  - Hỗ trợ chat cá nhân (1-1), nhóm (`getlg/v4`), Cloud của tôi, trả lời trích dẫn (quote), chuyển tiếp, thả cảm xúc (reaction), ghim, đánh dấu sao, thu hồi tin nhắn với mọi người và xóa phía người dùng.
  - Gửi/nhận văn bản, hình ảnh (hỗ trợ dán trực tiếp từ clipboard `Ctrl+V` / `Cmd+V`), tập tin đính kèm, nhãn dán và xem trước liên kết (OpenGraph / `parselink`).
  - Tự động phân giải tên hiển thị và avatar cho số lạ thông qua `getminiprofiles`.
- **Gợi ý & Tìm kiếm Sticker theo ngữ cảnh (`src/chat.rs`, `src/gui/stickers.rs`)**
  - Tự động gợi ý nhãn dán ngay trên thanh soạn thảo khi gõ từ khóa, cụm từ tiếng Việt hoặc biểu tượng cảm xúc (tương tự `chat.zalo.me`).
  - Truy vấn song song hai endpoint `/api/message/sticker/suggest/stickers` (`cmd 11741`) và `/api/message/sticker/search` (`cmd 11747`), tự động đối chiếu cả biến thể tiếng Việt có dấu, không dấu và từ viết tắt phổ biến (`tks`, `hpbd`, `g9`, `met qua` ↔ `mệt quá`, `co len` ↔ `cố lên`...).
  - Hỗ trợ tìm kiếm theo cụm từ cuối câu và tải trước (prefetch) ảnh sticker song song ở luồng nền, lưu cache tại `stickers_cache/`.
- **Lưu trữ cục bộ 2 tầng (`src/store.rs`)**
  - `threads_cache.json`: Snapshot danh sách hội thoại giúp hiển thị giao diện ngay khi mở app.
  - `zalo_history.db` (SQLite): Lưu trữ lịch sử tin nhắn lâu dài, khử trùng lặp theo `msg_id` và mốc thời gian.
- **Gọi thoại & Gọi video 1-1 (`src/call.rs`, `src/media.rs`, `src/audio.rs`)**
  - Triển khai trực tiếp luồng báo hiệu cuộc gọi (`401`/`416`/`407`/`402`/`408`/`409`) và bắt tay ZRTP relay bằng Rust, không phụ thuộc `ZaloCall.exe`.
  - Audio hai chiều qua `cpal` + `opus`; video H.264 đóng gói theo khung relay của Zalo (RTP PT 97/98, RTCP SR/PLI/REMB) kết hợp `ffmpeg` để thu webcam và giải mã khung hình.
- **Hai chế độ giao diện (`src/gui/`, `src/tui.rs`)**
  - **GUI (`eframe`/`egui`)**: Giao diện desktop native, hỗ trợ Dark/Light theme, điều hướng sticker bằng phím mũi tên, thông báo tin nhắn chưa đọc và khay Dev Inspector (`--dev`).
  - **TUI (`ratatui`/`crossterm`)**: Giao diện terminal gọn nhẹ cho môi trường CLI/SSH.

---

## Cấu trúc mã nguồn

```text
src/
├── main.rs            # Điểm vào CLI, phân tích tham số khởi chạy (--gui, --tui, --dev, --logout)
├── lib.rs             # Khai báo các module thư viện
├── session.rs         # Quản lý phiên, cookie jar (zpw_sek), khóa giải mã (zpw_enk) và bảng domain
├── crypto.rs          # AES-128/256-CBC, PKCS7, MD5 và hàm tạo chữ ký signkey
├── param_cipher.rs    # Mã hóa tham số xác thực ParamCipher v2 (zcid, zcid_ext, encrypt_key)
├── qr.rs              # Luồng sinh mã QR, long-polling chờ quét và xác nhận đăng nhập
├── login.rs           # Đăng nhập mật khẩu (PasswordCipher), getServerInfo, getLoginInfo
├── socket.rs          # Kết nối WebSocket thời gian thực (zpw_ws)
├── chat.rs            # API hội thoại, tin nhắn, nhóm, đính kèm, link preview và tìm kiếm sticker
├── store.rs           # Lưu trữ SQLite (zalo_history.db) và đồng bộ lịch sử
├── call.rs            # Báo hiệu VoIP, điều khiển phiên gọi 1-1 và bắt tay ZRTP
├── media.rs           # Đóng gói/giải mã RTP/RTCP, H.264 FU-A và luồng webcam/video
├── audio.rs           # Thu/phát âm thanh thời gian thực (cpal + libopus)
├── voip_log.rs        # Ghi nhận chẩn đoán luồng gói tin RTP/RTCP (voip.log)
├── logger.rs          # Bộ thu thập log mạng và quét mã lỗi cho chế độ --dev
├── tui.rs             # Giao diện dòng lệnh (ratatui)
└── gui/               # Giao diện đồ họa native (eframe/egui)
    ├── theme.rs       # Bảng màu Dark/Light, vector icon Lucide, nạp font hệ thống
    ├── stickers.rs    # Sticker picker, auto-suggest khi gõ và bộ lọc nhãn dán
    ├── worker.rs      # Luồng nền Tokio xử lý lệnh mạng và prefetch ảnh/sticker
    ├── workspace.rs   # Bố cục cột danh bạ, khung chat chính và ngăn thông tin hội thoại
    ├── messages.rs    # Render bong bóng tin nhắn, reaction, quote, ảnh clipboard
    └── modals.rs      # Hộp thoại cài đặt, cuộc gọi, xem ảnh, tra cứu SĐT và Dev Inspector
```

---

## Tải bản cài đặt dựng sẵn (Releases — v0.0.1)

Toàn bộ gói cài đặt dựng sẵn cho từng hệ điều hành và các bản phân phối Linux nằm tại **[GitHub Releases (v0.0.1)](https://github.com/ctdoteam1110/ctdozlite/releases)**. Binary Linux được liên kết tĩnh `libopus`, `sqlite3` và `rustls`, đồng thời cung cấp đủ định dạng để cài đặt trên cả distro đời mới lẫn đời cũ (`glibc >= 2.31`):

### 1. Cài đặt nhanh trên Linux (Mọi bản phân phối)

- **Ubuntu 22.04 / 24.04, Debian 12+, Linux Mint 21+, Kali, Pop!_OS, Zorin (`.deb`):**
  ```bash
  sudo apt install ./ctdozlite_0.0.1_amd64.deb
  ```
- **Ubuntu 20.04, Debian 11, Linux Mint 20 hoặc distro nhân cũ `glibc 2.31+` (`.deb` legacy):**
  ```bash
  sudo apt install ./ctdozlite_0.0.1_legacy_glibc2.31_amd64.deb
  ```
- **Fedora, RHEL, Rocky Linux, AlmaLinux, CentOS Stream, openSUSE (`.rpm`):**
  ```bash
  sudo dnf install ./ctdozlite-0.0.1-1.x86_64.rpm
  ```
- **AppImage (Chạy trực tiếp trên mọi bản Linux, không cần quyền root hay cài đặt):**
  ```bash
  chmod +x ctdozlite-0.0.1-x86_64.AppImage
  ./ctdozlite-0.0.1-x86_64.AppImage
  ```
- **Arch Linux, Manjaro, EndeavourOS, Gentoo, Void, NixOS hoặc cài thủ công (`.tar.gz`):**
  ```bash
  tar -xzf ctdozlite-0.0.1-linux-x86_64.tar.gz
  cd ctdozlite-0.0.1-linux-x86_64
  ./install.sh
  ```

### 2. Cài đặt trên Windows & macOS
- **Windows (x64):** Chạy bộ cài `ctdozlite-0.0.1-setup-windows-x86_64.exe` hoặc bản chạy ngay `ctdozlite-0.0.1-portable-windows-x86_64.exe`.
- **macOS:** Mở `ctdozlite-0.0.1-macos-arm64.dmg` (Apple Silicon) hoặc `ctdozlite-0.0.1-macos-x86_64.dmg` (Intel) và kéo `ctdozlite.app` vào `/Applications`.

> [!IMPORTANT]
> **Phiên bản iOS — Dùng 1 tài khoản Zalo song song trên 2 máy (Sắp ra mắt):**
> Bản iOS của `ctdozlite` hoạt động dưới dạng phiên phụ (`zpw_type=30`), cho phép đăng nhập và sử dụng **chung 1 tài khoản Zalo song song trên 2 máy điện thoại** mà không làm văng thiết bị chính. Hiện tại tài khoản Apple Developer của mình vừa bị Apple thu hồi chứng chỉ và phần giao diện cảm ứng chưa hoàn thiện trọn vẹn, nên gói `.ipa` sẽ sớm được bổ sung trong bản cập nhật tới.

---

## Tự biên dịch từ mã nguồn (Windows / Linux / macOS)

### 1. Yêu cầu chung
- **Rust** 1.75 trở lên ([rustup.rs](https://rustup.rs/))
- **CMake** (3.15+), `pkg-config` và trình biên dịch C/C++ (để biên dịch tĩnh `libopus` và `sqlite3`)
- **FFmpeg** trong `PATH` (tùy chọn — chỉ cần khi sử dụng tính năng gọi video H.264)

> Nếu máy dùng CMake 4.0+ gặp lỗi kiểm tra phiên bản tối thiểu trong `opus-sys`, đặt biến môi trường `CMAKE_POLICY_VERSION_MINIMUM=3.5` khi chạy `cargo build`.

### 2. Windows (MSVC)
Cài đặt *Visual Studio Build Tools* (chọn *Desktop development with C++* và *CMake tools for Windows*):

```powershell
winget install Gyan.FFmpeg
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite
$env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"
cargo build --release
```
Binary đầu ra: `target\release\zalo-rust.exe`

### 3. Linux (Debian / Ubuntu / Fedora / Arch)
Cài đặt các gói thư viện hệ thống cho âm thanh (`ALSA`) và cửa sổ (`X11` / `Wayland` / `GTK3`):

- **Ubuntu / Debian:**
  ```bash
  sudo apt update
  sudo apt install -y build-essential cmake pkg-config \
    libasound2-dev libxcb-render0-dev libxcb-shape0-dev \
    libxcb-xfixes0-dev libxkbcommon-dev libssl-dev \
    libgtk-3-dev zenity xclip wl-clipboard ffmpeg
  ```
- **Fedora:**
  ```bash
  sudo dnf install -y gcc gcc-c++ make cmake pkgconf-pkg-config \
    alsa-lib-devel libxcb-devel libxkbcommon-devel openssl-devel \
    gtk3-devel zenity xclip wl-clipboard ffmpeg
  ```
- **Arch Linux:**
  ```bash
  sudo pacman -S --needed base-devel cmake pkgconf alsa-lib \
    libxcb libxkbcommon openssl gtk3 zenity xclip wl-clipboard ffmpeg
  ```

Biên dịch:
```bash
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite
CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo build --release
```
Binary đầu ra: `target/release/zalo-rust`

### 4. macOS (Apple Silicon & Intel)
```bash
xcode-select --install
brew install cmake pkg-config opus ffmpeg
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite
cargo build --release
```
Binary đầu ra: `target/release/zalo-rust`

---

## Sử dụng

### Chạy giao diện đồ họa (GUI - mặc định)
```bash
./target/release/zalo-rust --gui
```
- Quét mã QR bằng ứng dụng Zalo trên điện thoại và xác nhận đăng nhập.
- Khi gõ các từ/cụm từ cảm xúc trong ô chat (`xin chào`, `cảm ơn`, `khóc`, `nhớ`, `cố lên`, `mệt quá`, `đi chơi`...), thanh gợi ý nhãn dán sẽ tự động hiển thị phía trên ô nhập. Có thể bấm chuột hoặc dùng phím `↑` `←` `→` `Enter` để chọn và gửi nhanh, hoặc nhấn `Esc` để đóng.

### Chạy giao diện dòng lệnh (TUI)
```bash
./target/release/zalo-rust --tui --qr
```
- Điều hướng bằng `↑`/`↓` (hoặc `j`/`k`), nhấn `Enter` để gửi tin nhắn, `Tab` để đổi vùng chọn, `r` để làm mới danh sách và `q` để thoát.

### Chế độ chẩn đoán giao thức (Dev Inspector)
```bash
./target/release/zalo-rust --gui --dev
```
- Hiển thị thêm bảng kiểm tra request/response, thời gian phản hồi, payload đã giải mã và trạng thái khóa phiên.

### Đăng xuất và xóa phiên
```bash
./target/release/zalo-rust --logout
```

---

## Đóng gói đa nền tảng (CI/CD)

Workflow GitHub Actions tại [`.github/workflows/ci-release.yml`](.github/workflows/ci-release.yml) tự động chạy `cargo test`, đóng gói và phát hành lên [GitHub Releases](https://github.com/ctdoteam1110/ctdozlite/releases) khi đẩy tag `v*`:

- **Linux (5 gói tương thích đa distro):**
  - `ctdozlite_0.0.1_amd64.deb` (Debian 12+ / Ubuntu 22.04, 24.04+ / Mint 21+)
  - `ctdozlite_0.0.1_legacy_glibc2.31_amd64.deb` (Debian 11 / Ubuntu 20.04+ / Mint 20+)
  - `ctdozlite-0.0.1-1.x86_64.rpm` (Fedora / RHEL / Rocky / AlmaLinux / openSUSE)
  - `ctdozlite-0.0.1-x86_64.AppImage` (Gói thực thi độc lập chạy trên mọi bản Linux)
  - `ctdozlite-0.0.1-linux-x86_64.tar.gz` & `ctdozlite-0.0.1-linux-x86_64-legacy-glibc2.31.tar.gz` (kèm script `install.sh` / `uninstall.sh`)
- **Windows:** `ctdozlite-0.0.1-portable-windows-x86_64.exe` và `ctdozlite-0.0.1-setup-windows-x86_64.exe` ([`packaging/windows/installer.iss`](packaging/windows/installer.iss))
- **macOS:** `ctdozlite-0.0.1-macos-arm64.dmg` / `ctdozlite-0.0.1-macos-x86_64.dmg` đóng gói `ctdozlite.app` ([`packaging/macos/Info.plist`](packaging/macos/Info.plist))

---

## Phiên bản hiện tại & Quy tắc cập nhật (Versioning)

- **Phiên bản hiện tại:** `0.0.1` (`v0.0.1`)
- **Quy tắc tăng phiên bản cho các bản cập nhật tiếp theo:**
  Khi phát hành bản cập nhật mới, tăng tuần tự số phiên bản (`0.0.1` -> `0.0.2` -> `0.0.3`...) đồng bộ tại 5 vị trí:
  1. `Cargo.toml` (`version = "0.0.x"`)
  2. `packaging/macos/Info.plist` (`CFBundleShortVersionString`)
  3. `packaging/windows/installer.iss` (`#define MyAppVersion`)
  4. `.github/workflows/ci-release.yml` (tên file đóng gói và tag release)
  5. `README.md` (badge `release-v0.0.x` và mục Lịch sử phiên bản dưới đây), sau đó tạo git tag `v0.0.x` và push lên `origin` để CI tự động build & đính kèm toàn bộ gói cài đặt vào trang Releases.

### Lịch sử phiên bản (Changelog)
- **`v0.0.1` (2026-10-31):**
  - Bản phát hành đầu tiên (`v0.0.1`): hoàn thiện giao diện GUI (`egui`, tách 7 module `src/gui/`) và TUI (`ratatui`).
  - Nâng cấp gợi ý & tìm kiếm sticker theo thời gian thực khi gõ (kết hợp song song `cmd 11741` + `cmd 11747`, hỗ trợ tiếng Việt có dấu/không dấu/viết tắt và prefetch thumbnail).
  - Hỗ trợ nhắn tin thời gian thực qua WebSocket (`zpw_ws`), lưu trữ cục bộ SQLite + JSON, gọi thoại & gọi video 1-1 thuần Rust.
  - Phát hành bộ cài đặt đa nền tảng (`.deb` chuẩn, `.deb` legacy `glibc 2.31`, `.rpm`, `.AppImage`, `.tar.gz`, `.exe`, `.dmg`).

---

## Tài liệu chi tiết

- [docs/HUONG_DAN_SU_DUNG.md](docs/HUONG_DAN_SU_DUNG.md): Các cờ CLI, phím tắt GUI/TUI và cách dùng các tính năng trong app.
- [docs/KIEN_TRUC_HE_THONG.md](docs/KIEN_TRUC_HE_THONG.md): Thiết kế luồng giao diện, luồng worker bất đồng bộ và lưu trữ SQLite + JSON.
- [docs/GIAO_THUC_ZALO.md](docs/GIAO_THUC_ZALO.md): Ghi chú về `ParamCipher v2`, `PasswordCipher`, `signkey` MD5 và bảng domain.
- [docs/KHAC_PHUC_SU_CO.md](docs/KHAC_PHUC_SU_CO.md): Giải thích các mã lỗi máy chủ Zalo thường gặp và cách xử lý.

---

## Lưu ý bảo mật

- `session.json` chứa cookie `zpw_sek`, khóa giải mã `zpw_enk` và định danh thiết bị của phiên đang đăng nhập. Không chia sẻ hoặc commit file này lên kho mã nguồn.
- Các tệp dữ liệu cục bộ (`session.json`, `zalo_history.db`, `threads_cache.json`, `stickers_cache/`, `*.log`) đã được cấu hình loại trừ trong `.gitignore`.

---

## Giấy phép

Phân phối dưới giấy phép kép **[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)**. Dự án phục vụ mục đích học tập và nghiên cứu kỹ thuật hệ thống với Rust.
