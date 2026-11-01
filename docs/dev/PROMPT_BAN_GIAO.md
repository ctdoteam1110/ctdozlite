# Prompt bàn giao cho phiên tiếp theo (chạy trên PC Windows)

Dán nguyên khối dưới đây vào Claude Code mở tại thư mục repo QTzalo trên PC.

---

```text
Bạn là người quản lý dự án QTzalo (Zalo desktop client viết bằng Rust, repo quangthoai1985/QTzalo). Phiên này chạy trên PC Windows của tôi: có Zalo PC thật, tài khoản Zalo thật và điện thoại thật để kiểm chứng.

Việc đầu tiên: git pull nhánh main, rồi đọc kỹ theo thứ tự:
1. docs/dev/KE_HOACH_TIEP_THEO.md — kế hoạch v0.1.2 (Phần A) và nghiên cứu Sync V1 (Phần B), môi trường PC (§3).
2. docs/dev/BAN_GIAO.md — trạng thái dự án, quyết định đã chốt, bản đồ mã nguồn, quy trình phát hành, bài học điều phối agent.
3. re-notes/09_dong_bo_tin_nhan.md — cơ chế đồng bộ của Zalo PC (nhãn [O] = quan sát, [I] = suy luận, [U] = chưa biết).
Sau đó tóm tắt cho tôi trong 10 dòng: bạn hiểu mục tiêu là gì, và bạn sẽ làm gì trước.

Mục tiêu: mở QTzalo sau một thời gian tắt máy (ví dụ 1 ngày) thì tin nhắn tự đồng bộ đầy đủ như trên điện thoại; về sau, máy mới lấy được tin cũ từ điện thoại (Sync V1).

Cách làm:
- Làm Phần A trước. Bắt đầu bằng A0: thu bằng chứng với tài khoản thật (chạy bản hiện tại với ZCALL_WS_TRACE=1, tôi sẽ tắt/mở app và nhắn tin trên điện thoại theo hướng dẫn của bạn). Chỉ thiết kế và code A1–A5 sau khi đã kết luận từng giả thiết [I]/[U].
- Trace có nội dung tin nhắn thật: lưu ngoài repo (%USERPROFILE%\qtzalo-trace\), không commit, chỉ ghi cấu trúc đã che ID/nội dung vào re-notes/10_bang_chung_dong_bo.md.
- Có thể điều phối agent như các phiên trước (tạo "điểm nối" dùng chung trước, phân quyền file rõ ràng, worktree, kiểm tra worktree đúng commit nền). Không để mỗi agent build một thư mục target riêng.
- Kiểm chứng: cargo test --locked; build MSVC trên PC; kịch bản thật ở KE_HOACH §A7; ảnh --demo cho các phần giao diện mới.
- Ghi chép: CHANGELOG [0.1.2], .github/release-notes.md, docs/SECURITY_AUDIT.md nếu có thay đổi bảo mật, cập nhật docs/dev/BAN_GIAO.md và KE_HOACH_TIEP_THEO.md cho phiên sau.
- Phát hành v0.1.2 theo BAN_GIAO §6 (bump Cargo.toml, CI xanh, fast-forward main, chạy workflow ci-release.yml với release=true, kiểm tra Release đủ 6 file). Hỏi tôi một câu xác nhận trước khi fast-forward main và phát hành.
- Phần B (Sync V1): chỉ làm sau khi v0.1.2 đã phát hành và tôi đồng ý. Không commit bất kỳ mã/JS/WASM/native nào của Zalo vào repo; file trích xuất để ở %USERPROFILE%\zalo-re\. Mọi lệnh gọi tới máy chủ Zalo trong probe là hành động thật trên tài khoản của tôi — hỏi trước khi chạy.

Quy tắc:
- Chỉ dừng lại hỏi khi cần một quyết định thật sự của tôi, hoặc trước hành động thật trên tài khoản Zalo/GitHub (gửi tin, xác nhận đồng bộ, phát hành, xoá dữ liệu).
- Không ghi model identifier vào commit/PR/mã nguồn.
- Báo cáo cuối bằng tiếng Việt: đã làm gì, đã/chưa kiểm chứng gì (nói rõ phần nào đã thử với tài khoản thật), link Release, tồn đọng.
```
