# Hướng Dẫn Sử Dụng ctdozlite (Zalo Native Client)

Tài liệu này cung cấp hướng dẫn toàn diện từ việc thiết lập, đăng nhập, sử dụng các giao diện và các tùy chọn dòng lệnh của **ctdozlite**.

---

## 1. Các chế độ hoạt động

Ứng dụng hỗ trợ 2 giao diện người dùng độc lập trong cùng một file nhị phân (`zalo-rust.exe`):

1. **Native GUI (Giao diện đồ họa - Khuyên dùng)**: Xây dựng trên nền tảng `eframe` và `egui` với backend đồ họa phần cứng Glow/OpenGL. Mượt mà, hiển thị font Segoe UI tiếng Việt chuẩn, hỗ trợ Dark/Light mode tự động.
2. **Terminal TUI (Giao diện dòng lệnh)**: Xây dựng trên nền tảng `ratatui` và `crossterm`. Dành cho máy chủ, môi trường SSH hoặc người dùng ưa thích terminal. Khung viền ANSI phẳng chống xô lệch ký tự.

---

## 2. Các tham số dòng lệnh (CLI Flags)

| Tham số | Ý nghĩa | Ví dụ |
|---|---|---|
| `--gui` | Khởi chạy chế độ giao diện đồ họa GUI (mặc định nếu không truyền cờ khác). | `zalo-rust --gui` |
| `--tui` | Khởi chạy chế độ dòng lệnh TUI. | `zalo-rust --tui` |
| `--qr` | Yêu cầu đăng nhập bằng mã QR trong terminal ngay khi mở. | `zalo-rust --tui --qr` |
| `--dev` | Bật chế độ Dev Inspector (hiển thị lưu lượng mạng, payload giải mã và máy quét lỗi). | `zalo-rust --gui --dev` |
| `--logout` | Xóa phiên đăng nhập lưu trữ (`session.json`) và thoát sạch sẽ. | `zalo-rust --logout` |
| `--phone <SỐ>` | Điền sẵn số điện thoại khi đăng nhập bằng mật khẩu. | `zalo-rust --tui --phone 0912345678` |
| `--pass <MK>` | Điền sẵn mật khẩu khi đăng nhập bằng mật khẩu. | `zalo-rust --tui --phone 0912345678 --pass MatKhau` |
| `--dump` | Ghi toàn bộ request/response ra thư mục `dump/` dạng JSON để phân tích. | `zalo-rust --dump` |

---

## 3. Hướng dẫn sử dụng Giao diện Đồ họa (GUI)

### 3.1. Đăng nhập bằng mã QR
1. Khởi chạy: `.\target\release\zalo-rust.exe --gui`
2. Màn hình đăng nhập sẽ hiển thị một mã QR kích thước lớn `280x280 px` với khung viền trắng rõ nét.
3. Mở ứng dụng Zalo trên điện thoại di động -> chọn biểu tượng quét mã QR ở góc trên.
4. Quét mã QR trên màn hình máy tính -> bấm nút **"Đăng nhập"** trên điện thoại.
5. Ứng dụng tự động lưu phiên vào `session.json`, tải danh bạ, các nhóm và mở ngay màn hình trò chuyện.
6. Lần khởi động tiếp theo, ứng dụng tự động kiểm tra phiên và vào thẳng màn hình chat mà không cần quét lại.

### 3.2. Màn hình trò chuyện (Chat)
- **Cột bên trái:**
  - Ô `🔍 Tìm kiếm bạn bè…`: Gõ tên người dùng để lọc danh sách liên hệ tức thì.
  - Nút `🔄`: Làm mới danh bạ từ máy chủ Zalo.
  - Danh sách hội thoại: Hiển thị avatar tròn (chữ cái đầu), tên liên hệ, thời gian và dòng trích đoạn tin nhắn mới nhất. Nhấp chuột vào bất kỳ hội thoại nào để mở cuộc trò chuyện.
