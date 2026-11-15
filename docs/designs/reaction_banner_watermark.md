# Chặn thông báo reaction cũ bằng mốc id đã báo

*   **Author:** ctdozlite maintainer (drafted with Claude Code)
*   **Status:** In Review. Baseline: commit `1ff6ee2`. Bản cài đặt đi kèm trong
    cùng PR (`src/reactions.rs`, `src/gui.rs`).
*   **Date:** 2026-11-15

--------------------------------------------------------------------------------

## Overview

Mỗi lần mở app, ctdozlite hiện lại thông báo hệ điều hành cho những reaction đã
báo ở lần chạy trước, ví dụ "<Tên> đã thả ❤️ vào tin nhắn của bạn: <nội dung>".
Spec này đề xuất lưu, theo từng tài khoản, **id lớn nhất của reaction đã hiện
thông báo**, và chỉ hiện thông báo cho reaction có id lớn hơn mốc đó. Server vẫn
gửi lại reaction như hiện nay, vì phần đếm số reaction cần dữ liệu đó.

## The Problem

### Server gửi lại reaction ở mỗi lần kết nối

*   **Thuật ngữ:**
    *   `uin` là id tài khoản Zalo.
    *   Tên hàng đợi có dạng `"{cmd}_{subCmd}"`, ví dụ `610_1` là cmd 610, sub 1.
    *   Hàng đợi điều khiển (`ctrl_qCmds`) chứa hành động như reaction và mute.
*   **Cách kéo hàng đợi:** sau khi WebSocket xác thực (`cmd 1`), app kéo từng
    hàng đợi bằng một khung `pull` với `lastId: 1`
    ([`src/socket.rs`](../../src/socket.rs), `pull_frame`). Server trả về mọi hành
    động nó còn giữ, kèm con trỏ `queueStatus[q].lastId`. App không lưu con trỏ
    này, nên lần kết nối nào cũng nhận lại toàn bộ.
*   **Việc gửi lại này có chủ đích.** `ReactionInbox`
    ([`src/reactions.rs`](../../src/reactions.rs), chú thích trên
    `MessageReactionBase`) tính số reaction bằng
    `base + các sự kiện live khác nhau`. Lý do: lịch sử tin nhắn trả về số cũ, và
    `chat::merge_previews` ([`src/chat.rs`](../../src/chat.rs)) ghi đè bản tóm tắt
    bằng số đó. Hàng đợi được gửi lại là nguồn đúng duy nhất.

### Vì sao bị báo lại

*   `ReactionInbox::record_new` chống trùng theo id, nhưng chỉ trong RAM, nên mất
    khi tắt app.
*   Cờ `ReactionEvent::historical`, dùng để chặn banner reaction cũ trong handler
    `GuiEvent::ReactionEventsReceived` ([`src/gui.rs`](../../src/gui.rs)), luôn bị
    gán `false` khi parse. Vì vậy nó không chặn được gì.

### Bằng chứng

Ghi lại trên máy người dùng ngày 2026-11-15, chi tiết ở
[`re-notes/39_reaction_replay_banners.md`](../../re-notes/39_reaction_replay_banners.md).

*   **Server gửi lại reaction cũ.** App mở lúc khoảng 22:00. `610_1` trả về 5
    reaction có `ts` lúc 18:30, 20:49 (3 cái) và 21:44.
*   **4 thông báo cho 5 id.** Mở app 2 lần liên tiếp, lần nào `dbus-monitor` cũng
    bắt được đúng 4 thông báo giống nhau. Reaction thứ năm là 👍 của chính người
    dùng (`uidFrom: "0"`), nên không có banner.
*   **Id tăng dần, `ts` thì không.** Mỗi reaction có `msgId` bằng `actionId` của
    hàng đợi, ví dụ `8492232095093`. Id này tăng dần theo thứ tự server. `ts` thì
    không theo thứ tự đó: id `…107943` có `ts …261`, nhưng id lớn hơn `…109138`
    lại có `ts …188`.

Đọc hết tin nhắn không ngăn được việc báo lại, vì điểm đã đọc của tin nhắn và
việc chống trùng reaction là hai cơ chế tách biệt.

## Goals

*   Mở lại app không hiện thông báo cho reaction đã báo ở lần chạy trước. Ngoại
    lệ: lần chạy đầu tiên sau khi cập nhật, khi chưa có mốc (CUJ 3).
*   Reaction đến khi app tắt, hoặc khi tài khoản đang chạy nền, vẫn được báo
    **một lần** khi người dùng mở lại hoặc chuyển sang tài khoản đó.
