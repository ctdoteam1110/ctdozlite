# Bản macOS Apple Silicon của fork

Bản cộng đồng miễn phí từ [ctdoteam1110/ctdozlite](https://github.com/ctdoteam1110/ctdozlite), giữ giấy phép MIT OR Apache-2.0. Hỗ trợ Mac chip Apple (M1 trở lên), macOS 11+. Intel, Windows và Linux không nằm trong ma trận phát hành của fork.

## Cài đặt

1. Mở [Releases của fork](https://github.com/lebad280324/ctdozlite/releases/latest).
2. Tải tệp có đuôi `macos-arm64.dmg`, mở DMG và kéo ứng dụng vào Applications.
3. Nếu thay bản cũ, thoát ứng dụng trước. Dữ liệu tài khoản không được đóng vào gói cài đặt.
4. Gói hiện ký ad-hoc, chưa có Developer ID/notarization. Nếu macOS chặn, xem hướng dẫn [mở ứng dụng từ nhà phát triển khác của Apple](https://support.apple.com/102445); chỉ cho phép đúng ứng dụng đã tải từ trang phát hành. Không tắt bảo vệ toàn hệ thống.

## Cập nhật

Mở **Cài đặt → Cập nhật**. Ứng dụng kiểm tra khi khởi động và định kỳ; cần truy cập HTTPS đến `api.github.com`, `github.com` và CDN tải tài sản của GitHub. Không cần token hay tài khoản GitHub trong ứng dụng.

Nguồn chính: `https://api.github.com/repos/lebad280324/ctdozlite/releases/latest`. Nguồn dự phòng: `https://github.com/lebad280324/ctdozlite/releases/latest/download/latest.json`.

Bộ cập nhật chỉ chọn gói tương thích, xác thực SHA-256 trước khi cài. Với gói có checksum và được cài trong thư mục ghi được, tiến trình phụ chờ ứng dụng thoát, kiểm tra chữ ký ad-hoc, phiên bản và kiến trúc rồi thay thế. Nếu không có checksum, ứng dụng mở DMG để cài thủ công. Bản sao cũ được giữ trong giao dịch đến khi lệnh mở ứng dụng thành công; đây không phải kiểm tra sức khỏe sau khởi động.

Nếu kho chưa có release công khai, API latest trả 404. Nếu mất mạng hoặc bị giới hạn API, thử lại hoặc mở trang Releases trong trình duyệt.

## Đồng bộ lịch sử từ điện thoại

Tính năng này cần **Node.js và Zalo PC chính thức đã cài trên Mac**. Bộ trợ giúp nạp thư viện đồng bộ từ bản Zalo PC trên máy; giao diện ứng dụng vẫn chạy bằng Rust native.

Mở **Cài đặt → Đồng bộ tin nhắn → Đồng bộ tin nhắn từ điện thoại ngay**, xác nhận trên điện thoại và giữ Zalo mở trong lúc tải. Tùy chọn **Chỉ đồng bộ hội thoại đang mở** dùng để tải riêng một hội thoại. Tiến trình toàn bộ chạy qua nhiều đợt; chờ kết quả cuối trước khi bấm lại.

Kết quả cho biết số tin mới được nhập từ điện thoại. Tin đã có trên Mac được giữ nguyên và tránh nhập trùng. Các phạm vi lịch sử điện thoại báo không tìm thấy (mã 502) được thống kê riêng. Kết quả chỉ có Cloud/Nhóm chưa xác nhận đồng bộ lịch sử điện thoại hoàn tất.

Nếu kết nối gián đoạn, ứng dụng chờ phản hồi đúng phiên đã gửi để tiếp tục. Khi không xác nhận được, ứng dụng báo đồng bộ chưa hoàn tất và giữ dữ liệu đã nhập. Có thể bấm **Dừng đồng bộ điện thoại** để kết thúc phiên.

## Tạo bản tiếp theo

- Tăng version trong Cargo.toml/Cargo.lock, thêm `docs/releases/v<version>.md` và cập nhật CHANGELOG.md.
- Chạy kiểm thử, cargo audit, kiểm tra mã nguồn và dữ liệu nhạy cảm trước khi commit/push theo tiêu chuẩn dự án.
- Chỉ gắn tag `v<version>` vào commit sạch đã qua CI. GitHub Actions build đúng `aarch64-apple-darwin`, kiểm tra native bundle và chữ ký, tạo DMG, SHA256SUMS.txt và latest.json rồi xuất bản.
- Không tải gói từ máy cá nhân lên thay artifact được CI xác minh. Không đưa session, database hoặc tài nguyên codec riêng vào repository/release.
