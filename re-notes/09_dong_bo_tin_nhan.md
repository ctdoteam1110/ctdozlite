# 09 — Cơ chế đồng bộ tin nhắn của Zalo PC

Ngày: 2026-11-01. Mục tiêu: hiểu Zalo PC làm thế nào để
(a) **mở máy sau một thời gian tắt** (ví dụ 1 ngày) vẫn tự hiện đủ tin nhắn đã nhận/gửi trên điện thoại, và
(b) **cài trên máy mới** thì lấy được lịch sử cũ.

Nhãn bằng chứng: **[O]** đọc trực tiếp trong mã Zalo; **[I]** suy luận; **[U]** chưa biết.

## 0. Nguồn và giới hạn

- **Không chạy được Zalo Desktop.** Phiên phân tích là container Linux trên cloud; proxy mạng chặn `zalo.me` / `zadn.vn` nên không tải được bộ cài mới.
- Nguồn **[O]** dùng ở đây:
  - `web-bundle/default-embed-render.js` (bundle giao diện Zalo PC, ~945 KB) lấy từ lịch sử git (`git show 54314e0^:web-bundle/default-embed-render.js`). File này đã bị xoá khỏi repo vì lý do bản quyền và **không được commit lại**.
  - Các ghi chú và trích dẫn có sẵn trong repo: `re-notes/02_chat_data.md`, chú thích `[O]` trong `src/chat.rs` (`sync-v2-sub-worker.js`, `compact-app-pc.js` của Zalo PC 26.10.11) và `src/socket.rs`.
- Bundle giao diện chứa cấu hình, tên trạng thái, schema DB và chuỗi hiển thị, **không chứa** lệnh mạng của luồng đồng bộ từ điện thoại. Phần đó nằm trong các worker chunk khác (`sync-v2-sub-worker`, …) không có trong repo → các endpoint của luồng "chuyển tin từ điện thoại" vẫn là **[U]**.

## 1. Tóm tắt

Zalo PC có **ba cơ chế khác nhau**, người dùng thường gọi chung là "đồng bộ":

| # | Tình huống | Cơ chế | QTzalo hiện tại |
|---|---|---|---|
| A | Tắt PC vài giờ đến vài ngày rồi mở lại | **Hàng đợi hành động phía máy chủ + con trỏ lưu bền** (`lastActionId` / `lastId` từng hàng đợi). Mở app thì kéo hết các hành động kể từ con trỏ. | Có kéo hàng đợi, nhưng **con trỏ không được lưu**: luôn bắt đầu từ `1` và chỉ kéo tối đa 3 đợt. |
| B | Nhóm và "Cloud của tôi" | **Lịch sử lưu trên máy chủ** (`/api/cm/getrecentv2`, `/api/cm/getoldv2`), tải theo trang. | Có: mỗi lần làm mới lấy 50 tin gần nhất mỗi nhóm; "Tải thêm tin nhắn cũ" lấy thêm. |
| C | Máy mới, hoặc tắt quá lâu so với thời gian máy chủ giữ hàng đợi | **"Đồng bộ tin nhắn từ điện thoại"**: điện thoại xuất DB, mã hoá, PC tải về, giải mã rồi trộn vào DB (chỉ một lần). Tin 1-1 cũ **không** lấy được từ máy chủ. | Không có. Giao thức **[U]**. |

Muốn "mở máy sau 1 ngày là tự có đủ tin như điện thoại" thì việc quan trọng nhất là **cơ chế A**, và việc này làm được mà không cần giao thức mới.

## 2. Cơ chế A — bắt kịp tin nhắn khi offline

### 2.1 Hàng đợi và con trỏ

- **[O]** Socket `zpw_ws`: sau `AUTHEN`, máy chủ liệt kê các hàng đợi cần kéo. Client kéo mỗi hàng đợi bằng `{first, reqId, lastId, preIds}` (`doGetOffline`, xem `src/socket.rs:14`). Nếu phản hồi có `more == 1` thì kéo tiếp từ `lastId` mới.
- **[O]** Long-poll HTTP `getChatFromPolling` (cmd 99100): tham số là
  `"{lastActionId},{lastActionIdOther}," + base64(JSON {queueId: {lastId, ids}}) + ",1"`. Phản hồi có `lastActionId`, `queueStatus`, `more` và có thể có `resetLastActionId` (`src/chat.rs:2897-3017`).
