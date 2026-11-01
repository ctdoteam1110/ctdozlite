# 10 — Bằng chứng đồng bộ với tài khoản thật (A0)

Ngày 2026-11-01. Kiểm chứng các giả thiết **[I]/[U]** của `re-notes/09` trước khi viết A1–A5 (`docs/dev/KE_HOACH_TIEP_THEO.md` §A0).

- Máy: Windows 11, PC của người dùng, tài khoản Zalo thật, điện thoại thật. Zalo PC 26.10.20 tắt trong lúc đo.
- QTzalo nhánh `claude/v0.1.2`, bật trace cấu trúc (`ZCALL_WS_TRACE=1`, module `src/trace_shape.rs`: chỉ in tên trường, kiểu, số lượng, con trỏ; nội dung tin chỉ còn độ dài).
- Trace thô nằm ở `%USERPROFILE%\qtzalo-trace\` và **không commit**. Tài liệu này chỉ ghi cấu trúc: đã bỏ ID người dùng/nhóm, tên, nội dung; giá trị con trỏ thay bằng ký hiệu (`L15`, `L16`…).
- Nhãn: **[O]** quan sát trong trace; **[I]** suy luận; **[U]** chưa biết.

## Lượt 1 — đăng nhập QR lần đầu trên máy này (23:00–23:02)

Bối cảnh: QTzalo chưa từng đăng nhập trên máy này (`%APPDATA%\QTzalo` không có `session.json`). Quét QR lúc 23:00:31, chạy khoảng 80 giây, dừng tiến trình lúc 23:01:56.

> Lưu ý vận hành: nút đóng cửa sổ của QTzalo chỉ **thu nhỏ** app (`src/gui.rs:1411`). Muốn tắt hẳn phải bấm Ctrl+Q, Shift+bấm nút đóng, hoặc chuột phải vào nút đóng.

### 1. AUTHEN (socket) [O]

```
data.qCmds      = 510_1, 511_1, 515_0, 517_0, 518_0
data.ctrl_qCmds = 603_1, 610_1, 611_1
data.bk_cmds    = { 510_1, 511_1, 603_1, 610_1, 611_1 }   // giá trị: chuỗi 5 ký tự
```

- **Không có** `510_0`, `511_0`, `513_0`, `713_0`. → Bác bỏ giả thiết của KE_HOACH A0.3 về danh sách queue.
- Có `515_0` và `517_0`. Theo tài liệu Zalo PC 26.10.20, đây là queue E2EE của **web** (`offline_msg_e2ee_web = 515/715`, `offline_session_e2ee_web = 517`); PC dùng `513/713` và `516`. → **[I]** máy chủ xếp phiên QTzalo (`zpw_type=30`) vào nhóm web khi xét E2EE.
- `bk_cmds` là trường chưa có trong tài liệu nào. Ý nghĩa **[U]**.

### 2. Kéo queue socket với `{first: true, lastId: 1}` [O]

| Queue | Số tin | `more` | `lastActionId` | Ghi chú |
|---|---|---|---|---|
| `510_1` | 1 (`webchat`) | 0 | `L15` | tin đến lúc đăng nhập |
| `511_1` | 0 | 0 | `L16` | |
| `515_0` | 0 | 0 | trùng `611_1` | có trường `eesession: []` |
| `517_0` | 0 | 0 | `"1"` | |
| `518_0` | 0 | 0 | — | không có `lastActionId`/`queueStatus` |
| `603_1` | 2 controls | 0 | | `transfer_after_login`, xem §5 |
| `610_1`, `611_1` | 0 | 0 | | |

- Mỗi phản hồi có `queueStatus[q] = {lastId, ids, evict}`. **Không** phản hồi nào có `isLastQueue` hay `resetLastActionId`.
- Phiên mới thì `lastId: 1` chỉ trả những gì phát sinh từ lúc đăng nhập. **[I]** hàng đợi offline gắn với phiên/thiết bị và bắt đầu từ lúc đăng nhập. Lượt 2 kiểm tra hàng đợi có giữ tin trong lúc app tắt hay không.

### 3. Long-poll `getChatFromPolling` [O]

- Lần đầu (cursor `1,1`, queue `1,2,3`): `msgs: []`. Máy chủ **không** trả tin cũ, chỉ `clearUnreads` (3), `delivereds` (1) và một `lastActionId` mới.
- `queueStatus` trả về các queue `1, 2, 3, 7, 8, 15, 16` dù client chỉ gửi `1, 2, 3`. Queue `1, 7, 8` có trường `evict`; `2, 3, 15, 16` thì không.
- `queueStatus["15"].lastId` trùng `lastActionId` của socket `510_1`; `queueStatus["16"].lastId` trùng `511_1`. → **Xác nhận** `queue_actionid_map` cho biến thể `_1`: `510_1 ↔ 15`, `511_1 ↔ 16`.
- Vì phiên này không có `511_0`, queue long-poll `"6"` không áp dụng. Biến thử `QTZALO_LP_QUEUES` nếu dùng thì phải là `15/16` (chú thích cũ trong `src/chat.rs` ghi là bị từ chối) **[U]**.
- Đợt 2 trả đúng 1 tin, cũng là tin vừa đến qua socket `510_1` (cùng `actionId`). Một tin có thể đi theo cả hai đường, nên phải chống trùng theo `msgId`.
- Trường của một tin thường: `actionId, at, cliMsgId, cmd (122 trên long-poll, 510 trên socket), content, dName, idTo, msgId, msgType, notify, paramsExt, propertyExt, quote, st, status, ts, ttl, uidFrom, …`. Tin thường **không** có `e2eeStatus`.
- Long-poll điều khiển (`lp_control`): `queueStatus` gồm `4, 5, 14, 17, 18, 19, 20`. `603_1 ↔ 19` và `610_1 ↔ 18` khớp `queue_ctrl_actionid_map`.

### 4. `get-last-msgs` và lịch sử nhóm [O]

- `get-last-msgs` với `{}`: `msgs: []`. Chỉ có `clearUnreads` của 137 hội thoại (79 1-1, 58 nhóm) và `clearUnreadsReact` (43). Không có tin xem trước.
- `/api/cm/getrecentv2` cho 60 lượt gọi nhóm: **tất cả 0 tin**, `hasMore = 0`. Phản hồi có các cờ `isFiltered`, `isFilteredByPhase`, `isFilteredByTimeJoin`, `isOld`, `tsJoinGroup` (lượt 1 chưa in giá trị; trace lượt 2 đã bổ sung).
- "Cloud của tôi": 50 tin, `hasMore = 1`, trải từ 03/2026 đến 10/2026.
- **[I]** Máy chủ lọc lịch sử nhóm theo thiết bị/thời điểm đăng nhập lần đầu, khớp chuỗi giao diện Zalo "Zalo chỉ hiển thị tin nhắn từ sau lần đăng nhập đầu tiên trên thiết bị này". → Với **thiết bị mới**, bác bỏ giả thiết "nhóm luôn đủ nhờ `getrecentv2`" (`re-notes/09` §1, cơ chế B).
- DB QTzalo sau lượt 1: 51 tin trong 2 hội thoại (50 tin Cloud của tôi + 1 tin 1-1 mới), **0 tin nhóm**.

### 5. Sự kiện `transfer_after_login` [O]

Queue điều khiển `603_1` (và queue `4` của `lp_control`) chứa 2 control giống nhau:

```
content: {
  act:      "transfer_after_login",
  act_type: "transfer_sync2",
  data:     "<chuỗi JSON> { imei: <69 ký tự>, pc: <số>, temp_key: <64 ký tự> }",
  ts:       <≈ thời điểm xác nhận QR>
}
```

- Khớp mô tả Zalo PC 26.10.20 §5.5: client lọc sự kiện "transfer after login", đọc `data.temp_key`, rồi phát `START_AUTO_SYNC` kèm `tempKey`.
- `act_type = "transfer_sync2"` → **[I]** khoá này chủ yếu dành cho **Sync v2** (tự đồng bộ sau đăng nhập). Tài liệu PC §6.5 cho thấy `pull_mobile_msg` (Sync v1) cũng nhận tham số `temp_key`.
- `temp_key` dài 64 ký tự, tức 32 byte dạng hex. Độ dài này khớp khoá transfer 32 byte mà điện thoại sinh khi xác nhận đăng nhập PC (tài liệu Android §8.1, `KEY_VALIDATE_TRANSFER_MSG_FROM_CONFIRM_LOGIN_PCWEB_<uid>`).
- Sự kiện chỉ có ngay sau khi quét QR. QTzalo hiện bỏ qua nó (chỉ lọc `act_type == "voip"`). Hạn dùng của `temp_key` **[U]**.

### 6. Khác [O]

- Đồng hồ máy chậm hơn máy chủ khoảng 69 giây (so `client_time` của `reqqr` với `server_time` của `getServerInfo`). Việc phát hiện khoảng thiếu (A3) phải dùng `ts` của máy chủ, không dùng giờ máy.
- Không thấy `533 HAS_MSG` hay `534 PUSH_MISS_MSG`.
- Tin đẩy trực tiếp trong lúc mở: `504` (xoá chưa đọc), `612` (reaction), `621` (Cloud cần xác minh).

## Lượt 2 — mở lại sau thời gian tắt

_Chờ thực hiện._

## Kết luận theo giả thiết

| Giả thiết (nguồn) | Kết luận |
|---|---|
| AUTHEN liệt kê `510_0/511_0/513_0/713_0` (KE_HOACH A0.3) | **Bác bỏ**: phiên QTzalo nhận `510_1, 511_1, 515_0, 517_0, 518_0` |
| `queue_actionid_map` dùng chung con trỏ socket ↔ long-poll (09 §2.1) | **Xác nhận** cho `510_1 ↔ 15`, `511_1 ↔ 16` |
| Long-poll với cursor `1` trả lại tin cũ (09 §5) | **Bác bỏ** với phiên mới: chỉ trả con trỏ |
| Nhóm đủ lịch sử nhờ `getrecentv2` (09 §1 B) | **Bác bỏ** với thiết bị mới: 0 tin, có cờ lọc |
| Có `resetLastActionId` / `evict = true` | Chưa rõ (lượt 1 không có) |
| Hàng đợi giữ tin trong lúc app tắt; `lastId: 1` lấy lại được | Chưa rõ (lượt 2) |
| Tin E2EE đến qua `513/713` | **Bác bỏ** queue: phiên này dùng `515/517`. Có tin E2EE thật hay không: chưa rõ |
| PC nhận `temp_key` sau đăng nhập QR (Phần B) | **Xác nhận**: `transfer_after_login` / `transfer_sync2` trong `603_1` |
