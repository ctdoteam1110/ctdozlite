# Giới hạn an toàn của đồng bộ điện thoại (2026-11-08)

## Android và định danh người gửi

`SenderId` trong cơ sở dữ liệu Android là số nguyên; `Session.login.uin` là định danh của phiên web. Không có bằng chứng trong cấu trúc `Session` cho phép coi hai giá trị này là cùng một định danh, kể cả khi UIN chỉ chứa chữ số. `send2me_id` cũng không chứng minh định danh Android.

Luồng nhập hiện có gọi `/api/znoise` để ánh xạ `fids` của tên tập tin Android sang định danh web. Chỉ khi ánh xạ của một tập tin người dùng trùng chính xác UIN của phiên đã bắt đầu nhập, ứng dụng mới dùng số của tập tin đó làm định danh người gửi của chủ tài khoản. Không dùng ánh xạ nhóm để nhận diện chủ tài khoản. Thiếu hoặc có nhiều ánh xạ phù hợp thì dừng trước khi mở kho dữ liệu đích, thay vì suy đoán hướng tin nhắn. Bản sao không chứa ánh xạ của chính tài khoản có thể chưa nhập được.

## Transfer Sync v2

Đường dẫn trước đây gọi Node.js và WASM ở ngoài gói ứng dụng để sinh khóa, giải mã metadata và giải mã tin nhắn. Khóa công khai dự phòng chỉ là dữ liệu ngẫu nhiên mang tiền tố đường cong, không có khóa riêng tương ứng được quản lý bởi mã Rust.

Cho đến khi có triển khai mật mã native cùng mẫu kiểm chứng, yêu cầu Sync v2 và nhập các gói Sync v2 trả lỗi rõ ràng trước khi tạo khóa, đánh thức điện thoại, tải gói hoặc ghi dữ liệu. Luồng Android RSA và các luồng Cloud/Nhóm vẫn độc lập. Các script nghiên cứu có thể còn trong kho mã, nhưng chương trình Rust không thực thi chúng.

## Kiểm chứng

Các kiểm thử dùng định danh giả để kiểm tra ánh xạ của hai tài khoản, trường hợp không xác định hoặc mâu thuẫn, cách ly bộ nhớ đệm theo tài khoản và từ chối Sync v2. Không kiểm thử với phiên thật, không gọi máy chủ Zalo. Chưa xác nhận giao thức bằng một lần đồng bộ điện thoại trực tiếp; tài liệu này ghi lại giới hạn và điều kiện dừng an toàn, không khẳng định đã xác minh giải mã native.