- **[O]** Cấu hình mặc định của Zalo PC (`default-embed-render.js`):
  ```
  offline_timeout: 48e4            // 8 phút mất kết nối thì coi là offline
  ping_interval: 6e4
  sync_id_from_lp: true, sync_id_to_lp: true,          // con trỏ dùng chung giữa socket và long-poll
  sync_ctrl_id_from_lp: true, sync_ctr_id_to_lp: true,
  init_lastid_lp: true, init_lastid_ctrl_sock: true,   // khởi tạo lastId từ giá trị đã lưu
  queue_actionid_map: {"510_0":"1", "511_0":"6", "510_1":"15", "511_1":"16"},
  queue_ctrl_actionid_map: {"603_0":"4","610_0":"5","611_0":"12","603_1":"19","604_0":"20","611_1":"17","610_1":"18"},
  evict_broadcast_queues: { lp_msg: [1,6,16], lp_ctrl: [4,19],
                            sock_msg: ["510_0","511_0","513_0","514_0","515_0","511_1","510_1",...] },
  msg_11_cmds: ["501"], msg_11_offline: ["510"]
  ```
  → Socket queue `510_0` (tin 1-1 offline) **tương ứng** long-poll queue `"1"`; `511_0` (tin nhóm offline) tương ứng `"6"`; `510_1` → `"15"`; `511_1` → `"16"`. Hai đường truyền dùng **chung một con trỏ**.
- **[O]** Schema DB cục bộ của Zalo PC: bảng `conversation` có cột `lastActionId`, `syncFromMobile`, `lastSmsLocalId`; bảng `message` cũng có `syncFromMobile`. Tức là con trỏ được **lưu bền theo từng hội thoại** trong DB của PC.
- **[O]** `update_last_action_time: { trigger_enable_last_action_time: 10, enable_deliver: 1, msg_cache_limit: 500 }`.

### 2.2 Khi máy chủ không còn giữ đủ hàng đợi

- **[O]** Bảng `missing_message_range { id, convId, fromTs, toTs, reason, status }` (index `convId_toTs`), module `missingMessageRangeManager`, tính năng `NotifyMissingMessages` / `MissingMessageEvents`.
- **[O]** Chuỗi hiển thị khi phát hiện khoảng trống:
  - "Đồng bộ từ điện thoại để xem tin nhắn trước ngày $0$/$1$/$2$." (trong hội thoại)
  - "Đồng bộ từ điện thoại để tìm tin nhắn trước ngày …" (tìm kiếm)
  - banner "Tin nhắn chưa được đồng bộ — Đồng bộ với điện thoại để có đủ tin nhắn trên máy này. [Đồng bộ ngay]"
- **[I]** Luồng hoạt động: con trỏ quá cũ (hoặc máy chủ trả `resetLastActionId`) → client nhảy tới con trỏ mới, ghi một `MissingMessageRange` cho khoảng bị mất → hiện banner gợi ý chạy cơ chế C. Thời gian máy chủ giữ hàng đợi là **[U]**.

## 3. Cơ chế B — lịch sử nhóm và Cloud trên máy chủ

- **[O]** `/api/cm/getrecentv2` và `/api/cm/getoldv2` trên `group_cloud_message`, dùng cho nhóm (`g<groupId>`) và Cloud của tôi (`src/chat.rs:3020`).
- **[O]** Cấu hình `load_media_from_cloud_message: { my_cloud: 0, group: 0, one_on_one: 0 }` và các TTL file (`noncloud/cloud TTLUpper 6048e5` = 7 ngày, `TTLLower 12096e5` = 14 ngày, `3/*/expired TTL 23328e5` = 27 ngày). Điều này cho thấy media/file trên máy chủ có hạn lưu.
- **[I]** Tin 1-1 cũ không có đường lấy lại tương đương: không thấy endpoint "history 1-1" nào trong registry REST (`re-notes/02_chat_data.md` §1).

## 4. Cơ chế C — "Đồng bộ tin nhắn từ điện thoại"

### 4.1 Hiển thị cho người dùng **[O]**
- Modal "Đồng bộ tin nhắn từ điện thoại": "Để đảm bảo tính bảo mật, Zalo chỉ hiển thị tin nhắn từ sau lần đăng nhập đầu tiên trên thiết bị này. Vui lòng đồng bộ từ điện thoại để xem đầy đủ nội dung trò chuyện."
- Bước 1: "Zalo sẽ đồng bộ những tin nhắn mới nhất từ điện thoại lên máy tính của bạn. Việc này chỉ được thực hiện một lần duy nhất. Vui lòng chuẩn bị sẵn điện thoại đã đăng nhập tài khoản Zalo của bạn. Trong trường hợp đổi máy mới, bạn có thể sao lưu dữ liệu từ File đã được xuất trước đó (Setting/Xuất dữ liệu → Nhập dữ liệu)."
- Web: "Zalo trên trình duyệt chỉ hiển thị tin nhắn trong vòng $0$ ngày gần nhất."

