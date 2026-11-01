# Nhật ký thay đổi

Mọi thay đổi đáng chú ý của QTzalo được ghi tại đây. Định dạng theo [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), phiên bản theo [Semantic Versioning](https://semver.org/lang/vi/).

QTzalo là bản fork của [ctdozlite](https://github.com/ctdoteam1110/ctdozlite), tách ra từ commit `73849bd` (tag `v0.0.2` của upstream).

## [0.1.0] - 2026-11-01

Phiên bản đầu tiên của QTzalo.

### Bảo mật

<!-- Điền khi tích hợp: lưu trữ dữ liệu an toàn, tắt log mặc định, link preview, RNG, WebSocket, mật khẩu, dependency. -->

### Sửa lỗi

<!-- Điền khi tích hợp: #21, #22, #4, #5/#6, #23, khoá gọi thử nghiệm. -->

### Giao diện

<!-- Điền khi tích hợp: bố cục 3 cột mới, thu gọn cột thành biểu tượng, theme sáng. -->

### Build & phát hành

- Workflow CI mới (`.github/workflows/ci-release.yml`):
  - Lấy phiên bản từ `Cargo.toml` cho mọi tên file và metadata; tag phải khớp phiên bản.
  - Phát hành bằng nút *Run workflow* (tick `release`) hoặc push tag `v<phiên bản>`.
  - Quyền token mặc định chỉ đọc; chỉ job phát hành có quyền ghi.
  - Mọi action bên thứ ba được ghim theo commit SHA.
  - Bỏ bản AppImage, vì `appimagetool` không có bản phát hành cố định để kiểm tra checksum.
  - Release chỉ được tạo khi test và bản Windows build thành công, kèm `SHA256SUMS.txt`.
- Bộ cài Windows (Inno Setup) đổi thương hiệu thành **QTzalo**:
  - `AppId` mới (không đụng bản ctdozlite đã cài) và cài file chạy thành `QTzalo.exe`.
  - Không cần quyền Administrator; yêu cầu Windows 10 trở lên.
  - Khi gỡ chỉ xoá dữ liệu người dùng nếu được đồng ý.
- Bản Windows link tĩnh C runtime (`+crt-static`) nên chạy được trên máy chưa cài Visual C++ Redistributable.
- `build.rs` nhúng icon theo **máy đích** (`CARGO_CFG_TARGET_OS`) thay vì máy build; hỗ trợ MinGW qua `windres`.
- Thêm `scripts/check-windows.sh` để kiểm tra mã dành riêng cho Windows ngay trên Linux.
- Gói Linux đổi tên thành `qtzalo` (`.deb`, `.tar.gz`). `install.sh` hỗ trợ `--user`, `uninstall.sh` hỗ trợ `--purge`.
- Xoá thư mục `web-bundle/` (JavaScript đã làm rối của Zalo, không dùng khi build).

### Tài liệu

- Viết lại `README.md` cho QTzalo: yêu cầu **Rust ≥ 1.88** (đã kiểm chứng 1.87 không build được), vị trí lưu dữ liệu, quy trình phát hành, ghi công dự án gốc.
- Thêm `docs/CAI_DAT.md` (hướng dẫn cài đặt, xử lý SmartScreen), `docs/SECURITY_AUDIT.md` (kết quả rà soát), `SECURITY.md`.
- Bổ sung `docs/KHAC_PHUC_SU_CO.md`: font, phiên đăng nhập, giờ hiển thị.

[0.1.0]: https://github.com/quangthoai1985/QTzalo/releases/tag/v0.1.0
