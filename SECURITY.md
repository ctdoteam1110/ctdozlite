# Chính sách bảo mật

## Báo cáo lỗ hổng

Nếu phát hiện lỗ hổng bảo mật trong QTzalo, vui lòng **không** mở issue công khai. Hãy dùng tính năng
[Report a vulnerability](https://github.com/quangthoai1985/QTzalo/security/advisories/new) (GitHub private vulnerability reporting) của repo.

Khi báo cáo, vui lòng ghi rõ:

- Phiên bản QTzalo (`zalo-rust --version`) và hệ điều hành.
- Các bước tái hiện, mức độ ảnh hưởng.
- **Không** đính kèm `session.json`, cookie `zpw_sek`, khoá `zpw_enk`, log cuộc gọi hay dữ liệu tin nhắn thật. Hãy che chúng đi.

Chúng tôi sẽ phản hồi trong khả năng có thể. Đây là dự án cộng đồng, không có cam kết thời gian xử lý.

## Phiên bản được hỗ trợ

Chỉ phiên bản mới nhất trên trang [Releases](https://github.com/quangthoai1985/QTzalo/releases) nhận bản vá bảo mật.

## Dữ liệu QTzalo lưu trên máy

Mọi dữ liệu nằm trong một thư mục riêng của người dùng:

| Hệ điều hành | Thư mục |
|---|---|
| Windows | `%APPDATA%\QTzalo` |
| Linux | `$XDG_DATA_HOME/qtzalo` (mặc định `~/.local/share/qtzalo`) |
| macOS | `~/Library/Application Support/QTzalo` |

| File | Nội dung | Mức nhạy cảm |
|---|---|---|
| `session.json` | Cookie `zpw_sek`, khoá `zpw_enk`, khoá phiên, định danh thiết bị (IMEI), bảng domain | **Rất cao**: ai có file này có thể dùng phiên Zalo của bạn mà không cần đăng nhập lại |
| `zalo_history.db` | Lịch sử tin nhắn đã tải về (SQLite, không mã hoá) | Cao |
| `threads_cache.json` | Danh sách hội thoại, tên, xem trước tin nhắn cuối | Trung bình |
| `app_settings.json` | Tuỳ chọn giao diện | Thấp |
| `stickers_cache/` | Ảnh nhãn dán, avatar, thumbnail đã tải | Thấp–trung bình |
| `logs/`, `dump/` | Log chẩn đoán, chỉ được tạo khi chạy với `--dev` / `--dump` | Cao, có thể chứa payload đã giải mã |

Biện pháp bảo vệ:

- Trên Linux/macOS, thư mục dữ liệu có quyền `0700` và `session.json` có quyền `0600`. File được ghi nguyên tử (ghi file tạm rồi đổi tên).
- Trên Windows, `%APPDATA%` chỉ tài khoản của bạn truy cập được.
- Mọi kết nối mạng dùng TLS (rustls) và không tắt kiểm tra chứng chỉ.

Khuyến nghị:

- Không chia sẻ, không commit, không gửi `session.json` cho bất kỳ ai.
- Nếu nghi ngờ lộ phiên: trên điện thoại vào *Zalo → Cài đặt → Tài khoản và bảo mật → Thiết bị đăng nhập* và đăng xuất thiết bị đó. Sau đó đăng xuất trong QTzalo (`zalo-rust --logout`).
- Chỉ tải bản cài từ trang Releases chính thức và kiểm tra SHA256 với `SHA256SUMS.txt`.

## Giới hạn đã biết

- Mã hoá ở tầng ứng dụng của giao thức Zalo (AES-CBC với IV bằng 0, khoá cố định) là đặc thù giao thức và không thay đổi được phía client. Bảo mật đường truyền dựa vào TLS.
- Lịch sử tin nhắn cục bộ chưa được mã hoá khi lưu trên đĩa.
- Bộ cài Windows chưa được ký số.

Chi tiết rà soát: [docs/SECURITY_AUDIT.md](docs/SECURITY_AUDIT.md).