### 4.2 Máy trạng thái **[O]**
```
SYNC_DB_STATE = { DONT_SYNC:-1, START:1, USR_CONFIRMED_PC:2, USR_CONFIRMED_MB:3, WAIT_FOR_DB:6,
                  DL:7, PASSWORD:8, MERGE:9, MERGING:10, ABORT:11, ABORTED:12, DONE:14, ERR:15,
                  RESUME_MERGE:16, RESUME:17 }
ERR_CODE = { ERR_DL, ERR_SEND_CONFRIM, ERR_TIMEOUT, ERR_NO_PERMISSION, ERR_TIMEOUT_DB, ERR_MERGE,
             ERR_CANT_GET_UIDS, ERR_INVALID_DB_INFO, ERR_INVALID_ENCRYPT_TYPE, ERR_NO_CKEY,
             ERR_MB_RESTORE, ERR_MB_EXPORT_DB, ERR_DL_NO_NETWORK, ERR_DL_NOSPC, ERR_MERGE_NOSPC, … }
ERR_MERGE_CODE = { NO_PATH, NO_UID, NO_UIN, ERR_DECRYPT:50, ERR_ENCRYPT:60, NO_MID, CHANGE_USER, … }
progress_points = { download: 10, decrypt: 20, restore: 70 }
transfer_when_login = { time_wait_for_temp_key: 3e3, mobile_transfer_confirm_timeout: 3e3,
                        no_missing_message_events_timeout: 3e3, enable_drop_flow: false }
```
Một trạng thái khác (`pa`): `{DONT_SYNC:-1, DONE:0, WAIT_CONFIRM:1, WAIT_MOBILE_CONFIRM:2, ERR:3, WAIT_FOR_DATA:4}`. Feature id: `SyncMsgBackup:107`, `TrustedDevice:106`, `Sync2:88`, `MigrateZaloPc:81`. Bảng `SyncDownload {fromId, convId, actionId, clientMsgId, globalMsgId…}`.

### 4.3 Diễn giải **[I]**
1. PC đề nghị đồng bộ → người dùng xác nhận trên PC (`USR_CONFIRMED_PC`) → điện thoại nhận yêu cầu, người dùng xác nhận (`USR_CONFIRMED_MB`).
2. Điện thoại **xuất DB tin nhắn** (`ERR_MB_EXPORT_DB`), **mã hoá** bằng khoá phiên/khoá tạm (`ERR_NO_CKEY`, `ERR_INVALID_ENCRYPT_TYPE`, `time_wait_for_temp_key`; có thể cần mật khẩu/passcode — trạng thái `PASSWORD`), rồi tải lên (`WAIT_FOR_DB`).
3. PC **tải về** (`DL`, `ERR_DL_NOSPC`), **giải mã** (`ERR_DECRYPT`), **trộn** vào DB cục bộ (`MERGE`/`MERGING`, đánh dấu `syncFromMobile`), có thể **tiếp tục** nếu bị ngắt (`RESUME`, `RESUME_MERGE`).
4. Media tải riêng sau (`syncDownload.topics ["1","2.photo","2.file",…]`, `SyncDownloadFromTransfer`).

**[U]**: lệnh/endpoint gửi yêu cầu tới điện thoại, nơi chứa DB tạm, định dạng DB (SQLite?), thuật toán và nguồn khoá mã hoá, cách xác thực "thiết bị tin cậy".

## 5. So sánh với QTzalo 0.1.1

