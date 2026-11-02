# Nhắc tên thành viên từ composer

## Nguồn và hợp đồng

Nguồn local: `dist/old-media-research/shared-worker-original.js`.
SHA-256: `0a5a08a2a1d074c827d2b1826f8434fc04e243a1daa0a03a376353e54cdd2466`.
Các offset dưới đây là chỉ số ký tự JavaScript của file đọc thành chuỗi Unicode, không phải byte offset.

- `static sendMentionMessage` tại 12179650 gọi `JSON.stringify(a)` trước khi gán `mentionInfo:a`, giữ `message:t`, `clientId:n`, `grid`, `language`, `visibility`; endpoint là `/api/group/mention`.
- `_getListMention` của editor tại 13221135 lấy `getDataNodeInfo()` và tạo `{pos:e.startOffset,len:e.length,uid:n.uid,type:n.type}`. Node offset và string length của JavaScript dùng UTF-16. Một nhánh tương tự tại 13224254 dùng `{pos:t.startOffset,len:t.length,uid:n.uid,type:n.type}`.
- Export `MENTION_TYPE_USER` trỏ tới `Vi`; khai báo `Vi=0,Hi=1` tại 1510789 phân biệt user và all. Slice này chỉ tạo user type 0 từ UID được chọn trực tiếp.
- `transformQuoteMessage` tại 12177181 thêm `e.mentionInfo=JSON.stringify(o)` khi có mentions. `sendGroupQuoteMessage` tại 12178201 gọi `/api/group/quote` qua transform này.

Implementation bổ sung internal variants optional mentions, giữ public API và request client ID wrappers. Không phân tích tên đã gõ thành UID, không dựng @all, không gửi mention trong chat cá nhân. Các trường đã có của request group/quote, signing, params encryption và transport được giữ nguyên. Quote fallback hiện có vẫn gửi text khi endpoint quote lỗi; fallback này giữ mentions cùng request ID, nhưng không khẳng định server đã giữ reply.

## Fixture và kiểm tra

`tests/fixtures/composer_group_mentions.json` là dữ liệu tổng hợp, không phải capture tài khoản live. Hai thành viên cùng tên `An` có UID khác nhau. Emoji trước mentions làm UTF-16 offset khác Unicode scalar offset; khoảng trắng đầu/cuối được giữ nguyên.

`chat::send_identity_tests::composer_signed_group_mentions_preserve_raw_spans_quote_and_request_id` chạy loopback HTTP, giải mã `params` bằng khóa tổng hợp và kiểm tra payload thực từ standard session path cho mention, quote, remote fallback và local fallback. Test không gửi tới Zalo. Headless egui test nhấn picker bằng pointer events, chọn UID 22 và xác minh command gửi có raw text/reply/offset thật.

Chưa có nghiệm thu tài khoản Zalo live. Draft/retry recovery chỉ trong bộ nhớ; outbox durable thuộc slice khác. Runtime generation guard được tích hợp ở parent, không thay bằng account UID check.