- **Cột bên phải:**
  - Tiêu đề: Tên hội thoại, trạng thái hoạt động, ID và nút `📜 Tải lịch sử nhóm` (nếu là nhóm chat).
  - Vùng tin nhắn:
    - Tin nhắn gửi đi (`Tôi`): Bong bóng màu xanh Zalo `#0068FF` căn bên phải.
    - Tin nhắn nhận về: Bong bóng màu xám tối căn bên trái, tương phản sắc nét.
    - Các nhãn đặc biệt: `[Nhãn dán]`, `[Hình ảnh]`, `[Tin nhắn thoại]`, `[Tập tin]`, `[Video]`.
  - Ô soạn tin nhắn:
    - Nhập nội dung và nhấn **`Enter`** (hoặc bấm nút **`Gửi`**).
    - Sau khi gửi, con trỏ chuột được giữ nguyên trong ô nhập (`auto-focus`), cho phép gõ tin nhắn liên tục không cần chạm chuột.

### 3.3. Đăng xuất tài khoản
- Nhấn vào nút màu đỏ **`🚪 Đăng xuất`** ở góc trên bên phải.
- Ứng dụng lập tức hủy các tác vụ chạy ngầm, xóa token trong bộ nhớ, xóa file `session.json` và `threads_cache.json`, đồng thời tự động tạo ngay một mã QR mới để quét tài khoản khác.

### 3.4. Sử dụng Dev Inspector (Khi bật `--dev`)
- Bấm vào thanh **`▲ Mở Dev Inspector`** ở đáy màn hình:
  - **Tab Logs:** Xem danh sách mọi yêu cầu HTTP/HTTPS, độ trễ ms, mã HTTP, URL và toàn bộ payload JSON đã giải mã.
  - **Tab Scanner:** Công cụ tự động phân tích phát hiện các mã lỗi giao thức (`18060`, `18022`, `600`, `102`, `114`, `429`).
  - **Tab CryptoState:** Bảng thông số kỹ thuật mật mã đang áp dụng trong phiên.

---

## 4. Hướng dẫn sử dụng Giao diện Dòng lệnh (TUI)

### 4.1. Khởi chạy TUI
```powershell
.\target\release\zalo-rust.exe --tui --qr
```

### 4.2. Bảng phím tắt điều khiển TUI

| Phím tắt | Chức năng |
|:---:|---|
| **`↑` / `↓`** hoặc **`k` / `j`** | Di chuyển lên/xuống để chọn hội thoại trong danh sách bên trái. |
| **`Enter`** | Gửi tin nhắn khi đang ở ô soạn thảo / Chuyển tiêu điểm vào ô soạn thảo khi ở danh sách. |
| **`Gõ phím bất kỳ`** | Tự động chuyển focus xuống ô soạn thảo và điền ký tự vào ngay lập tức. |
| **`Esc`** | Thoát focus ô soạn thảo, quay về danh sách hội thoại. |
| **`Tab`** | Chuyển đổi qua lại giữa cột danh sách và ô soạn thảo. |
| **`r`** | Làm mới danh bạ và đồng bộ tin nhắn mới nhất. |
| **`h`** | Tải lịch sử tin nhắn của nhóm chat đang chọn. |
| **`x`** | Đăng xuất tài khoản và xóa sạch phiên lưu trữ. |
| **`l`** | Mở cửa sổ popup xem Dev Logs & Protocol Scanner (chỉ hoạt động khi bật `--dev`). |
| **`q`** | Thoát ứng dụng an toàn và trả lại trạng thái terminal. |

---

## 5. Công cụ kiểm thử độc lập (`test_fetch`)

Dự án cung cấp một công cụ kiểm thử giao thức độc lập được biên dịch sẵn trong `target/release/test_fetch.exe`.

- **Kiểm tra phiên và in dữ liệu toàn bộ hội thoại:**
  ```powershell
  cargo run --release --bin test_fetch
  ```
- **Kiểm tra gửi tin nhắn cá nhân:**
  ```powershell
  cargo run --release --bin test_fetch -- --send <USER_ID> "Nội dung tin nhắn"
  ```
- **Kiểm tra gửi tin nhắn nhóm:**
  ```powershell
  cargo run --release --bin test_fetch -- --send-group <GROUP_ID> "Nội dung tin nhắn"
  ```