*   Số reaction trên tin không đổi so với hiện nay: server vẫn gửi lại, và
    `ReactionInbox` không đổi.
*   Không đổi giao thức WebSocket.

## Non-Goals

*   Không lưu con trỏ hàng đợi (xem Alternative A).
*   Không xoá cờ `historical`. Nó vẫn là một cổng không dùng tới và có thể dọn
    sau.
*   Không đổi phần chống trùng hay báo thông báo **tin nhắn**.

## Critical User Journeys (CUJs)

### CUJ 1: Mở lại app không bị báo lại reaction cũ

*   **Given**: Lần chạy trước đã hiện banner cho reaction R (id `r`). Mốc đã lưu
    lớn hơn hoặc bằng `r`.
*   **When**: Người dùng tắt app rồi mở lại.
*   **Then**: Server gửi lại R. `ReactionInbox` dùng R để tính số reaction như
    trước. R có id ≤ mốc, nên không có banner và không cộng số reaction chưa xem.

### CUJ 2: Reaction đến khi app tắt hoặc tài khoản đang chạy nền

*   **Given**: Người khác thả reaction R2 (id > mốc) vào tin của người dùng trong
    lúc app tắt, hoặc trong lúc tài khoản chạy nền (socket nền bỏ qua reaction).
*   **When**: Người dùng mở app, hoặc chuyển sang tài khoản đó.
*   **Then**: R2 có trong lần gửi lại và có id > mốc, nên hiện banner một lần.
    Mốc tiến lên `id(R2)`. Mở lại lần nữa thì không báo R2.

### CUJ 3: Lần chạy đầu sau khi cập nhật, hoặc file mốc hỏng

*   **Given**: Không có `accounts/<uin>/reaction_banner_mark`, hoặc file không đọc
    được.
*   **When**: App nhận lần gửi lại đầu tiên.
*   **Then**: Mốc là 0, nên reaction cũ hiện banner **một lần**, giống hôm nay. Mốc
    được lưu ngay sau batch đó, nên lần sau không báo lại.

### CUJ 4: Reaction đến khi app đang mở

*   **Given**: App đang mở, có người thả reaction R3.
*   **When**: Sự kiện live đến, và lần mở app sau server gửi lại R3.
*   **Then**: Lúc đến, R3 hiện banner và mốc tiến lên. Khi được gửi lại, R3 không
    hiện banner, với điều kiện sự kiện live mang cùng id (OQ1).

## Technical Plan (Architecture)

```text
 WS 610_1/611_1 (pull, replayed every start) ─┐
 WS live reaction push ───────────────────────┴─► parse_reaction_events
                                                      │  ReactionEvent.event_id = msgId (= actionId)
 GuiEvent::ReactionEventsReceived                     ▼
   accepted = ReactionInbox::record_new(events)   (counts: unchanged)
   mark = load once per uin from accounts/<uin>/reaction_banner_mark
   banner / reaction-unread only if  seq(ev) > mark   (no numeric seq → not suppressed)
   mark = max(mark, seq of accepted events) → saved on a background thread
```

*   **Phạm vi:** chỉ đổi cổng banner và cổng đếm reaction chưa xem trong
    `src/gui.rs`. Các phần sau không đổi: socket, `ReactionInbox`, mute, seen,
    VoIP, mobile-sync, file-done.
*   **Id dùng làm mốc:** `ReactionEvent::event_id`, giá trị `msgId` mà app đã
    parse sẵn. Đây là số thứ tự của server, tăng dần (xem Bằng chứng). Sự kiện
    không có id dạng số thì không bị chặn, tức giữ hành vi hiện tại.
*   **Mốc tiến qua mọi sự kiện được chấp nhận**, kể cả reaction của chính mình
    hay lệnh gỡ reaction. Các sự kiện này không hiện banner, nhưng cùng chuỗi số
    nên không làm sai mốc.
*   **Nơi lưu:** một file văn bản chứa một số, `accounts/<uin>/reaction_banner_mark`.
    Đường dẫn chỉ được dựng khi `account_manager::is_safe_uin(uin)` đúng. Thư mục
    `accounts/<uin>/` bị xoá cùng tài khoản (`delete_saved_account`). File được
    ghi bằng `data_dir::write_private_atomic`.
