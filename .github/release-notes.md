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

![Tải tệp trong QTzalo](https://raw.githubusercontent.com/quangthoai1985/QTzalo/v{{VERSION}}/docs/assets/qtzalo-download.png)

- **Tải tệp, video và ảnh gốc về máy** vào "Thư mục lưu file" (Cài đặt): nút tải trên từng thẻ tệp/video/ảnh và trong trình xem ảnh, có tiến trình phần trăm, nút **Mở** / **Mở thư mục** khi xong và **Thử lại** khi lỗi.
- **An toàn khi tải:** chỉ qua `https`, làm sạch tên file, không ghi đè file có sẵn, gắn Mark-of-the-Web trên Windows, không tự mở file thực thi (`.exe`, `.bat`, `.ps1`, `.lnk`…).
- **Quản lý cache ảnh** trong *Cài đặt → Dữ liệu & làm mới*: xem dung lượng, xoá cache ảnh (không đụng tới đăng nhập, lịch sử và cài đặt), mở thư mục dữ liệu.
- **Số thành viên nhóm** hiện ở header chat khi máy chủ Zalo cung cấp.
- Chọn "Thư mục lưu file" bằng hộp chọn thư mục của hệ điều hành; ảnh dán từ clipboard không còn rơi vào thư mục đang chạy app.
- Nâng `ratatui`/`crossterm`, loại bỏ cảnh báo `lru` unsound.

Xem đầy đủ: [CHANGELOG.md](https://github.com/quangthoai1985/QTzalo/blob/v{{VERSION}}/CHANGELOG.md).

### Lưu ý

- Dữ liệu người dùng nằm ở `%APPDATA%\QTzalo` (Windows) / `~/.local/share/qtzalo` (Linux). Lần đầu chạy, dữ liệu cũ trong thư mục làm việc được tự động chuyển vào đây.
- QTzalo không liên kết với VNG / Zalo. Dùng client bên thứ ba có thể vi phạm điều khoản dịch vụ của Zalo.
