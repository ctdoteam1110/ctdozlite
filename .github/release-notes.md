## QTzalo v{{VERSION}}

Ứng dụng Zalo desktop native viết bằng Rust, không chính thức. Bản build từ commit `{{COMMIT}}`.

### Tải về

| Hệ điều hành | File |
|---|---|
| **Windows 10/11 x64 (bộ cài)** | `QTzalo-{{VERSION}}-setup-windows-x86_64.exe` |
| Windows 10/11 x64 (portable) | `QTzalo-{{VERSION}}-portable-windows-x86_64.exe` |
| Ubuntu 22.04+ / Debian 12+ | `qtzalo_{{VERSION}}_amd64.deb` |
| Linux khác (glibc ≥ 2.35) | `qtzalo-{{VERSION}}-linux-x86_64.tar.gz` |
| macOS Apple Silicon (thử nghiệm, nếu có) | `QTzalo-{{VERSION}}-macos-arm64.dmg` |

Kiểm tra file bằng `SHA256SUMS.txt`. Bộ cài Windows chưa ký số: nếu SmartScreen cảnh báo, bấm **More info → Run anyway** sau khi đã kiểm tra SHA256. Hướng dẫn chi tiết: [docs/CAI_DAT.md](https://github.com/quangthoai1985/QTzalo/blob/v{{VERSION}}/docs/CAI_DAT.md).

### Điểm mới

![Giao diện QTzalo](https://raw.githubusercontent.com/quangthoai1985/QTzalo/v{{VERSION}}/docs/assets/qtzalo-ui.png)

- **Giao diện mới 3 cột:** tông sáng xanh nhạt, có giao diện tối. Thanh điều hướng thu gọn được thành dải icon, danh sách hội thoại thu gọn thành cột avatar.
- **Hiển thị đúng chữ Việt và chữ Hán** trên Windows, Linux (kể cả Fedora) và macOS (#5, #6).
- **Giờ tin nhắn theo múi giờ của máy**, không còn lệch 7 tiếng (#4).
- **Không mất đăng nhập khi rớt mạng** (#21). Đăng xuất không xoá lịch sử tin nhắn.
- **Danh sách bạn bè không còn lỗi 404** (#22).
- **Gọi được mọi liên hệ 1-1:** đã gỡ khoá thử nghiệm còn sót.
- **Bảo mật:**
  - Phiên đăng nhập và lịch sử lưu trong thư mục riêng của người dùng, chỉ chủ tài khoản đọc được.
  - Log chẩn đoán tắt mặc định.
  - Link trong tin nhắn đến không còn làm lộ IP.
  - Sinh số ngẫu nhiên bằng nguồn của hệ điều hành.
  - Cookie WebSocket chỉ gửi qua `wss://` tới Zalo.
  - Vá lỗ hổng tokio.

Xem đầy đủ: [CHANGELOG.md](https://github.com/quangthoai1985/QTzalo/blob/v{{VERSION}}/CHANGELOG.md).

### Lưu ý

- Dữ liệu người dùng nằm ở `%APPDATA%\QTzalo` (Windows) / `~/.local/share/qtzalo` (Linux). Lần đầu chạy, dữ liệu cũ trong thư mục làm việc được tự động chuyển vào đây.
- QTzalo không liên kết với VNG / Zalo. Dùng client bên thứ ba có thể vi phạm điều khoản dịch vụ của Zalo.