*   **Luồng:** đọc một lần cho mỗi `uin`, ở lần nhận reaction đầu tiên. File chỉ
    vài byte, giống cách `settings.save()` đang chạy trên luồng UI. Ghi chạy
    trong `std::thread::spawn`, giống `set_thread_read`.

## Open Questions

*   **OQ1: Sự kiện reaction live (`cmd 601` push) có `msgId` giống bản server gửi
    lại không?**
    *   Chưa bắt được khung live.
    *   Nếu "không": reaction nhận live sẽ bị báo lại một lần ở lần mở sau. Khi
        đó bước tiếp theo là lấy `actionId` từ khung live, nếu có.
    *   Kiểm chứng: Verification bước 3.
*   **OQ2: các hàng đợi mang reaction có dùng chung một chuỗi số không?**
    *   Lần bắt ngày 2026-11-15: `actionId` của `611_1` (`8491877875858`,
        `8491899057416`) cùng dải với `610_1` (`849…`).
    *   `603_1` dùng dải riêng (`1273…`), nhưng không chứa reaction nào.
    *   Nếu sau này `603_1` mang reaction, id của nó nhỏ hơn mốc và banner sẽ bị
        chặn nhầm. Khi đó cần một mốc riêng cho mỗi dải.

## Risks & Mitigations

| Rủi ro | Hậu quả | Giảm thiểu |
| :----- | :------ | :--------- |
| Sự kiện không có `msgId` dạng số | Không chặn được, báo lại như hôm nay | Không chặn khi thiếu id, nên không bao giờ mất banner |
| Id live khác id gửi lại (OQ1) | Reaction live bị báo lại một lần ở lần mở sau | Verification bước 3. Nếu xảy ra thì dùng `actionId` |
| Reaction mới đến sau một reaction có id lớn hơn (ví dụ `611_1` trả về sau `610_1`, hoặc live xen với lần gửi lại) | Mất banner cho reaction đó. Số reaction vẫn đúng | Chấp nhận: trong một batch, mọi sự kiện đều được xét với mốc cũ. Chỉ khi hai nguồn giao nhau mới xảy ra (OQ2) |
| Hai luồng ghi chạy lệch thứ tự | Mốc cũ hơn được ghi sau cùng, báo lại một lần | Chấp nhận, có ghi chú `shortcut:` trong code |
| File mốc hỏng hoặc mất | Báo lại một lần (CUJ 3) | Đọc lỗi thì coi là 0 |
| App chết trước khi ghi mốc | Báo lại batch đó một lần | Chấp nhận. Không mất dữ liệu, vì server vẫn gửi lại |

## Alternatives Considered

### Alternative A: Lưu con trỏ hàng đợi (`lastId`) và gửi lại khi kết nối

*   **Pros:**
    *   Server không gửi lại hành động cũ.
    *   Đã kiểm chứng ngày 2026-11-15: server chấp nhận `lastId` đã lưu với
        `first: true` và trả `reacts: []`.
*   **Cons:**
    *   **Làm sai số reaction:** `ReactionInbox` cần lần gửi lại, vì lịch sử trả
        về số cũ.
    *   **Mất reaction của tài khoản chạy nền:** socket nền bỏ qua khung điều khiển
        (`background_accounts.rs`, `is_chat_ish`) nhưng vẫn tiến con trỏ.
    *   **Không tự phục hồi:** nếu server từ chối hoặc evict con trỏ, socket kết
        nối lại mãi với cùng `lastId`. Tin nhắn realtime dùng chung socket đó.
    *   **Mở thêm khoảng mất dữ liệu:** `tx.send` chỉ xếp hàng sự kiện.
    *   **Nghĩa của con trỏ chưa rõ khi `more == 1`:** phân trang ưu tiên
        `lastActionId`, chưa có capture để biết.
    *   **Ảnh hưởng mọi consumer khác** của `603_1`/`610_1`/`611_1`.
*   **Reason for Rejection:** bỏ lần gửi lại mà phần đếm reaction đang dựa vào,
    và mỗi rủi ro trên cần thêm code (kiểm role, đường phục hồi, kênh commit, lưu
    inbox). Cách đề xuất giữ lần gửi lại, nên không có các rủi ro này.

### Alternative B: Mốc theo `ts` của reaction

*   **Pros:** giống cách đề xuất, không đổi giao thức.
*   **Cons:** `ts` không theo thứ tự server (xem Bằng chứng), nên reaction có `ts`
    nhỏ hơn mốc sẽ bị bỏ sót banner.
*   **Reason for Rejection:** id server tăng dần, còn `ts` thì không.

