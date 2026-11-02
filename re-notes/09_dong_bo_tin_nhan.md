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

## 8. Đối chiếu với tài liệu "Đồng bộ & Sao lưu tin nhắn trên Zalo PC" (Zalo PC 26.10.20)

Người dùng cung cấp ngày 2026-11-01 một tài liệu phân tích Zalo PC 26.10.20, quét 143 file JS cùng các module WASM/native. Tài liệu không được commit vào repo; dưới đây chỉ ghi những điểm đã đối chiếu và hệ quả cho QTzalo.

**Khớp với §2–§4 ở trên:** `queue_actionid_map`, `evict_broadcast_queues`, máy trạng thái transfer (`SYNC_DB_STATE`) và `missing_message_range` đều trùng. Khác biệt nhỏ: tài liệu ghi `offline_timeout = 48.000 ms`, bundle cũ trong repo ghi `48e4` (480.000 ms). Có thể do khác phiên bản, hoặc đọc nhầm số mũ.

**Điểm mới, dùng được ngay cho Bước 1:**
- Kéo bù offline theo từng trang cho tới cờ `isLastQueue`. Mỗi queue `<cmd>_<subCmd>` giữ `lastActionId` và `queueLastMsgIds`, và các con trỏ này được lưu bền.
- `queueStatus[q].evict = true` nghĩa là máy chủ đã đẩy bỏ một phần hàng đợi (overflow). Zalo phát `QUEUE_OVERFLOW` và gợi ý đồng bộ từ điện thoại (nguồn `OVERFLOW`). → QTzalo nên coi `evict` giống `resetLastActionId`: ghi lại khoảng thiếu và hiện banner.
- Tải bù tin bị thiếu: `532 VERIFY_MSG` (gửi danh sách `actionId`), `534 PUSH_MISS_MSG`, `570 GET_MSG` (`max_msg_get = 20`, `timeout_retry_miss = 30 s`).
- ACK hàng đợi: `ackOfflineQueue` (`DELETE_QUEUE_ACK_ONEONE/GROUP`) để máy chủ xoá phần đã giao. **Chỉ làm sau khi có trace thật**: chưa rõ việc không ACK có làm hàng đợi của QTzalo bị evict sớm hay không, và ACK sai thì mất tin không lấy lại được.
- Lịch sử: thêm `/api/cm/rgetv2` (12508) và `/api/cm/mget` (12503) bên cạnh `getoldv2`/`getrecentv2`.
- Gắn nhãn nguồn tin (`MsgSources`: `SOCKET=10`, `POLL=1`, `MISS_MSG=11`, `CLOUD=3`, `SYNC_MOBILE_*=2/7`) để biết tin đến từ đâu khi gỡ lỗi.

**Điểm mới quan trọng — E2EE:**
- Tin 1-1/nhóm có thể được mã hoá đầu cuối bằng Signal (libsignal, SenderKey cho nhóm). Hàng đợi offline E2EE: PC dùng `513`/`713`, web dùng `515`/`715`; `516`/`517` là phiên khởi tạo.
- QTzalo đang kéo `513/514/515/517/518` (`src/socket.rs:266-282`) nhưng **không có phần giải mã Signal**. Nội dung các tin E2EE vì thế không đọc được.
- Muốn đọc E2EE thì phải đăng ký QTzalo làm thiết bị E2EE (prekey/bundle, `INIT_SESSION`…) và có thể cần liên kết thiết bị tin cậy (`trusted_device_protocol`). Đây là việc lớn và nhạy cảm về bảo mật.
- Việc trước mắt: **nhận diện** tin E2EE (`e2eeStatus` hoặc cmd của queue) và hiện chỗ giữ chỗ "Tin nhắn mã hoá đầu cuối — xem trên điện thoại" thay vì bỏ qua hay hiện rác.

**Đồng bộ từ điện thoại:**
- **Sync v2** (WS `590/591/592` + task `{syncId, ussidx, ik, payload}`): dữ liệu được mã hoá bằng Sync2 (Ed25519, ChaCha20, phiên kiểu Signal, zstd + protobuf), chạy trong WASM `libzproto_wasm`. Tài liệu tự ghi AEAD/KDF là "chưa xác định" → **chưa làm được**.
- **Sync v1 / transfer** (HTTP `GET /api/message/pull_mobile_msg` cmd 12000 với `public_key` RSA-2048, `from_seq_id`, `min_seq_id`, `temp_key`):
  - Điện thoại tạo file backup; PC nhận sự kiện `BACKUP_SUCCESS {url, checksum, encrypted_key, db_format}`.
  - PC giải `encrypted_key` bằng RSA PKCS#1, rồi giải nén và giải mã bằng native `db-cross-v4 decompressAndDecryptDb(input, output, password)`. Bước khôi phục dùng `password: uin`.
  - Phần RSA là chuẩn. **Ẩn số còn lại là định dạng của `db-cross-v4`** (module native). Đây là hướng khả thi nhất nếu có module native (`resources/` trong thư mục cài Zalo PC) để phân tích.
  - Hạn chế: điện thoại phải mở Zalo và người dùng phải xác nhận; có thể bị từ chối nếu client không được nhận diện là Zalo PC.

## 9. Đối chiếu với tài liệu phía điện thoại (Zalo Android 26.10.02) và bằng chứng thật

