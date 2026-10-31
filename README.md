# ctdozlite 🚀

> **Client Zalo PC Native siêu nhẹ bằng Rust — Zero Node.js, Zero Electron, tiêu thụ ~10MB RAM.**

[![CI & Release](https://github.com/ctdoteam1110/ctdozlite/actions/workflows/ci-release.yml/badge.svg)](https://github.com/ctdoteam1110/ctdozlite/actions/workflows/ci-release.yml)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)]()
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue.svg)]()
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)]()

**ctdozlite** là bản tái hiện độc lập (clean-room reverse engineered implementation) của Zalo Desktop Client được viết hoàn toàn bằng **Rust**. Ứng dụng loại bỏ toàn bộ gánh nặng của Chromium/Electron, khởi động tức thì dưới **5 mili-giây** và tiêu tốn chỉ **~10MB RAM** (so với 350MB – 600MB của Zalo chính thức).

---

## ✨ Tính Năng Nổi Bật

- ⚡ **Siêu nhẹ & Hiệu năng cao:** Viết bằng Rust 100%, không Node.js, không Electron, không Chromium.
- 📱 **Đăng nhập QR & Mật khẩu chuẩn:**
  - Hỗ trợ đầy đủ luồng quét mã QR (`reqqr`, long-polling `408`, mobile confirmation `18012`).
  - Hỗ trợ thuật toán mật khẩu Zalo `PasswordCipher` + `ParamCipher v2`.
  - Tự động lưu và phục hồi phiên (`session.json`), tự động nhận diện và làm mới khi phiên hết hạn.
- 💬 **Hệ thống Chat & Nhóm hoàn chỉnh:**
  - Nhận và gửi tin nhắn cá nhân (1-1) và nhóm chat (Group chat) hai chiều tức thì.
  - Tự động nhận diện tin nhắn từ **người lạ**, tra cứu hồ sơ tên hiển thị & avatar thật từ server Zalo qua `getminiprofiles`.
  - Phân tích đa dạng loại nội dung: văn bản, `[Nhãn dán]`, `[Hình ảnh]`, `[Tin nhắn thoại]`, `[Video]`, `[Tập tin] {tên_file}`, `[Vị trí]`, `[Tin nhắn đã thu hồi]`.
- 🗄️ **Lưu trữ Cục bộ 2 Tầng (Two-tier Storage):**
  - **Snapshot JSON (`threads_cache.json`):** Mở app là có ngay tin nhắn trong 5ms.
  - **SQLite Database (`zalo_history.db`):** Lưu trữ tin nhắn tích lũy vĩnh viễn, chống trùng lặp thông minh theo `msg_id` và timestamp.
- 🖥️ **Đa giao diện (Dual Interface):**
  - **Native GUI (`eframe`/`egui`):** Giao diện đồ họa hiện đại, font Segoe UI tiếng Việt sắc nét, Dark/Light mode tự động, bong bóng chat co giãn thông minh, giữ focus liên tục khi nhấn Enter.
  - **Terminal TUI (`ratatui`/`crossterm`):** Giao diện dòng lệnh 60fps, khung viền phẳng ANSI chống xô lệch, phím tắt điều hướng nhanh nhẹn.
- 📞 **Gọi thoại & Gọi video 1-1 (Pure-Rust VoIP):**
  - Báo hiệu cuộc gọi (`401` / `416` / `407` / `402` / `408` / `409`) và khởi tạo phiên relay ZRTP (`InitZRTP` `0x000B`/`0x000C`, `Echo` `0x0005`, `Ping` `0x0002`, `IncallNegotiateInfo` `0x0020/0x14`) viết 100% bằng Rust, **không cần `ZaloCall.exe`**.
  - Media RTP/RTCP hai chiều: audio Opus (`cpal` + `opus`), video H.264 theo định dạng khung của Zalo (`0x03`/`0x04` Audio RTP, `0x05` Audio RTCP, `0x0D`/`0x0E` Video RTP PT 97/98, `0x0F` Video RTCP SR/SDES/PLI/REMB).
  - Cửa sổ cuộc gọi riêng biệt, hỗ trợ chọn thiết bị đầu vào **Mic** và **Camera**, thu hình webcam thật + xem trước PIP và giải mã video H.264 thời gian thực qua `ffmpeg`.
  - Nhật ký chẩn đoán `voip.log` dạng văn bản thuần (tag `CALL`, `SIG`, `RTP>/<`, `RTCP>/<`, `STAT`, `VDEC`, `VCAM`, `TXST`).
