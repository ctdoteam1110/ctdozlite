# Nhập bản sao lưu Zalo Desktop V2

## Tự tìm khóa trên Windows

1. Đăng nhập cùng tài khoản trong ctdozlite và Zalo Desktop.
2. Trong ctdozlite, mở Cài đặt → Đồng bộ tin nhắn → Nhập bản sao lưu Zalo Desktop và chọn tệp `.zl.zip`.
3. Thoát Zalo hoàn toàn từ biểu tượng ở khay hệ thống, rồi chọn **Tự tìm khóa từ Zalo**. Chờ ứng dụng xác minh khóa và mở lại Zalo bình thường.
4. Chọn **Nhập bản sao lưu** và chờ thông báo kết quả. Khóa không được ghi vào tệp hoặc nhật ký.

Nếu không tìm được khóa tự động, làm theo hướng dẫn thủ công bên dưới.

## Hướng dẫn thủ công

1. Đăng nhập cùng tài khoản trong ctdozlite và Zalo Desktop. Thoát Zalo hoàn toàn bằng biểu tượng ở khay hệ thống.
2. Trên Windows, mở PowerShell và chạy đường dẫn `Zalo.exe` của phiên bản đã cài cùng hai tham số `--remote-debugging-address=127.0.0.1 --remote-debugging-port=9229`. Chỉ bật chế độ này trên máy cá nhân và chỉ trong lúc lấy UIN.
3. Mở Chrome hoặc Edge, vào `chrome://inspect`, cấu hình `localhost:9229`, và chọn trang chính của Zalo. Trong DevTools Console, dùng đoạn JavaScript ở phần “Recover the UIN from the authorized local session” của hướng dẫn được liên kết trong cài đặt để lấy UIN nội bộ 32 ký tự. Không chia sẻ UIN và không đưa UIN vào mã nguồn hoặc nhật ký.
4. Thoát hoàn toàn phiên Zalo có debugger, mở lại Zalo bình thường, rồi kiểm tra cổng 9229 không còn lắng nghe. Trong ctdozlite, mở Cài đặt → Đồng bộ tin nhắn → Nhập bản sao lưu Zalo Desktop, chọn tệp `.zl.zip`, nhập UIN vào ô được che, và bắt đầu nhập.
5. Chờ thông báo kết quả. Giá trị UIN chỉ tồn tại trong bộ nhớ trong lúc xử lý. Thông báo phân biệt tin nhắn được thêm, tệp tìm thấy, dữ liệu chưa hỗ trợ và tệp không có trong bản sao lưu.

Chỉ nhập tệp do chính tài khoản hiện tại tạo. Không phải mọi tệp được nhắc tới trong tin nhắn đều có mặt trong bản sao lưu. Lưu dữ liệu ctdozlite trên ổ đĩa có quyền truy cập phù hợp; cơ sở dữ liệu chứa tin nhắn và tệp khôi phục.


Đối với lịch sử cũ chưa được gắn với tài khoản, ứng dụng không tự nhận chủ sở hữu. Hãy tạo hồ sơ dữ liệu mới bằng tùy chọn dòng lệnh --data-dir và đăng nhập trước khi nhập bản sao lưu.
