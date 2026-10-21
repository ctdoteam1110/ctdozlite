# Tài Liệu Kỹ Thuật ctdozlite (Zalo Native Client in Rust)

Chào mừng bạn đến với bộ tài liệu kỹ thuật hoàn chỉnh của dự án **ctdozlite** (`zalo-rust`). Đây là client Zalo native chạy độc lập trên Windows/Linux/macOS, được viết hoàn toàn bằng Rust, loại bỏ 100% Electron và Node.js nhằm mang lại hiệu năng cao và mức tiêu thụ tài nguyên tối thiểu (~10MB RAM).

---

## 📚 Mục Lục Tài Liệu

| Tài liệu | Nội dung chính |
|---|---|
| **[1. Hướng Dẫn Sử Dụng](HUONG_DAN_SU_DUNG.md)** | Cài đặt, biên dịch, chạy giao diện đồ họa GUI (`egui`), chạy giao diện dòng lệnh TUI (`ratatui`), các phím tắt và tham số dòng lệnh CLI. |
| **[2. Kiến Trúc Hệ Thống](KIEN_TRUC_HE_THONG.md)** | Thiết kế kiến trúc đa luồng bất đồng bộ (Tokio runtime), cơ chế lưu trữ 2 tầng (SQLite + Snapshot JSON cache), quản lý kênh truyền mpsc. |
| **[3. Đặc Tả Giao Thức Zalo PC](GIAO_THUC_ZALO.md)** | Kết quả Reverse Engineering chi tiết: ParamCipher v2, PasswordCipher, SignKey MD5, bảng mã hóa AES-128/256, Long-polling và REST API. |
| **[4. Hướng Dẫn Khắc Phục Sự Cố](KHAC_PHUC_SU_CO.md)** | Phân tích và cách xử lý các mã lỗi máy chủ: `18012`, `18060`, `18022`, `600`, `102`, `429 Too Many Requests`, hiển thị font tiếng Việt. |

---

## ⚡ Bắt Đầu Nhanh (Quick Start)

### 1. Yêu cầu môi trường
- Rust Toolchain 1.75+ (khuyên dùng bản stable mới nhất): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Hệ điều hành: Windows 10/11, Linux (X11/Wayland), macOS.

### 2. Biên dịch bản Release
```bash
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite
cargo build --release
```

### 3. Khởi chạy
- **Chạy Giao diện Đồ họa (GUI):**
  ```bash
  ./target/release/zalo-rust --gui
  ```
- **Chạy Giao diện Dòng lệnh (TUI):**
  ```bash
  ./target/release/zalo-rust --tui --qr
  ```
- **Chạy với Dev Inspector (Bật nhật ký và quét giao thức):**
  ```bash
  ./target/release/zalo-rust --gui --dev
  ```
