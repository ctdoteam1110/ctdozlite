# Nháp và gửi bền từ composer

Composer lưu nháp theo tài khoản và thread bằng worker; UI chỉ tạo snapshot trong bộ nhớ. Snapshot có raw text, reply gốc, UID/range UTF-16 đã chọn và attachment paths. Metadata mention được kiểm lại khi đọc và trước gửi, không suy UID từ tên đã gõ.

- Thay đổi nháp được coalesce trong mailbox tối đa 32 thread, mỗi payload tối đa 256 KiB, với một wake không mang payload. Worker debounce 120 ms rồi ghi; lỗi không xác nhận lưu thành công và giữ snapshot để thử lại. Đóng ứng dụng/đổi phiên yêu cầu flush dưới lifecycle barrier. Lỗi flush chặn close/logout, giữ mailbox và trở về incarnation đã xác thực để tiếp tục sửa/thử lại.
- Nút ảnh/tệp chọn attachment vào nháp, drag/drop và clipboard cũng thuộc nháp. Người dùng nhấn **Gửi** sau khi kiểm tra. Attachment paths được khôi phục sau restart; danh sách không đòi UI đọc file hoặc decode thumbnail.
- UI giữ nguyên nháp cho đến khi worker xác nhận **stage** đã ghi thành công. Nếu người dùng sửa hoặc đổi thread trong lúc stage, chỉ bản nháp còn khớp raw text, selected UID, reply và paths mới được xóa. Stage không phải xác nhận server đã nhận.
- Stage lưu một client ID decimal cho text và từng ID riêng cho tệp trước network. Cùng process, allocator CAS được chia sẻ với plain/sticker/forward legacy; store vẫn dùng persisted floor và khóa interprocess. Không khẳng định allocator legacy cũ bảo đảm giữa nhiều process.
- Văn bản/reply/mention được gửi trước các tệp; tệp không lặp caption. Đây là thay đổi hành vi rõ ràng, không thêm field mention/reply chưa được xác minh vào attachment protocol. Link card cũ vẫn dùng standard signed path với client ID đã stage.
- Text ack được lưu độc lập trước attachment ack. Lỗi tệp không hạ trạng thái text đã xác nhận. Mỗi attachment được ack theo ID, kể cả hai paths giống nhau. Khôi phục tệp còn lại không mang lại text/reply/mentions đã xác nhận.
- Sau restart hoặc phiên cũ bị hủy, pending trở thành **chưa rõ kết quả**, không tự gửi lại. UI yêu cầu kiểm tra lịch sử và bấm **Mở phần chưa xác nhận để sửa**. Khôi phục chỉ tạo nháp có thể sửa, không gọi transport. Nháp mới hơn không bị ghi đè.
- Lỗi sau stage/network được báo là chưa rõ hoặc đã xác nhận nhưng chưa lưu trạng thái; không dùng thông báo “chưa gửi” cho request có thể đã tới server.

`spawn_transition_blocking` chỉ phục vụ auth commit được cho phép, cập nhật ticket của chính task khởi tạo trước notify. Blocking descendants thông thường giữ snapshot ticket cũ và bị fence; session lock đi trước mutation barrier, không giữ sync barrier qua await.

## Kiểm chứng

Tests dùng temp account stores, synthetic members và fake transport hoặc loopback encrypted protocol fixture, không gửi tới tài khoản Zalo live. Consumer tests kiểm stage-before-network, exact IDs, partial ack, reboot/load events, edit-after-failure, same-account transition trước queued wake, failed lifecycle flush/retry và cancellation trước poll. Pointer egui test sử dụng picker thật và command durable thật.

Phạm vi ở đây là durable composer/outbox. AccountHistory exact confirmed-row sink, legacy sticker/scheduler/button producers và persistence queue chung của toàn ứng dụng thuộc bước tích hợp A4, chưa được dùng để khẳng định toàn bộ lịch sử/outgoing producers đã bền.