### Alternative C: Gán `historical = true` cho gói pull đầu tiên của mỗi kết nối

*   **Pros:** khoảng 5 dòng, dùng lại cổng đã có.
*   **Cons:** reaction đến khi app tắt cũng nằm trong gói đầu, nên mất banner. Mục
    tiêu 2 không đạt.
*   **Reason for Rejection:** vi phạm Goal 2.

### Alternative D: Lưu toàn bộ `ReactionInbox` (danh sách id đã thấy) xuống DB

*   **Pros:** chống trùng chính xác theo id.
*   **Cons:**
    *   Cần bảng mới và migration.
    *   Tập id tăng mãi, nên cần cơ chế dọn.
    *   Khoảng 100 dòng trở lên.
*   **Reason for Rejection:** một số duy nhất là đủ, vì id tăng dần.

## Non-Functional Requirements

*   **Security & Privacy:** file mốc chỉ chứa một số, không chứa nội dung hay
    khoá. Ghi với quyền private. `uin` được kiểm `is_safe_uin` trước khi dựng
    đường dẫn. `accounts/` đã được `.gitignore` loại trừ.
*   **Performance:** đọc một file vài byte cho mỗi tài khoản. Ghi khi mốc tiến,
    chạy ngoài luồng UI.
*   **Rollout & Rollback:** chạy binary cũ thì quay về hành vi cũ. Người dùng sẽ
    lại thấy reaction cũ được báo mỗi lần mở app. Xoá file mốc thì báo lại một lần
    rồi tự lưu lại.

## Additional Resources

*   [`src/reactions.rs`](../../src/reactions.rs): parse reaction, `ReactionInbox`,
    hàm mốc.
*   [`src/gui.rs`](../../src/gui.rs): handler `GuiEvent::ReactionEventsReceived`.
*   [`src/socket.rs`](../../src/socket.rs): pull hàng đợi (không đổi).
*   [`src/background_accounts.rs`](../../src/background_accounts.rs): socket nền
    (không đổi).
*   [`re-notes/39_reaction_replay_banners.md`](../../re-notes/39_reaction_replay_banners.md):
    capture.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

### Target Files to Create/Modify

#### 1. `src/reactions.rs` (Modify)

*   Thêm `banner_seq(&ReactionEvent) -> Option<u64>`: `event_id` dạng số và > 0.
*   Thêm `already_bannered(&ReactionEvent, mark) -> bool`: `seq <= mark`. Không
    có id thì trả `false`.
*   Thêm `load_banner_mark(uin) -> u64` và `save_banner_mark(uin, mark)`. Hai hàm
    dùng file `account_dir(uin)/reaction_banner_mark`, chỉ khi `is_safe_uin`. Đọc
    lỗi thì trả 0. Ghi bằng `write_private_atomic`.
*   Thêm test `banner_gate_uses_server_seq_not_ts` với id và `ts` thật từ capture.

#### 2. `src/gui.rs` (Modify)

*   Thêm field `reaction_banner_mark: (String, u64)`, giá trị đầu là `(String::new(), 0)`.
*   Trong handler `GuiEvent::ReactionEventsReceived`:
    *   nạp lại mốc khi `uin` khác với `uin` đang giữ;
    *   cả vòng banner và vòng đếm reaction chưa xem đều `continue` khi
        `already_bannered`;
    *   sau hai vòng, nếu id lớn nhất trong `accepted` > mốc thì cập nhật field
        và ghi file trong `std::thread::spawn`.

#### 3. `re-notes/39_reaction_replay_banners.md` (Create)

*   Ghi capture `610_1` đã ẩn uid/nội dung, quan hệ `msgId`/`actionId`/`ts`, và
    kết quả kiểm chứng.

### Verification

1.  `cargo test --release --locked --lib reactions::` pass.
2.  Chạy binary mới từ thư mục dữ liệu của người dùng (`~`) với `dbus-monitor`
    theo dõi `Notify`.
    *   **Lần 1:** chưa có file mốc, có thể có banner reaction cũ (CUJ 3). Sau đó
        file `accounts/<uin>/reaction_banner_mark` xuất hiện.
    *   **Lần 2:** 0 banner reaction (CUJ 1). Số reaction trên các tin đó giống
        lần 1. Không phải đăng nhập lại.
3.  **OQ1:** khi app mở, nhờ người khác thả reaction mới vào tin của người dùng.
    Có 1 banner. Tắt, mở lại thì 0 banner. Chưa làm được nếu không có người thứ
    hai.