Người dùng cung cấp ngày 2026-11-01 tài liệu "Đồng bộ giữa Zalo PC và ứng dụng Android" (phân tích Zalo Android `26.10.02`, đối chiếu PC `26.10.20`). Tài liệu không được commit. Bằng chứng với tài khoản thật nằm ở `re-notes/10_bang_chung_dong_bo.md`.

### 9.1 Điểm mới từ phía điện thoại

- PC và điện thoại không nói chuyện trực tiếp; mọi thứ đi qua máy chủ.
- **Sync v2:**
  - PC gửi WS `590`, máy chủ đẩy `init_session` xuống điện thoại.
  - Điện thoại báo trạng thái và upload từng gói lên `transfersync.zaloapp.com/api/transfer-sync-v2/{status,error,upload}`; xác nhận bằng `/confirmsync`.
  - PC đã được người dùng xác nhận một lần thì được "tin" 1 năm (bản ghi theo `uid + deviceId`, tra theo `ik`): các lần sau `BYPASS_CONFIRM`, không hỏi lại.
- **Sync v1 (transfer):**
  - Điện thoại nhận `{publicKey, fromSeqId, min_seq_id, temp_key, pc-name}` và hiện `PcRequestTransferView`.
  - Người dùng xác nhận qua `POST trans-bin.zaloapp.com/upload/confirmtransfer` (`user_action`: 1 đồng ý, 0 bỏ qua, 2 tự huỷ vì đang bận sync v2 hoặc khôi phục).
  - Pipeline của điện thoại: `EXPORT → COMPRESS → ENCRYPT → UPLOAD`.
  - Không có bypass như v2: mỗi lần đều cần xác nhận, trừ luồng ngay sau đăng nhập dùng `temp_key`.
- **Khoá transfer 32 byte** được điện thoại sinh đúng lúc người dùng xác nhận đăng nhập PC bằng QR (`KEY_VALIDATE_TRANSFER_MSG_FROM_CONFIRM_LOGIN_PCWEB_<uid>`).
- **Định dạng trung gian "Cross DB":** bảng `chats(userId, ownerId, ownerType, fromId, fromName, cliMsgId, globalMsgId, ts, ttl, type, msg, attach, sequenseId)` và `threads` (+ `offset`, `sync_status`). Điện thoại tạo bằng `BackupRestoreUtils.convertAndroidDbToCross`; PC mở bằng `db-cross-v4`.
- Sao lưu PC → điện thoại (`bkpc2mb.zaloapp.com`) và sao lưu Drive của điện thoại ("SMLBackup") là chiều khác, không liên quan tới việc lấy tin về PC.

### 9.2 Bằng chứng thật liên quan (`re-notes/10`)

- **[O]** Ngay sau khi quét QR, queue điều khiển `603_1` có `act: "transfer_after_login"`, `act_type: "transfer_sync2"`, `data: {imei, pc, temp_key (64 ký tự hex = 32 byte)}`. Khớp với khoá transfer ở trên; `act_type` cho thấy nó dành cho Sync v2 trước hết.
- **[O]** Tin 1-1 và tin nhóm sau lần đăng nhập đều về qua hàng đợi (`510_1`, `511_1`). Trên máy mới, lịch sử **trước** lần đăng nhập đầu tiên không lấy được: `getrecentv2` của nhóm trả 0 tin (cờ `isFiltered`). Cả 61 nhóm đều có `e2ee = 0`.

### 9.3 Đánh giá lại hướng Sync V1

Giữ **Sync V1** làm hướng chính cho Phần B, với các điều chỉnh sau:

1. **Thu hẹp ẩn số.** Dữ liệu sau giải mã gần như đã biết cấu trúc (Cross DB `chats`/`threads`), nên chỉ còn lớp nén + mã hoá của `db-cross-v4`.
2. **Thêm nguồn phân tích thứ hai.** Phân tích phía mã hoá trong APK Android: các lớp Java `cf1.*`, `ve1.g`, `xe1.g`, `BackupRestoreUtils`. Nếu bước nén/mã hoá viết bằng Java thì đọc bằng jadx nhanh hơn nhiều so với Ghidra trên `.node`.
3. **Tận dụng `temp_key`.** Lưu `temp_key` trong RAM ngay sau đăng nhập QR để thử luồng "transfer after login" mà không cần xác nhận thêm (hạn dùng **[U]**).
4. **Giá trị thực của V1:** lấy lịch sử **trước** lần đăng nhập đầu tiên trên máy mới, cả 1-1 lẫn nhóm, vì snapshot được xuất từ DB của điện thoại. Việc bắt kịp sau khi tắt máy đã do Phần A (v0.1.2) giải quyết.
5. **Sync v2 vẫn để sau.** Ưu điểm của v2 là bypass xác nhận và đồng bộ tăng dần theo khoảng. Nhược điểm: cần cài lại mật mã Sync2 (curve25519/Ed25519 + ChaCha20, phiên kiểu Signal, AEAD/KDF chưa rõ) và cần định danh E2EE của PC.

**Phương án dự phòng:** nhập thẳng từ DB của Zalo PC trên cùng máy (`%APPDATA%\ZaloData`, SQLCipher với khoá suy ra từ UIN, cộng một lớp mã hoá cột AES-CBC). Không cần gọi máy chủ, nhưng chỉ dùng được khi máy đã có Zalo PC, và cách suy ra khoá **[U]**.
