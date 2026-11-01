window.CTD_GITHUB = {
  "fetchedAt": "2026-11-01T15:46:48.301Z",
  "repo": "ctdoteam1110/ctdozlite",
  "totalIssues": 30,
  "totalPrs": 24,
  "issues": [
    {
      "number": 56,
      "title": "app chiếm quá nhiều ram trong windows server 2019",
      "state": "CLOSED",
      "author": {
        "login": "heroes1412",
        "name": "heroes1412"
      },
      "createdAt": "2026-11-01T15:42:01Z",
      "body": "### Summary\n\n\nmình là ng dùng bình thường, k phải marketing hay gì mà nhiều group nhưng v0.0.2 thấy chiếm 400mb ram, 0.0.3 chiếm 2gb luôn\n\n### Reproduction\n\n1.\n2.\n3.\n\n\n### Area\n\nauth\n\n### Server error_code (if any)\n\n_No response_\n\n### Environment\n\n- OS / Version:\n- ctdozlite version:\n- Interface: GUI / TUI\n- Logs (redacted):\n\n\n### Secret check\n\n- [x] I confirm no session tokens, cookies, IMEI, phone numbers or raw captures are included.",
      "labels": []
    },
    {
      "number": 38,
      "title": "feat(<scope>): <summary> Hẹn giờ gửi tin nhắn (Schedule message)",
      "state": "OPEN",
      "author": {
        "login": "vuduc810",
        "name": "vuduc810"
      },
      "createdAt": "2026-11-01T11:16:06Z",
      "body": "### Summary\n\nỨng dụng hiện tại chưa hỗ trợ lập lịch gửi tin nhắn vào một thời điểm cụ thể trong tương lai. \n\nTính năng này giúp người dùng chủ động gửi tin nhắn công việc, đặt lịch nhắc nhở hoặc nhắn tin ngoài giờ làm việc mà không làm phiền người nhận ngay lập tức (tương tự như trải nghiệm hẹn giờ gửi trên app tin nhắn của Google trên Android).\n\n### Proposal\n\nThêm tính năng \"Hẹn giờ gửi tin nhắn\" (Schedule Send) tương tự app tin nhắn của Google trên Android:\n\n- Luồng thao tác: Nhấn giữ nút \"Gửi",
      "labels": []
    },
    {
      "number": 35,
      "title": "Bổ sung REST API, MCP",
      "state": "OPEN",
      "author": {
        "login": "congnvp",
        "name": "congnvp"
      },
      "createdAt": "2026-11-01T10:48:21Z",
      "body": "Mình đang rất kỳ vọng dự án này có thể phát triển mở các tính năng tích hợp công cụ thứ ba vào để quản lý tin nhắn, trả lời tự động... Và có phương thức trả dữ liệu/đồng bộ dữ liệu cho công cụ thứ ba để dễ quản lý và lưu trữ. ",
      "labels": []
    },
    {
      "number": 34,
      "title": "Rủi ro pháp lý về thương hiệu & bản quyền từ VNG và đề xuất hướng khắc phục",
      "state": "OPEN",
      "author": {
        "login": "OngAnBaNgan",
        "name": "Ông An Bà Ngân"
      },
      "createdAt": "2026-11-01T09:46:02Z",
      "body": "Chào tác giả và đội ngũ phát triển,\n\nTrước tiên, cảm ơn các bạn vì đã tạo ra một dự án mã nguồn mở rất thú vị và chất lượng bằng Rust. Tuy nhiên, với tư cách là một người quan tâm đến sự phát triển lâu dài của dự án, mình muốn đóng góp một vài ý kiến quan trọng liên quan đến **rủi ro pháp lý** từ phía VNG/Zalo. \n\nCác công ty công nghệ lớn như VNG thường có chính sách bảo vệ thương hiệu và Điều khoản sử dụng (ToS) rất khắt khe đối với các ứng dụng bên thứ ba (đặc biệt là hành vi reverse engineeri",
      "labels": []
    },
    {
      "number": 33,
      "title": "Lỗi phông chữ ở mục đăng nhập Zalo, không hiển thị trên tên/logo app ở chuyển tab",
      "state": "CLOSED",
      "author": {
        "login": "maiminhdung",
        "name": "maiminhdung"
      },
      "createdAt": "2026-11-01T09:32:42Z",
      "body": "Thông tin hệ điều hàng:\n+ OS: Nobara Linux 44 (Fedora 44)\n+ DE: GNOME 50.6\n+ WM: Mutter (Wayland)\n\nLỗi xảy ra ở phiên bản 0.0.2 của app.\n\n\n",
      "labels": []
    },
    {
      "number": 32,
      "title": "fix(ui): trạng thái online đang đặt cứng — mọi hội thoại 1-1 đều hiện chấm xanh",
      "state": "CLOSED",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T09:24:29Z",
      "body": "## Context\n- Báo cáo gốc: #31 (mục 3, macOS, bản đầu): *\"Giao diện luôn hiển thị mặc định những người dùng là đang online dù không hề biết state online của người đó\"*.\n- Cùng triệu chứng xuất hiện trong ảnh của #8 (`Nhóm đang hoạt động`) và trong ảnh #31 (`Chưa kết bạn trên Zalo` vẫn có chấm xanh).\n\n## Current behaviour\nApp chưa từng gọi bất kỳ API/field presence nào — toàn bộ chỉ báo online là hằng số trong code giao diện:\n\n- `src/gui/workspace.rs:804` — mọi hội thoại không phải nhóm (`!t.is_gr",
      "labels": []
    },
    {
      "number": 31,
      "title": "Issue trên MacOS version đầu",
      "state": "CLOSED",
      "author": {
        "login": "trungnt1000vn",
        "name": "Trung Nguyễn "
      },
      "createdAt": "2026-11-01T09:05:23Z",
      "body": "\n\n\n\nList các vấn đề : \n- Danh sách chat đang bị sai, hiển thị sai cả về mặt thời gian, thứ tự \n- Chưa sync được lịch sử tin nhắn, hoàn toàn không có gì dù đã đồng ý chuyển tin nhắn sang.\n- Giao diện luôn hiển thị mặc định những người dùng là đang online dù không hề biết state online của người đó\n- Nhóm đang hiển thị ID của nhóm chứ chưa hiển thị tên của nhóm là gì.\n- Lỗi hiển thị cho phần Setting",
      "labels": []
    },
    {
      "number": 27,
      "title": "task(net): try multiple host candidates per endpoint instead of one verified constant",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T07:44:25Z",
      "body": "## Context\n\n- Parent: #3 (item 2 — domain 404)\n- Follows: #22 (Fix A: hằng số đã verify thắng `zpw_service_map_v3`)\n- Blocked by: None\n- Nhãn đề xuất: `tech-debt` (repo chưa có nhãn này; account mở issue không có quyền `push` để tạo nhãn upstream)\n\n## Scope\n\nFix A khiến mỗi endpoint chỉ còn **một** host đã verify. Đúng cho lỗi 404 hiện tại, nhưng đổi lại: khi Zalo xoay host đã verify (`tt-chat3` → `tt-chat4`, `p5-msg-chat` → `p6-…`), client bám hằng số cũ và 404 cho tới khi có bản build mới. Đợt",
      "labels": []
    },
    {
      "number": 24,
      "title": "Lỗi (windows) và đề xuất tính năng",
      "state": "OPEN",
      "author": {
        "login": "DEWE-ex",
        "name": "DEWE-ex"
      },
      "createdAt": "2026-11-01T07:15:42Z",
      "body": "Lỗi:\n\n\n- Phần border khi thu nhỏ cửa sổ/upfile bị lỗi\n- Không có tin nhắn group\n- Files từ điện thoại lên pc bị chuyển thành link (từ laptop về điện thoại bình thường)\nĐề xuất tính năng (về cơ bản là mấy cái tính năng sẵn có của zalo thôi)\n- Xóa tin nhắn\n- Xóa cuộc trò truyện\n- Lưu file về máy\n- Ẩn tin nhắn =)))\n- Nhắc hẹn\nDaika cứ làm dần, cuối tuần sau em rảnh em test tiếp cho daika",
      "labels": []
    },
    {
      "number": 23,
      "title": "fix(ui): \"Đồng bộ tin nhắn\" only refreshes previews — rename, show real progress and errors",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T07:10:13Z",
      "body": "## Context\nParent: #3 (item 3 — \"Bấm nút đồng bộ chỉ có một toast notification hiện lên, không có pop-up tiến trình, và không có gì xảy ra\").\n\nTrùng: #10 (nội dung \"chưa thể đồng bộ toàn bộ tin nhắn\"), #8 (không đồng bộ được đoạn chat cũ).\n\n## Root cause\nNút gửi `WorkerCmd::RefreshThreads` (`src/gui/modals.rs:73`). Handler (`src/gui/worker.rs:609`) chỉ:\n- validate session + `sync_profile_me`\n- `get_friends`\n- `poll_new_messages` → ~50 tin gần nhất mỗi nhóm / Cloud\n\n**Không** tải lịch sử đầy đủ, ",
      "labels": []
    },
    {
      "number": 22,
      "title": "fix(net): zpw_service_map_v3 overrides verified domains — TUI friend list 404s on tt-friend-wpa.zaloapp.com",
      "state": "CLOSED",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T07:10:12Z",
      "body": "## Context\nParent: #3 (item 2 — `HTTP 404 Not Found from https://tt-friend-wpa.zaloapp.com/api/...`).\n\n## Root cause\nĐịa chỉ API đến từ 3 tầng, và thứ tự ưu tiên hiện tại là **sai**:\n\n| Tầng | Nguồn | Hiện tại |\n|---|---|---|\n| 1 | `endpoints::SERVICE_MAP_V3` — đã verify thật 2026-10-21 | bị ghi đè |\n| 2 | `zpw_service_map_v3` từ server lúc login | **thắng tuyệt đối** (`apply_server_info`, `src/session.rs:535`) |\n| 3 | `session.json` lưu lại | lưu vĩnh viễn cái sai |\n\n`load_from_file()` (`src/se",
      "labels": []
    },
    {
      "number": 21,
      "title": "fix(session): network/HTTP errors are treated as \"expired\" and delete session.json",
      "state": "CLOSED",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T07:10:10Z",
      "body": "## Context\nParent: #3 (item 1 — \"Phiên đăng nhập trong session.json đã hết hạn … tài khoản được log out ngay khi đóng app\").\n\n## Root cause\n`Session::validate_session()` (`src/session.rs:242`) trả `false` cho **mọi** `Err`, kể cả lỗi mạng, HTTP 404/500, sai domain:\n\n```rust\nmatch self.get(&url).await {\n    Ok(resp) => resp.error_code == 0,\n    Err(_) => false,          // network error, 404, parse error -> \"expired\"\n}\n```\n\nCaller rồi xoá thẳng credentials:\n\n| Caller | Hành vi khi `false` |\n|---|",
      "labels": []
    },
    {
      "number": 20,
      "title": "docs: generate user docs site from docs/*.md (mdBook) — deploy blocked on Hostinger access",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T07:10:08Z",
      "body": "## Context\nParent: #3 (item 4 — \"làm một trang docs về feature parity\").\n\nNgười dùng cuối không đọc GitHub. Docs chỉ nằm trong repo là sai chỗ. Nhưng hiện tại **không có đường ống deploy**:\n\n- `ctdozlite.ctdo.net` là HTML tĩnh, host **Hostinger (hPanel)**, sau Cloudflare.\n- Chỉ có `index.html`, `contributors.html`, `privacy.html`, `disclaimer.html`. Không có trang docs, không có `sitemap.xml`.\n- Repo chỉ có 1 workflow (`ci-release.yml`), không đụng tới website.\n- Người báo issue / contributor **",
      "labels": []
    },
    {
      "number": 19,
      "title": "feat(sync): full message history sync from Zalo cloud",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T06:57:36Z",
      "body": "## Context\n- Parent: #3 (item 3)\n- Duplicate report: #10 (\"chỉ mới đồng bộ tin nhắn của My document\")\n\n## Current behaviour\nThe \"Đồng bộ tin nhắn\" button sends `WorkerCmd::RefreshThreads`, which only:\n- validates the session and refreshes the profile,\n- calls `get_friends` to reload the thread list,\n- calls `poll_new_messages`, which merges ~50 recent messages per group / Cloud thread.\n\nIt does **not** download full history, emits **no progress events**, and swallows errors via `unwrap_or_defaul",
      "labels": []
    },
    {
      "number": 17,
      "title": "chore(repo): require pull requests on main (branch protection)",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T06:29:56Z",
      "body": "## Context\n- Parent: #2\n- Release notes now list **merged PRs only**. Direct commits to `main` appear only via the Full Changelog link, so they are invisible in the release description.\n\n## Problem\nNothing currently prevents direct pushes to `main`, and the repository history up to `v0.0.2` was pushed directly. As long as that is allowed, auto-generated release notes will silently miss changes.\n\nThe current maintainer account used for this work has **pull-only access**, so this cannot be enabled",
      "labels": []
    },
    {
      "number": 16,
      "title": "docs(release): decide whether to keep a cumulative CHANGELOG.md in the repository",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T06:26:06Z",
      "body": "## Context\n- Parent: #2\n- Release notes are now auto-generated by GitHub from merged PRs (`generate_release_notes`).\n- The hand-maintained changelog section in `README.md` was removed to avoid duplication; the README now points to GitHub Releases.\n\n## Problem\nThe repository has **no `CHANGELOG.md`**. Release history exists only on the Releases page, and it is:\n- generated **once** at release creation (it does not update later);\n- **per release**, not cumulative;\n- **PR-only** (direct commits app",
      "labels": []
    },
    {
      "number": 15,
      "title": "build(packaging): inject Cargo.toml version into local installer.iss / Info.plist builds",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T06:26:06Z",
      "body": "## Context\n- Parent: #2\n- `packaging/windows/installer.iss` and `packaging/macos/Info.plist` no longer contain a real version. They default to the sentinel `0.0.0`.\n- CI injects the real version from `Cargo.toml`:\n  - Windows: `iscc /DMyAppVersion=<version> packaging/windows/installer.iss`\n  - macOS: `plutil -replace CFBundleShortVersionString -string <version> .../Info.plist`\n\n## Problem\nA local packaging run without the CI flags produces installers labelled `0.0.0`. That is safe (it cannot be ",
      "labels": []
    },
    {
      "number": 14,
      "title": "task(release): categorize auto-generated release notes with .github/release.yml",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T06:21:09Z",
      "body": "## Context\n- Parent: #2\n- Related: release notes now rely on `generate_release_notes`, so merged PRs are listed automatically.\n\n## Problem\nGitHub reads `.github/release.yml` to group auto-generated release notes into categories. Categories are matched by **PR labels**, but the upstream repository currently has **no labels configured** (a label taxonomy exists only on the fork). Adding `release.yml` before labels exist would push every PR into a single catch-all category, which is no better than ",
      "labels": []
    },
    {
      "number": 13,
      "title": "task(ci): add dry-run packaging workflow_dispatch",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T06:05:41Z",
      "body": "## Context\n- Parent: #2\n- Blocked by: None\n\n## Scope\nAdd a `workflow_dispatch` entry point that builds and packages all platforms without creating a GitHub Release, so release plumbing can be validated without pushing a real `v*` tag.\n\n- Input `version` (optional, default from `Cargo.toml`).\n- Input `publish` (default `false`); the release job is skipped when false.\n- Reuse the single version-propagation mechanism introduced for #2; do not duplicate version logic.\n\n## Acceptance\n- [ ] Manual run",
      "labels": []
    },
    {
      "number": 12,
      "title": "task(release): resolve self-updater manifest and manual release assets",
      "state": "OPEN",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T06:05:41Z",
      "body": "## Context\n- Parent: #2\n- Blocked by: None\n\n## Evidence\n- Release `v0.0.2` contains `latest.json` (`version: 0.0.2`, `sha256`, `repository`, `notes`) and `ctdozlite-0.0.2-portable-windows-x86_64.exe`.\n- Neither artifact exists in the tagged tree (`git grep latest.json v0.0.2` returns nothing) nor in any workflow.\n- `src/` contains no self-updater code, yet the `v0.0.2` release notes claim in-place auto-update.\n- `latest.json` has 7 downloads, so something may consume it.\n- The other 9 assets in ",
      "labels": []
    },
    {
      "number": 10,
      "title": "Đồng bộ tin nhắn",
      "state": "OPEN",
      "author": {
        "login": "T1st-isme",
        "name": "Hà Trung Triển"
      },
      "createdAt": "2026-11-01T05:23:17Z",
      "body": "Chưa thể đồng bộ toàn bộ tin nhắn từ zalo, chỉ mới đồng bộ tin nhắn của \"My document\"",
      "labels": []
    },
    {
      "number": 9,
      "title": "Cập nhật thứ tự nhắn tin sai , không hiện nhóm , tin nhắn được ghim lên đầu nên thêm tự import tin nhắn từ folder có sẵn ( migrate )",
      "state": "OPEN",
      "author": {
        "login": "Khoasoma",
        "name": "khoasoma"
      },
      "createdAt": "2026-11-01T03:39:56Z",
      "body": "\n\n\ngửi ảnh hiện url cdn ( nên sửa )\n\n\n\n\nDự án hiện tại đã khá hoàn chỉnh , phần cốt lõi như giao nhận tin nhắn đã hoàn thành , chỉ cần cải thiện về mặt giao diện là được",
      "labels": []
    },
    {
      "number": 8,
      "title": "Không có tên nhóm, không đồng bộ được đoạn chat cũ + Lỗi UI cài đặt. Win11",
      "state": "CLOSED",
      "author": {
        "login": "slothteabar",
        "name": "slothteabar"
      },
      "createdAt": "2026-11-01T03:15:47Z",
      "body": " \n\n",
      "labels": []
    },
    {
      "number": 7,
      "title": "[Feature Request] Thêm API hỗ trợ Dev Plugin và Theme",
      "state": "OPEN",
      "author": {
        "login": "Shigure-Ruiseki",
        "name": "Ruiseki"
      },
      "createdAt": "2026-11-01T03:14:54Z",
      "body": "Mình muốn hỏi liệu dự án có dự định cung cấp API để cộng đồng có thể tự phát triển plugin hoặc theme trong tương lai không?\nMình nghĩ nếu có API chính thức thì sẽ rất hữu ích, vì mọi người có thể tự mở rộng tính năng hoặc tùy chỉnh giao diện theo nhu cầu mà không cần phải chỉnh sửa trực tiếp mã nguồn của dự án.\nVí dụ: Custom Sticker Pack sync bằng GD hoặc cloud nào đó, theme sync với gtk và qt",
      "labels": []
    },
    {
      "number": 6,
      "title": "Không thể hiển thị được chữ",
      "state": "CLOSED",
      "author": {
        "login": "Nghia323223",
        "name": "Nghia323223"
      },
      "createdAt": "2026-11-01T02:21:03Z",
      "body": "Mình dùng nó không hiện được chữ việt, hay tàu, ảnh là link và bấm cũng không làm được hay xem được\nmình chạy trên Fedora 44.\n\n",
      "labels": []
    },
    {
      "number": 5,
      "title": "k hiển thị dc chữ tàu, k hiển thị dc ảnh, group nhiều thành viên bị chớp chớp???",
      "state": "OPEN",
      "author": {
        "login": "heroes1412",
        "name": "heroes1412"
      },
      "createdAt": "2026-11-01T02:18:23Z",
      "body": "chữ tàu thì hiện ô vuông, ảnh thì là 1 cái link, nhưng bấm vào link cũng k xem dc ảnh.\n group nhiều thành viên bị chớp chớp, kiểu bị refresh hay sao đó\nmình chạy trên windows",
      "labels": []
    },
    {
      "number": 4,
      "title": "Hiện không đúng giờ",
      "state": "CLOSED",
      "author": {
        "login": "tinzisreal",
        "name": "Nguyen Huu Tung"
      },
      "createdAt": "2026-11-01T01:37:39Z",
      "body": "",
      "labels": []
    },
    {
      "number": 3,
      "title": "`session.json`, TUI và đồng bộ tin nhắn",
      "state": "CLOSED",
      "author": {
        "login": "shinishiho",
        "name": "Đỗ Quang Minh"
      },
      "createdAt": "2026-10-31T23:34:40Z",
      "body": "1. `⚠️ Phiên đăng nhập trong session.json đã hết hạn. Đang chuyển sang đăng nhập mới…` (trên TUI). Tương tự trong GUI, tài khoản được log out ngay khi đóng app.\n2. TUI trống, không tải được danh sách chat: `Lỗi tải danh sách bạn bè: HTTP 404 Not Found from https://tt-friend-wpa.zaloapp.com/api/...`\n3. Đồng bộ tin nhắn trên GUI: Bấm nút đông bộ chỉ có một toast notification hiện lên, không có pop-up tiến trình, và không có gì xảy ra.\n\nBtw, mình biết phần mềm đang \"beta\", nên nếu được thì bạn hãy ",
      "labels": []
    },
    {
      "number": 2,
      "title": "CI release version",
      "state": "CLOSED",
      "author": {
        "login": "shinishiho",
        "name": "Đỗ Quang Minh"
      },
      "createdAt": "2026-10-31T23:24:27Z",
      "body": "CI release đang hardcode version là `0.0.1` thì phải, kết quả là release cho v0.0.2 mà filename vẫn ghi `0.0.1`.\nTrên góc trái GUI hoặc `zalo-rust --version` cũng đều hiện `v0.0.1`.\n\nEdit: có vẻ từ tag 0.0.1 -- 0.0.2 chỉ có commit về CI và docs.",
      "labels": []
    },
    {
      "number": 1,
      "title": "Bug UI",
      "state": "CLOSED",
      "author": {
        "login": "duyan0",
        "name": "Võ Duy Ân"
      },
      "createdAt": "2026-10-31T15:58:20Z",
      "body": "",
      "labels": []
    }
  ],
  "prs": [
    {
      "number": 55,
      "title": "feat(chat): bảo toàn định danh và trạng thái tin nhắn",
      "state": "MERGED",
      "author": {
        "login": "thuongtin",
        "name": "Ethan"
      },
      "createdAt": "2026-11-01T14:14:31Z",
      "body": "## Nội dung\n- Tách model tin nhắn và trạng thái thành PR độc lập: loại tin, wire identity, delivery state, unread policy, reaction summary/replay ledger và sidebar presentation.\n- Bổ sung các pure helper `message_model`, `reactions`, `unread`, export cho library/binary và giá trị mặc định tại các constructor hiện có.\n\n## Lý do\n- Giữ định danh và metadata gốc qua các lần merge; tránh đánh đồng unread với kích thước history batch hoặc xóa reaction khi nhận snapshot cũ.\n- Cung cấp nền nhỏ nhất cho ",
      "labels": []
    },
    {
      "number": 54,
      "title": "feat(core): tải cập nhật có giới hạn và hoàn tác",
      "state": "MERGED",
      "author": {
        "login": "thuongtin",
        "name": "Ethan"
      },
      "createdAt": "2026-11-01T14:05:14Z",
      "body": "## Nội dung\n- Thêm module độc lập để tải release có giới hạn dung lượng/deadline, kiểm tên asset, size, SHA-256 tùy chọn và chữ ký định dạng executable.\n- Cài đặt giao dịch qua sibling staging/backup, launcher callback và hoàn tác nếu launch thất bại; hỗ trợ portable deferred stage/abort/drop.\n\n## Lý do\n- Ngăn ghi gói chưa xác thực lên bản đang chạy và giữ được bản cũ khi thay thế hoặc khởi chạy thất bại.\n\n## Cách thực hiện\n- Nền độc lập mới nhất: upstream `main` tại `1345f5d0cfd24fdb7d54e5caf3a",
      "labels": []
    },
    {
      "number": 53,
      "title": "feat(media): giới hạn tài nguyên và cô lập cache",
      "state": "MERGED",
      "author": {
        "login": "thuongtin",
        "name": "Ethan"
      },
      "createdAt": "2026-11-01T14:05:09Z",
      "body": "## Nội dung\n- Thêm module độc lập cho tải media có giới hạn, định danh cache theo tài khoản/thread/message/chất lượng và lỗi có kiểu.\n- Có cancellation ticket, ghi original qua file tạm riêng, cache account-isolated, prune cache, LRU texture và admission giới hạn trước khi spawn.\n\n## Lý do\n- Cung cấp lớp tài nguyên dùng lại để hạn chế bộ nhớ, dung lượng đĩa và số job đồng thời; không trộn adapter GUI vào PR nền tảng.\n\n## Cách thực hiện\n- Nền độc lập mới nhất: upstream `main` tại `1345f5d0cfd24fd",
      "labels": []
    },
    {
      "number": 52,
      "title": "fix(session): ghi phiên bằng thay thế nguyên tử",
      "state": "MERGED",
      "author": {
        "login": "thuongtin",
        "name": "Ethan"
      },
      "createdAt": "2026-11-01T14:05:04Z",
      "body": "## Nội dung\n- Ghi `session.json` qua file tạm riêng có quyền `0600` trên Unix, flush hoàn chỉnh rồi rename thay thế.\n- Bổ sung kiểm thử ghi lần hai, lỗi thay thế không phá đích, dọn file tạm và roundtrip định dạng phiên.\n\n## Lý do\n- Tránh cắt cụt file phiên khi ghi thất bại; giữ định dạng `SavedSession` và luồng đăng nhập hiện có.\n\n## Cách thực hiện\n- Nền độc lập mới nhất: upstream `main` tại `1345f5d0cfd24fdb7d54e5caf3a88e3e3b2acd4a`, đã bao gồm các PR upstream #40 và #41. Giữ nguyên thay đổi b",
      "labels": []
    },
    {
      "number": 51,
      "title": "feat(ci): đóng gói và kiểm tra helper macOS độc lập",
      "state": "MERGED",
      "author": {
        "login": "thuongtin",
        "name": "Ethan"
      },
      "createdAt": "2026-11-01T13:09:23Z",
      "body": "## Nội dung\n- Đóng gói độc lập 5 helper macOS theo đúng arm64, x86_64 hoặc universal của app.\n- Rebase nguyên patch lên upstream `1345f5d`; 23 file, +1349/-0, không sửa Rust hoặc giao thức.\n\n## Lý do\n- Gói macOS cần đầy đủ helper với kiến trúc đồng nhất và contract headless có thể tái lập.\n\n## Cách thực hiện\n- Compile từng Swift slice, liên kết tĩnh zstd 1.5.7 đã kiểm SHA256 và đưa BSD license vào bundle.\n- Public build không tải/đóng gói codec JS/WASM bên ngoài; missing-assets trả lỗi có hướng ",
      "labels": []
    },
    {
      "number": 50,
      "title": "chore(ci): strip symbols from release binaries",
      "state": "MERGED",
      "author": {
        "login": "GrootTheDeveloper",
        "name": "Dang Phuc An Khang"
      },
      "createdAt": "2026-11-01T13:04:31Z",
      "body": "## What\n- `[profile.release] strip = true`: Windows release exe 33.4 MB → 24.9 MB\n\n## Why\n- The release exe carried its symbol table\n\n## How\n- strip only; codegen unchanged. Thin LTO + `codegen-units = 1` reached 15.3 MB but used more CPU during the first seconds after start in my measurements, so it is left out\n\n## Verification\n- [x] `cargo test --locked` (107 passed)\n- [ ] `cargo clippy --all-targets -- -D warnings`: `main` already fails it (56 lib warnings); this branch adds none (per-lint co",
      "labels": []
    },
    {
      "number": 49,
      "title": "fix(ci): link the Windows icon on the GNU toolchain",
      "state": "MERGED",
      "author": {
        "login": "GrootTheDeveloper",
        "name": "Dang Phuc An Khang"
      },
      "createdAt": "2026-11-01T13:04:28Z",
      "body": "## What\n- `cargo build` works on `x86_64-pc-windows-gnu` (MSYS2 / MinGW) again\n\n## Why\n- `build.rs` passes the raw `app_icon.res` to the linker; GNU ld cannot read `.res`, so the GNU build fails at link time\n\n## How\n- On `target_env = \"gnu\"`, `windres` (MinGW binutils) converts the `.res` to a COFF object which is linked instead; MSVC path unchanged\n\n## Verification\n- [x] `cargo test --locked` (107 passed)\n- [ ] `cargo clippy --all-targets -- -D warnings`: `main` already fails it (56 lib warning",
      "labels": []
    },
    {
      "number": 48,
      "title": "perf(ui): load the CJK fallback font only when CJK text appears",
      "state": "MERGED",
      "author": {
        "login": "GrootTheDeveloper",
        "name": "Dang Phuc An Khang"
      },
      "createdAt": "2026-11-01T13:04:25Z",
      "body": "## What\n- Startup RAM 253 MB → 196 MB when no Han / Kana / Hangul text is present (offline fixture: fake session, 303 threads, 200 messages per 1-1 thread, 62k rows in `zalo_history.db`, network blocked); unchanged once CJK text shows up\n\n## Why\n- `setup_custom_fonts` read `msyh.ttc` (~20 MB) or another CJK font at every start and egui kept it for the whole run\n\n## How\n- Fonts are built without it; `ensure_cjk_font` rebuilds them with the same CJK fallback the first time such text appears in the",
      "labels": []
    },
    {
      "number": 47,
      "title": "perf(ui): render only what is on screen",
      "state": "MERGED",
      "author": {
        "login": "GrootTheDeveloper",
        "name": "Dang Phuc An Khang"
      },
      "createdAt": "2026-11-01T13:04:21Z",
      "body": "## What\n- Idle RAM 208 MB → 162–166 MB, idle CPU 3.0–3.4 % → 0.6–1.2 % (offline fixture: fake session, 303 threads, 200 messages per 1-1 thread, 62k rows in `zalo_history.db`, network blocked, a 2 000-message group open)\n\n## Why\n- egui laid out every bubble of the open chat each frame (2 000 galleys per repaint in a large group) and painted every thread row, downloading every avatar\n- Each group bubble scanned all threads to find the sender avatar\n- Images were decoded twice (validate, then use)",
      "labels": []
    },
    {
      "number": 46,
      "title": "perf(ui): move the message poll timer into the worker",
      "state": "OPEN",
      "author": {
        "login": "GrootTheDeveloper",
        "name": "Dang Phuc An Khang"
      },
      "createdAt": "2026-11-01T13:04:19Z",
      "body": "## What\n- The GUI no longer repaints every 1.2 s while idle; idle CPU 3.0–3.4 % → 2.3–2.7 % of one core (offline fixture: fake session, 303 threads, 200 messages per 1-1 thread, 62k rows in `zalo_history.db`, network blocked; the fixture itself triggers a reconnect event every ~2.4 s, so a real idle session saves more)\n\n## Why\n- The `PollMessages` trigger lived in `update()`, so `request_repaint_after(1200 ms)` re-laid out the whole UI forever just to fire a timer\n\n## How\n- A tokio interval in t",
      "labels": []
    },
    {
      "number": 45,
      "title": "feat(chat): resumable history sync for groups and Cloud",
      "state": "OPEN",
      "author": {
        "login": "GrootTheDeveloper",
        "name": "Dang Phuc An Khang"
      },
      "createdAt": "2026-11-01T13:04:15Z",
      "body": "> Draft. Stacked on #43 (first commit). Not yet exercised against a live account (my phone camera cannot scan the login QR right now); it reuses the same `getoldv2` call and digit-only cursor rule as the existing `LoadGroupHistory` backfill. Overlaps in purpose with the history parts of #42; happy to close if that lands first.\n\n## What\n- Settings → \"Đồng bộ ngay\" pages old history for every group and \"Cloud của tôi\" into `zalo_history.db`, with progress in the status line and a \"Dừng\" button\n- E",
      "labels": []
    },
    {
      "number": 44,
      "title": "perf(store): open the window faster with a large history",
      "state": "MERGED",
      "author": {
        "login": "GrootTheDeveloper",
        "name": "Dang Phuc An Khang"
      },
      "createdAt": "2026-11-01T13:04:13Z",
      "body": "> Stacked on #43 (first commit). Please review the last 3 commits; I will rebase once #43 lands.\n\n## What\n- Window shows in 0.61–0.70 s instead of 1.04–1.12 s with a large history (Windows 11, release build, offline fixture: fake session, 303 threads, 200 messages per 1-1 thread, 62k rows in `zalo_history.db`, network blocked)\n\n## Why\n- Every hydrate ran `DELETE FROM messages WHERE text = 'sendBubbleMessage'` (full table scan) and loaded every `(msg_id, thread_id)` row in the database into a Has",
      "labels": []
    },
    {
      "number": 43,
      "title": "fix(store): return the newest rows from messages_for",
      "state": "MERGED",
      "author": {
        "login": "GrootTheDeveloper",
        "name": "Dang Phuc An Khang"
      },
      "createdAt": "2026-11-01T13:04:04Z",
      "body": "## What\n- `messages_for(thread, n)` returns the newest `n` messages (still oldest-first) instead of the oldest `n`\n- `persist_threads` runs in one transaction\n\n## Why\n- `ORDER BY ts ASC LIMIT n` handed back the *oldest* rows, so any thread with more stored history than the hydrate limit (200 at startup, 500 when opened) reopened on stale messages and the newest ones were missing\n- In autocommit every inserted row was its own fsync\n\n## How\n- `SELECT ... FROM (SELECT ... ORDER BY ts DESC LIMIT ?2)",
      "labels": []
    },
    {
      "number": 42,
      "title": "docs(chat): mục lục các PR nhỏ thay thế bản tổng",
      "state": "CLOSED",
      "author": {
        "login": "thuongtin",
        "name": "Ethan"
      },
      "createdAt": "2026-11-01T12:30:11Z",
      "body": "# PR #42: chuỗi đóng góp đã rebase và chia theo dependency\n\nKhông yêu cầu review/merge monolith này. Giữ nguyên source cũ để tham chiếu; mỗi PR bên dưới có delta riêng.\n\n## Nền và trạng thái hiện tại\n\n- Upstream `main`: `1345f5d0cfd24fdb7d54e5caf3a88e3e3b2acd4a`; đã có #40/#41 và các thay đổi history/render/CJK, GNU/MSVC icon, release strip, đổi binary thành `ctdozlite`, avatar nhóm và group-call/audio mới.\n- Integration tip: `20a7412a5a78c722a709f12252ce5fe30f6a3db7`. Đã giải quyết conflict bằn",
      "labels": []
    },
    {
      "number": 41,
      "title": "fix(store): chọn thư mục dữ liệu ghi được trước khi khởi động",
      "state": "MERGED",
      "author": {
        "login": "thuongtin",
        "name": "Ethan"
      },
      "createdAt": "2026-11-01T12:06:21Z",
      "body": "## Thay đổi (What)\n\nBổ sung cơ chế chọn và kiểm tra thư mục dữ liệu **trước khi mở phiên đăng nhập, cache, log hoặc cơ sở dữ liệu**:\n\n- `--data-dir <PATH>` và `--data-dir=PATH` để chọn rõ thư mục dữ liệu.\n- `--print-data-dir` để chuẩn bị, in thư mục được chọn rồi thoát trước các thao tác khác.\n- `--version`/`-V` thoát trước khi chuẩn bị nơi lưu dữ liệu, không tạo Application Support chỉ để xem phiên bản.\n\n**Phạm vi PR: fix độc lập về khởi động/lưu trữ. UI tổng thể chưa được tối ưu/hoàn thiện, kh",
      "labels": []
    },
    {
      "number": 40,
      "title": "fix(chat): hiển thị giờ tin nhắn theo múi giờ hệ điều hành",
      "state": "MERGED",
      "author": {
        "login": "thuongtin",
        "name": "Ethan"
      },
      "createdAt": "2026-11-01T12:06:16Z",
      "body": "## Thay đổi (What)\n\nSửa cách hiển thị thời gian tin nhắn và hội thoại để dùng **múi giờ của hệ điều hành**, thay vì mặc định hiển thị UTC từ phép chia epoch.\n\n**Phạm vi PR: fix độc lập về tính đúng đắn của thời gian. UI tổng thể chưa được tối ưu/hoàn thiện, không phải lý do loại các tính năng có UI khỏi đóng góp. Chỉ mới kiểm thử trên Mac (macOS 27.0.1, Apple Silicon arm64); Windows và Linux chưa được kiểm thử.**\n\n## Lý do (Why)\n\nCách tính giờ cũ lấy epoch, chia giây rồi modulo 24. Kết quả là gi",
      "labels": []
    },
    {
      "number": 36,
      "title": "docs: test gemini code assist bot",
      "state": "CLOSED",
      "author": {
        "login": "TIPC1110",
        "name": "ctdo"
      },
      "createdAt": "2026-11-01T11:02:25Z",
      "body": "Test PR to verify @gemini-code-assist bot review",
      "labels": []
    },
    {
      "number": 30,
      "title": "fix(session): keep session.json on network errors, prompt only on a real expiry",
      "state": "MERGED",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T09:53:59Z",
      "body": "> Independent of #29, but both touch `session.rs`: #29 rewrites the domain heal path, this one the session status path. Merging #29 first keeps the conflict trivial.\n\n## What\n\n- `SessionStatus { Valid, Expired, Unreachable }` replaces `validate_session() -> bool`. Only `error_code` `600` / `-102` mean the credentials are gone; transport, HTTP, parse and unrelated codes are `Unreachable`.\n- `SessionError` carries the numeric code, so `send()` and the seven `chat.rs` call sites no longer bury it i",
      "labels": []
    },
    {
      "number": 29,
      "title": "fix(net): let verified endpoints win over zpw_service_map_v3",
      "state": "MERGED",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T07:51:20Z",
      "body": "## What\n\n- `Domains::heal_from_verified()` restores every endpoint listed in `endpoints::SERVICE_MAP_V3` whenever a session is loaded, so a verified constant always wins over what `session.json` kept.\n- `Domains::apply_service_map()`: `zpw_service_map_v3` now only fills keys that have no verified entry (`group_cloud_message`), or slots that are still empty.\n- `Domains::default()` heals itself, so a fresh QR login no longer starts on the `p*-msg-chat.zaloapp.com` / `p*-ctl-chat.zaloapp.com` long-",
      "labels": []
    },
    {
      "number": 28,
      "title": "Add macOS x86_64 build to CI workflow",
      "state": "MERGED",
      "author": {
        "login": "vuhuymicrosoft-bot",
        "name": "jesse"
      },
      "createdAt": "2026-11-01T07:47:58Z",
      "body": "",
      "labels": []
    },
    {
      "number": 26,
      "title": "ci(macos): build native Intel (x86_64) .dmg alongside Apple Silicon",
      "state": "MERGED",
      "author": {
        "login": "quangnguyenn98",
        "name": "quangnguyenn98"
      },
      "createdAt": "2026-11-01T07:43:51Z",
      "body": "Turn the macos-dmg job into an arm64/x86_64 matrix (macos-14 and macos-15-intel), suffix the .dmg and artifact names with the arch, and update the release notes and README accordingly.",
      "labels": []
    },
    {
      "number": 25,
      "title": "docs: user-facing feature matrix + mdBook docs site (build only, deploy pending)",
      "state": "MERGED",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T07:23:30Z",
      "body": "## What\n- Add `docs/TINH_NANG.md`: a 38-row feature matrix over login, messaging, history, friends, calls and platform.\n- Add `docs/DA_BIET.md`: a page generated from the live GitHub issue list, never hand-edited.\n- Extend `docs/KHAC_PHUC_SU_CO.md` with session expiry, friend-list 404, multi-account working directories and re-scan QR cases.\n- Add `book.toml`, `scripts/gen-known-issues.sh`, `scripts/build-docs.sh` and `.github/workflows/docs.yml` to render `docs/*.md` as a static site.\n- Link the",
      "labels": []
    },
    {
      "number": 18,
      "title": "fix(release): derive version from Cargo.toml and generate release notes",
      "state": "MERGED",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T06:30:43Z",
      "body": "## What\n- Make `Cargo.toml` the single source of truth for the release version.\n- Derive every artifact name, package metadata and installer version from it in CI.\n- Replace the hand-written release body with GitHub's generated notes (merged PRs).\n\n## Why\n- Issue #2: `v0.0.2` shipped 9 assets still named `0.0.1` because the version was hardcoded in 6 places.\n- Release descriptions were maintained by hand and duplicated the README install section.\n- A tag could be pushed with a version that did n",
      "labels": []
    },
    {
      "number": 11,
      "title": "docs(harness): add engineering standards and issue workflow",
      "state": "MERGED",
      "author": {
        "login": "ngxccc",
        "name": "Ngoc Tran"
      },
      "createdAt": "2026-11-01T05:43:08Z",
      "body": "## What\r\n- Add `AGENTS.md` with build/test commands, architecture map and non-negotiables.\r\n- Add `GLOSSARY.md` with session/protocol/chat/runtime vocabulary and `INV-1..4`.\r\n- Add nine standards under `docs/standards/`: comments, API/errors, SQLite, concurrency, testing, security, git flow, issue tracking, domain docs.\r\n- Add `docs/adr/README.md` template + index; link the standards from `docs/README.md`.\r\n- Add GitHub issue forms (bug, feature, task, sub-issue) and ignore local agent tooling a",
      "labels": []
    }
  ]
};
