# Nền tảng lịch sử theo tài khoản

Kho lịch sử mới lưu riêng từng tài khoản tại
`accounts/<sha256(account_id)>/history-v2.sqlite3`. Tệp cũ và các bảng lịch đồng bộ,
hẹn giờ, API vẫn giữ nguyên. GUI dùng kho mới qua worker cho tải, lưu, phân trang,
tìm kiếm và mở trích dẫn ngoài phần tin nhắn đã tải. GUI không tự nhập tệp cũ hoặc
cache của tài khoản khác khi đăng nhập.

## Dữ liệu được giữ

- Định danh cuộc trò chuyện và tin nhắn chính xác, kể cả ID trùng ở hai cuộc trò chuyện.
- Metadata ảnh, sticker, liên kết, trích dẫn, emoji, reaction, mention và trạng thái giao/đọc.
- Con trỏ chưa đọc, trạng thái tắt thông báo và dấu chưa đọc thủ công.
- Dấu xóa cục bộ theo đúng tin nhắn, ngăn bản lịch sử cũ làm tin đã xóa xuất hiện lại.

Mỗi lần đọc lịch sử hoặc tìm kiếm trả tối đa 200 tin. Tìm kiếm hỗ trợ tiếng Việt
không dấu và coi nội dung nhập là văn bản, không phải cú pháp truy vấn. Trích dẫn
chỉ mở đúng một kết quả theo ID; các ID mơ hồ không bị đoán bằng tên hay nội dung.

## Nhập dữ liệu cũ

Việc nhập chỉ chạy khi được gọi rõ ràng và sao chép sang kho đích. Tệp SQLite nguồn
được mở chỉ đọc; JSON nguồn không bị ghi lại. Lỗi nhập hủy toàn bộ lượt ghi đích.
Nhập lại cùng bộ nguồn không tự phục hồi những tin đã xóa trong kho mới.

Kho cũ hiện không lưu đầy đủ trạng thái chưa đọc trong SQLite. Nếu JSON hoặc bảng
chưa đọc của nguồn không chứa trạng thái đó, việc nhập không thể dựng lại dữ liệu
chưa từng được lưu. Kho mới lưu trạng thái này bền vững cho các lần ghi tiếp theo.

Ứng dụng từ chối tệp có phiên bản mới hơn hoặc chủ tài khoản khác trước khi sửa tệp.
Quyền tệp/thư mục mới trên Unix là 0600/0700. Thiết kế và phạm vi tích hợp nằm tại
[ADR-0004](adr/0004-account-owned-history-store.md).

## Luồng GUI

Nút tải thêm đọc một trang cục bộ tối đa 200 tin theo con trỏ thời gian và ID,
không tăng dần một lệnh LIMIT để nạp lại toàn bộ hội thoại. Đồng bộ lịch sử từ máy
chủ vẫn là thao tác riêng, giữ nguyên giao thức hiện có và ghi từng phần vào kho
của tài khoản đang đăng nhập. Tìm kiếm trong hội thoại và mở trích dẫn có thể đọc
những tin cũ chưa nằm trong cửa sổ gần nhất.

Xóa một tin tạo dấu xóa đúng ID. Xóa lịch sử một hội thoại loại bỏ metadata và trạng
thái chưa đọc của hội thoại, đồng thời giữ dấu xóa cho các ID đã có; bản snapshot cũ
không khôi phục chúng. Một tin mới có ID khác vẫn được phép xuất hiện bình thường.
Xóa dữ liệu cục bộ chỉ tác động kho của tài khoản hiện tại, không xóa tệp legacy,
bảng hẹn giờ hoặc phiên đăng nhập. Đăng xuất giữ nguyên lịch sử theo tài khoản.

### Phần tích hợp còn lại

- API HTTP/MCP và TUI vẫn đọc kho legacy, chưa phản chiếu đầy đủ lịch sử GUI mới.
- Snapshot lưu từ GUI cần bước coalescing dùng LatestSnapshot chung với composer.
- Kiểm chứng đổi tài khoản cùng định danh phiên nhiều lần cần runtime epoch fence;
  kiểm tra tài khoản và khóa phiên tại adapter không thay thế hàng rào generation.
- Các test consumer chứng minh luồng worker thực tế với dữ liệu disposable, không
  thay cho nghiệm thu thao tác GUI với tài khoản thật.