| Điểm | Zalo PC | QTzalo | Hệ quả |
|---|---|---|---|
| Lưu con trỏ hàng đợi | Lưu trong DB (`conversation.lastActionId`), `init_lastid_lp` | `PollState::default()` (= `"1"`) mỗi lần mở app và sau khi đăng nhập lại (`src/gui/worker.rs:350`, `:538`, `:559`, `:1989`) | **[I]** Khi mở lại sau 1 ngày, client không nói được "tôi đã có tới đâu". Việc tin offline có về hay không phụ thuộc vào cách máy chủ xử lý `lastId=1` (**[U]**). |
| Số đợt kéo | Kéo tới khi `more == 0` | Tối đa 3 đợt (`src/chat.rs:3150`) | Sau một ngày nhiều tin có thể bị cắt. |
| Hàng đợi long-poll tin nhắn | `lp_msg: [1, 6, 16]` (6 = nhóm offline `511_0`) | `["1","2","3"]` (`src/chat.rs:2915`). Chú thích ghi thêm 15/16 thì bị từ chối. | Mâu thuẫn, cần thử với tài khoản thật. Nhóm vẫn được bù bằng `getrecentv2`. |
| Socket kéo offline | `lastId` lưu bền | Luôn `first:true, lastId:1` (`src/socket.rs:349-356`) | Như trên. |
| `get-last-msgs` | Tham số `threadIdLocalMsgId` (tên gợi ý: msgId cục bộ cuối cùng của từng hội thoại) | Gửi `{}` hoặc `"1"` | **[I]** Có thể dùng để hỏi "tin sau msgId X" từng hội thoại. |
| Khoảng trống | `missing_message_range` + banner | Không có | Người dùng không biết mình thiếu tin. |
| Đồng bộ từ điện thoại | Có | Không | Giao thức **[U]**. |

## 6. Đề xuất cho QTzalo (chưa được người dùng chốt)

**Bước 1 — làm được ngay, không cần giao thức mới (đề xuất cho v0.1.2):**
1. **Lưu `PollState` bền** theo tài khoản (`<data>/sync_state_<uin>.json`, ghi bằng `paths::write_private`). Nạp khi khởi động; **không** reset khi chỉ mất mạng; chỉ reset khi đổi tài khoản.
2. Lúc khởi động, **kéo tới khi `more == 0`** (có giới hạn an toàn, ví dụ 200 đợt, có tiến trình "Đang đồng bộ tin nhắn… N tin"). Ghi mọi tin vào `zalo_history.db` như hiện nay.
3. Gặp `resetLastActionId` (con trỏ quá cũ): ghi một **khoảng trống** (bảng `missing_ranges` trong SQLite: `conv_id, from_ts, to_ts`) và hiện banner kiểu Zalo: "Có thể thiếu tin nhắn từ … đến … — xem trên điện thoại".
4. **Nhóm và Cloud:** sau `getrecentv2`, nếu tin cũ nhất trả về vẫn mới hơn tin mới nhất trong DB thì lật tiếp `getoldv2` cho tới khi nối liền (giới hạn số trang). Như vậy nhóm luôn đủ.
5. Tương tự cho socket: lưu `lastId` của từng hàng đợi `510_x`/`511_x`, kéo `first:true` với `lastId` đã lưu.

**Bước 2 — thử nghiệm có cờ, cần tài khoản thật:** long-poll queue `6`/`16` theo `queue_actionid_map`; `get-last-msgs` với `localMsgId` thật. Bật bằng biến môi trường, ghi log bằng `ZCALL_WS_TRACE=1` để đối chiếu.

**Bước 3 — "Đồng bộ từ điện thoại" (cơ chế C):** chỉ làm sau khi có bằng chứng giao thức. Cần một trong hai:
- `resources/app.asar` (hoặc thư mục cài) của Zalo PC trên máy người dùng để phân tích tĩnh các worker chunk; hoặc mở mạng cho `zalo.me`, `*.zadn.vn` để tải bộ cài;
- bản ghi (trace) một lần đồng bộ thật.

Có rủi ro: dữ liệu mã hoá đầu cuối, xác nhận thiết bị tin cậy, khả năng tài khoản bị đánh dấu khi client lạ gọi luồng này. Cần người dùng quyết định trước khi làm.

## 7. Cách kiểm chứng Bước 1 với tài khoản thật

1. Mở QTzalo, chờ đồng bộ xong, rồi tắt.
2. Trong lúc tắt, nhận khoảng 20 tin 1-1 và 20 tin nhóm trên điện thoại, và tự gửi vài tin từ điện thoại.
3. Mở lại QTzalo (chạy với `ZCALL_WS_TRACE=1 --dev`): tất cả tin phải hiện đúng thứ tự, không trùng lặp. Log phải có các đợt `more=1` cho tới `more=0`.
4. Lặp lại với thời gian tắt 1 ngày và 7 ngày; ghi lại lúc nào máy chủ trả `resetLastActionId`.
