# Khôi phục tính năng trên upstream v0.0.4

Bản tích hợp lấy upstream `20e742c` làm nền, không chép đè GUI từ nhánh cũ.
Ba thay đổi nhỏ về quyền thông báo, mở trích dẫn đã nạp và hợp nhất đúng định danh
được giữ làm các commit độc lập. Những phần còn lại được nối lại với worker,
SQLite theo tài khoản và vòng đời phiên của upstream.

## Các nhóm tính năng

| Nhóm | Hành vi và tài liệu |
| :--- | :--- |
| Emoji | Alias được vẽ inline, copy giữ raw text, Unicode giữ grapheme. Có Noto fallback và helper màu macOS. [Chi tiết](zalo-message-emoji.md). |
| Reaction | Journal theo tài khoản ghi trước khi phát sự kiện, replay đúng tin kể cả tin chưa nạp; số đếm và trạng thái xóa được giữ. Import cache là thao tác chọn nguồn rõ ràng. |
| Mention | Span incoming kiểm UTF-16 và UID; composer chọn thành viên thật, giữ metadata qua nháp và gửi. Gõ tên đơn thuần không tạo UID. [Giao thức](../re-notes/36_composer_group_mentions.md). |
| Nháp và gửi | Stage trước network, client ID bền, ack riêng text/tệp, mở phần chưa xác nhận để sửa thay vì tự gửi lại. [Chi tiết](durable-composer.md). |
| Lịch sử | Kho riêng theo tài khoản, metadata đầy đủ, phân trang 200 tin, tìm tiếng Việt, trích dẫn ngoài viewport, xóa đúng ID và tombstone. [Chi tiết](LICH_SU_THEO_TAI_KHOAN.md). |
| API GUI | HTTP/MCP nhúng đọc đúng kho tài khoản và epoch. API chạy độc lập và TUI giữ hợp đồng legacy. [Kiểm chứng](verification/account-scoped-gui-api.md). |
| Ảnh | Thumbnail lỗi không chặn lấy bản gốc; cache có giới hạn, đổi tài khoản không nhận kết quả cũ; mở viewer native qua tệp riêng và argv, không qua shell. |
| Thông báo | Lọc theo tài khoản, mute, foreground, ID máy chủ và chống lặp; bootstrap lịch sử không tạo thông báo mới. Quyền hệ điều hành được hiển thị riêng. |
| Presence | Timestamp, lỗi/quyền riêng tư và phản hồi cũ được xử lý có kiểm tra; không thay ngưỡng online chỉ để giống nhánh archive. |
| Trạng thái giao/đọc | Receipt dùng định danh wire đã xác minh. Đọc cục bộ chỉ cập nhật sau dwell trong vùng tin thực sự hiển thị; gửi seen là kết quả riêng. [Ngoại lệ giao thức cần review](adr/0006-evidence-bound-seen-v2-transport.md). |
| Đồng bộ điện thoại/nhóm | Giữ luồng HTTP history của upstream, bổ sung provider socket và decode archive qua helper; dữ liệu archive chỉ bổ sung cache, không tua ngược tin live. [Giao thức](../re-notes/phone-history-pipeline.md). |
| Đổi phiên | Epoch fence chặn A/B/A, tác vụ blocking và sự kiện muộn; side effect native cũng kiểm lại phiên. Không dùng UID giống nhau để nhận lại công việc từ incarnation cũ. |

## Ranh giới kiểm chứng

- `cargo test --locked` dùng fixture tổng hợp, SQLite disposable, loopback và fake
  transport. Nó không gửi tin, reaction hoặc seen tới tài khoản thật.
- Các self-test helper macOS kiểm codec, ảnh và glyph riêng với GUI tương tác.
  Build thành công không chứng minh quyền thông báo đã cấp hoặc toast đã hiện.
- Default build không chứa artwork Zalo hoặc tự tải pack. Một số alias không có
  Unicode tương ứng vẫn hiển thị raw text. Pack local là opt-in riêng.
- Protocol seen-v2 có bằng chứng nguồn và test loopback, chưa được xem là provider
  acceptance. ADR-0006 để trạng thái đề xuất cho maintainer review.
- Dữ liệu schema mới không được hạ phiên bản tại chỗ. Rollback source giữ nguyên
  tệp người dùng; thử rollback và migration luôn dùng bản sao disposable.

## Giữ tương thích upstream

Không thay bộ hẹn giờ, feed, updater hoặc cơ chế resume HTTP history bằng bản cũ.
Không đồng nhất thread theo tên/avatar, không đoán server ID từ timestamp và không
khôi phục nội dung Recall bằng cache. Các nhánh archive vẫn là nguồn đối chiếu,
không phải nhánh để phát triển tiếp hoặc merge nguyên khối.
