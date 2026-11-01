# Nền tảng lịch sử theo tài khoản

Kho lịch sử mới lưu riêng từng tài khoản tại
`accounts/<sha256(account_id)>/history-v2.sqlite3`. Tệp cũ và các bảng lịch đồng bộ,
hẹn giờ, API vẫn giữ nguyên. Nền tảng này chưa tự chuyển GUI sang kho mới.

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