- 🔍 **Dev Inspector & Protocol Scanner:** Tích hợp bộ chẩn đoán bắt gói tin, đo độ trễ mạng, giải mã payload và tự động quét phát hiện mã lỗi giao thức (`--dev`). Ẩn 100% trong chế độ Production.

---

## 📊 So Sánh Hiệu Năng

| Chỉ số | Zalo PC (Electron) | ctdozlite (Rust) | Mức cải thiện |
|---|:---:|:---:|:---:|
| **RAM tiêu thụ (Idle)** | 350 MB – 550 MB | **~10.5 MB** | **Giảm ~97%** |
| **Kích thước bộ cài** | ~180 MB | **~11 MB** (File đơn exe duy nhất) | **Nhỏ hơn 16 lần** |
| **Thời gian khởi động** | 2.5s – 4.0s | **< 0.05s** (Tức thì) | **Nhanh hơn 50 lần** |
| **Phụ thuộc môi trường** | Node, Electron, Chromium | **Không (Standalone Binary)** | Độc lập 100% |

---

## 🚀 Cài Đặt & Biên Dịch (Windows / Linux / macOS)

### 1. Yêu cầu chung
- **Rust toolchain** (1.75 trở lên): [https://rustup.rs/](https://rustup.rs/)
- **CMake** (3.15+) & trình biên dịch C/C++ (để crate `opus` biên dịch tĩnh `libopus`, và `rusqlite` biên dịch tĩnh SQLite).
- **FFmpeg** trong `PATH` (tùy chọn — dùng để thu hình webcam và giải mã video H.264 khi gọi video; chat và gọi thoại không cần `ffmpeg`).

> **Lưu ý cho CMake 4.0+:** Nếu máy cài CMake 4.x báo lỗi tương thích với `CMakeLists.txt` của `opus-sys`, hãy đặt biến môi trường `CMAKE_POLICY_VERSION_MINIMUM=3.5` trước khi chạy `cargo build`.

---

### 2. Biên dịch trên Windows (MSVC)

1. Cài đặt **Rust** (`rustup-init.exe`) và **Visual Studio Build Tools** (chọn workload *Desktop development with C++* + *CMake tools for Windows*).
2. Cài đặt `ffmpeg` (qua Winget hoặc Scoop):
   ```powershell
   winget install Gyan.FFmpeg
   ```
3. Clone và biên dịch bản Release:
   ```powershell
   git clone https://github.com/ctdoteam1110/ctdozlite.git
   cd ctdozlite
   $env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"
   cargo build --release
   ```
4. File thực thi độc lập nằm tại: `target\release\zalo-rust.exe`

---

### 3. Biên dịch trên Linux (Ubuntu / Debian / Fedora / Arch)

1. Cài đặt các thư viện phát triển hệ thống cho âm thanh (`ALSA`) và cửa sổ đồ họa (`X11` / `Wayland` / `OpenGL`):
   - **Ubuntu / Debian:**
     ```bash
     sudo apt update
     sudo apt install -y build-essential cmake pkg-config \
       libasound2-dev libxcb-render0-dev libxcb-shape0-dev \
       libxcb-xfixes0-dev libxkbcommon-dev libssl-dev \
       libgtk-3-dev zenity xclip wl-clipboard ffmpeg
     ```
   - **Fedora / RHEL:**
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
2. Clone và biên dịch bản Release:
   ```bash
   git clone https://github.com/ctdoteam1110/ctdozlite.git
   cd ctdozlite
   CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo build --release
   ```
3. File thực thi độc lập nằm tại: `target/release/zalo-rust`

---

### 4. Biên dịch trên macOS (Apple Silicon & Intel)

1. Cài đặt **Xcode Command Line Tools**, **CMake** và **FFmpeg** (qua Homebrew):
   ```bash
   xcode-select --install
   brew install cmake pkg-config ffmpeg
   ```
2. Clone và biên dịch bản Release:
   ```bash
   git clone https://github.com/ctdoteam1110/ctdozlite.git
   cd ctdozlite
   CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo build --release
   ```
3. File thực thi độc lập nằm tại: `target/release/zalo-rust`

---

### 5. CI/CD Tự Động Đóng Gói Đa Nền Tảng (`.deb` / `.exe` / `.dmg`)

Dự án tích hợp sẵn GitHub Actions tại [`.github/workflows/ci-release.yml`](.github/workflows/ci-release.yml), tự động chạy `cargo test` và đóng gói bộ cài gốc cho cả 3 hệ điều hành trên mỗi lần `push`, `pull_request`, hoặc khi gắn tag phát hành:

| Hệ điều hành | Sản phẩm đóng gói tự động | Cấu hình nguồn |
|---|---|---|
| **Linux (Debian / Ubuntu / Mint)** | `ctdozlite_<ver>_amd64.deb` (tích hợp `/usr/bin/zalo-rust`, symlink `ctdozlite`, biểu tượng & menu `.desktop`) | [`packaging/linux/ctdozlite.desktop`](packaging/linux/ctdozlite.desktop) |
| **Windows (x64)** | `ctdozlite-portable-windows-x86_64.exe` (file đơn) + `ctdozlite-setup-windows-x86_64.exe` (bộ cài Inno Setup) | [`packaging/windows/installer.iss`](packaging/windows/installer.iss) |
| **macOS (Apple Silicon / Intel)** | `ctdozlite_<ver>_macos.dmg` (chứa `ctdozlite.app` đã gắn `AppIcon.icns`, quyền Mic/Camera & liên kết kéo-thả `/Applications`) | [`packaging/macos/Info.plist`](packaging/macos/Info.plist) |

**Cách phát hành phiên bản mới lên GitHub Releases (kèm bảng băm `SHA256SUMS.txt`):**
```bash
git tag v0.1.0
git push origin v0.1.0
```
Hoặc vào tab **Actions -> CI & Cross-Platform Release (.deb / .exe / .dmg) -> Run workflow** để tải trực tiếp các gói `.deb`, `.exe`, `.dmg` từ mục **Artifacts**.

---

## 📖 Hướng Dẫn Sử Dụng

### 1. Khởi chạy Giao diện Đồ họa (GUI - Khuyên dùng)
```bash
./target/release/zalo-rust --gui
```
- Màn hình sẽ hiện mã QR lớn có viền trắng tương phản cao.
- Mở Zalo trên điện thoại -> Quét mã QR -> Bấm **"Đăng nhập"**.
- Trò chuyện trực tiếp trên giao diện chat 2 cột.

### 2. Khởi chạy Giao diện Dòng lệnh (TUI)
```bash
./target/release/zalo-rust --tui --qr
```
- Dùng các phím `↑`/`↓` hoặc `j`/`k` để chọn cuộc trò chuyện.
- Gõ bất kỳ chữ cái nào để soạn tin nhắn, bấm `Enter` để gửi tin.
- Bấm `Tab` để đổi focus, bấm `r` để làm mới, bấm `q` để thoát.

### 3. Bật chế độ Dev Inspector (Chẩn đoán & Quét log)
```bash
./target/release/zalo-rust --gui --dev
```
- Bấm vào thanh `▲ Mở Dev Inspector` ở chân màn hình để xem toàn bộ luồng mạng và payload giải mã.

### 4. Gọi thoại / Gọi video (Pure-Rust VoIP)

Yêu cầu:
- **Hoạt động độc lập 100% bằng Rust** (không cần cài Zalo PC hay `ZaloCall.exe`).
- **`ffmpeg`** có trong `PATH` (cần cho thu hình webcam và giải mã video H.264; gọi thoại chỉ dùng `cpal` + `opus` nên không cần `ffmpeg`).

Cách dùng:
- Mở một cuộc trò chuyện 1-1 trong GUI và bấm biểu tượng **Gọi thoại** hoặc **Gọi video** ở góc phải thanh tiêu đề.
- Khi có cuộc gọi đến, cửa sổ cuộc gọi sẽ tự động bật lên với 2 nút **Nghe máy** (xanh) và **Từ chối** (đỏ).
- Có thể chọn thiết bị đầu vào **Mic** và **Camera** ngay trên thanh công cụ đầu cửa sổ cuộc gọi hoặc trong **Cài đặt -> Cài đặt chung -> Thiết bị Âm thanh & Video**.

Lưu ý:
- Hiện hỗ trợ gọi **1-1** (thoại & video); gọi nhóm bị khóa.
- Gọi đi được bảo vệ bởi danh sách cho phép thử nghiệm (`CALL_ALLOWED_NAMES` / `CALL_ALLOWED_IDS` trong `src/call.rs`). Ghi đè ID lúc chạy bằng biến môi trường `ZALO_CALL_ALLOW_IDS`.
- App tự động đàm phán H.264 (`supportHevcDecode=0`), gửi `RTCP REMB` và `H264DecCapacityInfo(720p, 30fps)` để tối ưu chất lượng hình ảnh nhận về.
Nhật ký cuộc gọi:

| Biến môi trường | Tác dụng |
|---|---|
| `ZALO_VOIP_LOG=<đường dẫn>` | Ghi `voip.log` vào đường dẫn khác (mặc định `./voip.log`, xoay vòng ở 2 MiB) |
| `ZALO_VOIP_LOG=off` | Tắt hoàn toàn `voip.log` |

`voip.log` và `call_trace.log` là văn bản thuần, chứa địa chỉ relay và `extendData` của cuộc gọi. Cả hai đã nằm trong `.gitignore`, **không chia sẻ hoặc commit**.

### 5. Đăng xuất tài khoản
- Nhấn nút màu đỏ **`🚪 Đăng xuất`** trên thanh tiêu đề GUI, hoặc bấm phím **`x`** trong TUI, hoặc chạy:
  ```bash
  ./target/release/zalo-rust --logout
  ```

---

## 📚 Tài Liệu Kỹ Thuật

Toàn bộ tài liệu chi tiết được lưu trong thư mục `docs/`:

- **[docs/HUONG_DAN_SU_DUNG.md](docs/HUONG_DAN_SU_DUNG.md)**: Chi tiết cách dùng GUI, TUI, bảng phím tắt, các cờ CLI.
- **[docs/KIEN_TRUC_HE_THONG.md](docs/KIEN_TRUC_HE_THONG.md)**: Kiến trúc đa luồng Tokio non-blocking, cơ chế lưu trữ 2 tầng SQLite + JSON.
- **[docs/GIAO_THUC_ZALO.md](docs/GIAO_THUC_ZALO.md)**: Đặc tả giao thức Zalo PC, ParamCipher v2, PasswordCipher, signkey MD5, bảng mã hóa AES-128/256.
- **[docs/KHAC_PHUC_SU_CO.md](docs/KHAC_PHUC_SU_CO.md)**: Bảng tra cứu mã lỗi giao thức và cách khắc phục sự cố.

Ghi chú reverse engineering về cuộc gọi (thư mục `re-notes/`):

- **[re-notes/06_call_media.md](re-notes/06_call_media.md)**: Luồng media cuộc gọi (ZRTP, relay, RTP audio).
- **[re-notes/07_voip_log.md](re-notes/07_voip_log.md)**: Giải mã `voip.log` của `ZaloCall.exe` và máy trạng thái native.
- **[re-notes/08_video_call.md](re-notes/08_video_call.md)**: Cơ chế gọi video: định dạng H.264 của Zalo, RTCP/PLI, kết quả các lần gọi thử và các việc còn mở.

---

## 🔒 Bảo Mật & Lưu Ý

- File `session.json` chứa token phiên xác thực và khóa giải mã cá nhân. **Tuyệt đối không chia sẻ file này hoặc commit lên Git**.
- Dự án đã tích hợp sẵn `.gitignore` để tự động loại trừ mọi file cấu hình, khóa phiên, database cục bộ và nhật ký cuộc gọi (`*.log`).
- Trước khi push, kiểm tra `git status` và không đưa `session.json`, `voip.log`, `call_trace.log`, `*.db`, `dump/` lên Git.

---

## 📜 Giấy Phép (License)

Dự án được phân phối dưới giấy phép **MIT OR Apache-2.0**.
Mục đích phục vụ nghiên cứu kỹ thuật đảo ngược, tối ưu hóa hiệu năng và học tập kiến trúc hệ thống Rust.
