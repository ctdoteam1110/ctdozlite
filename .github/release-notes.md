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

![Đồng bộ tin nhắn khi mở QTzalo](https://raw.githubusercontent.com/quangthoai1985/QTzalo/v{{VERSION}}/docs/assets/qtzalo-sync.png)

- **Tự bắt kịp tin nhắn khi mở app.** App nhớ vị trí đã đồng bộ của từng tài khoản, nên tắt máy một ngày rồi mở lại vẫn về đủ tin 1-1 và tin nhóm, chỉ lấy phần mới, có dải tiến trình "Đang đồng bộ tin nhắn… N tin". Khởi động lúc chưa có mạng thì tự thử lại.
- **Báo thiếu tin:** nếu máy chủ đã bỏ bớt tin trong lúc tắt máy, app hiện banner kèm khoảng thời gian có thể thiếu.
- **Danh sách hội thoại giống điện thoại:** chỉ hiện hội thoại có hoạt động, xếp theo thời điểm hoạt động cuối; ô tìm kiếm vẫn tìm được mọi bạn bè.
- **Ảnh, video, tệp không còn hiện link.** Bấm ảnh mở cửa sổ xem ảnh riêng; bấm video phát trong cửa sổ của QTzalo (cần `ffmpeg`, nếu không có thì mở bằng trình phát mặc định); bấm tệp thì tải về rồi mở. Không còn mở trình duyệt.
- **Sửa tên nhóm** hiện thành "Nhóm <số>"; sửa lỗi long-poll lặp liên tục khi có tin nhóm chờ.

![Ảnh, video, tệp trong tin nhắn](https://raw.githubusercontent.com/quangthoai1985/QTzalo/v{{VERSION}}/docs/assets/qtzalo-media-cards.png)

Xem đầy đủ: [CHANGELOG.md](https://github.com/quangthoai1985/QTzalo/blob/v{{VERSION}}/CHANGELOG.md).

### Lưu ý

- Dữ liệu người dùng nằm ở `%APPDATA%\QTzalo` (Windows) / `~/.local/share/qtzalo` (Linux). Lần đầu chạy, dữ liệu cũ trong thư mục làm việc được tự động chuyển vào đây.
- Trên một máy vừa đăng nhập lần đầu, máy chủ Zalo chỉ gửi tin từ sau lần đăng nhập đó; lịch sử cũ (nhất là của nhóm) chưa lấy được. Tính năng "đồng bộ từ điện thoại" đang được nghiên cứu.
- QTzalo không liên kết với VNG / Zalo. Dùng client bên thứ ba có thể vi phạm điều khoản dịch vụ của Zalo.
