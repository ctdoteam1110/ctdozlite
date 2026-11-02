# Nền tảng lịch sử theo tài khoản

Kho lịch sử mới lưu riêng từng tài khoản tại
`accounts/<sha256(account_id)>/history-v2.sqlite3`. Tệp cũ và các bảng lịch đồng bộ,
hẹn giờ và API độc lập vẫn giữ nguyên. GUI dùng kho mới qua worker cho tải, lưu, phân trang,
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

### Ghi chờ, thử lại và chuyển phiên

Worker giữ trang đã nhận trong mailbox có tối đa 8 lượt được đặt chỗ trước khi đọc
nguồn tiếp theo. Toàn bộ trang mạng tối đa 128 MiB được giữ trước lần chờ ghi đầu
tiên, kể cả phần chưa ghi; mỗi phần ghi SQLite tối đa 32 MiB. Chỉ sau xác nhận ghi
thành công mới tiến con trỏ phần và công bố kết quả lên GUI. Một tin hoặc metadata
riêng vượt 32 MiB, hoặc một trang vượt 128 MiB, bị từ chối rõ ràng, không được xác
nhận đã lưu. Snapshot thay thế từ GUI giới hạn 1 MiB và dùng `LatestSnapshot` chung
với composer; snapshot của phiên cũ bị từ chối sau chuyển phiên.

Lượt ghi hợp lệ thất bại vẫn nằm trong mailbox để thử lại, không gửi lại tin lên
máy chủ. Chuyển tài khoản, đăng xuất và đóng ứng dụng flush cả lịch sử lẫn nháp qua
cùng hàng rào vòng đời; lỗi ghi chặn xác nhận hoàn tất. Mailbox trong bộ nhớ không
thay cho bảo đảm sống sót qua crash trước khi ghi đĩa.

Trang điện thoại có sender chưa phân giải được lưu nguyên bản, ràng buộc tài khoản,
trong thư mục riêng `phone-pending-v1` với quyền 0600/0700 trước khi tra UID. Tệp chờ
chỉ được tiếp tục khi người dùng bắt đầu đồng bộ rõ ràng; sau khi toàn bộ tin đã vào
kho mới thì tệp mới bị xóa. Giới hạn là 8 tệp, tổng 256 MiB và 128 MiB mỗi trang.
Xóa dữ liệu cục bộ cũng xóa các tệp chờ đúng tài khoản, không để chúng tự phục hồi
lịch sử vừa xóa. Tài khoản khác, phiên đăng nhập và tệp legacy được giữ nguyên.

Kết quả gửi thành công từ GUI chỉ được công bố sau khi hàng lịch sử đã ghi bền vững.
Composer có outbox bền vững trước transport. Các nút gửi legacy và scheduler chỉ
đặt chỗ bộ nhớ, không có cùng bảo đảm durable intent trước transport hoặc exactly
once. Phản hồi hết phiên hoặc chưa xác nhận lưu được báo trạng thái chưa rõ, không
công bố thành công và không tự gửi lại.

### Ranh giới tích hợp và nghiệm thu

- API HTTP/MCP nhúng trong GUI dùng kho theo tài khoản; CLI/TUI, API độc lập và
  bảng scheduler vẫn dùng đường legacy tương ứng.
- Adapter dùng runtime epoch fence, kiểm tra lại phiên dưới mutation guard trước I/O.
  Test consumer chặn A/B/A cùng khóa phiên và tác vụ blocking còn lại sau hủy.
- Test consumer chạy luồng worker thực tế với dữ liệu disposable, không thay cho
  nghiệm thu thao tác GUI hoặc transport với tài khoản thật.

### Dấu xóa có định danh máy chủ

Schema 3 giữ thêm định danh máy chủ đã xác minh trong dấu xóa. Tin mới dùng lại
client ID vẫn được giữ nếu máy chủ cấp ID khác; bản cũ bị xóa không xuất hiện lại
khi đổi khóa canonical. Dấu xóa schema 2 thiếu nguồn định danh được giữ nguyên và
báo lỗi mơ hồ thay vì đoán. Nâng cấp không xóa hàng hay sửa kho legacy.
Xem [ADR-0005](adr/0005-server-aware-deletion-tombstones.md).
