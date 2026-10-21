# ctdozlite 🚀

> **Client Zalo PC Native siêu nhẹ bằng Rust — Zero Node.js, Zero Electron, tiêu thụ ~10MB RAM.**

[![Build](https://img.shields.io/badge/build-passing-brightgreen.svg)]()
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

## 🚀 Cài Đặt & Biên Dịch

### Yêu cầu
- Rust toolchain 1.75 trở lên: [https://rustup.rs/](https://rustup.rs/)

### Biên dịch bản Release
```bash
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite
cargo build --release
```
File thực thi độc lập sẽ nằm tại `target/release/zalo-rust.exe` (Windows) hoặc `target/release/zalo-rust` (Linux/macOS).

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

### 4. Đăng xuất tài khoản
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

---

## 🔒 Bảo Mật & Lưu Ý

- File `session.json` chứa token phiên xác thực và khóa giải mã cá nhân. **Tuyệt đối không chia sẻ file này hoặc commit lên Git**.
- Dự án đã tích hợp sẵn `.gitignore` để tự động loại trừ mọi file cấu hình, khóa phiên và database cục bộ.

---

## 📜 Giấy Phép (License)

Dự án được phân phối dưới giấy phép **MIT OR Apache-2.0**.
Mục đích phục vụ nghiên cứu kỹ thuật đảo ngược, tối ưu hóa hiệu năng và học tập kiến trúc hệ thống Rust.
