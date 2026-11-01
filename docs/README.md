# Tài liệu kỹ thuật ctdozlite

Thư mục này chứa tài liệu chi tiết về cách sử dụng, cấu trúc mã nguồn và giao thức mạng của `ctdozlite` (`zalo-rust`).

## Danh mục tài liệu

| Tài liệu | Nội dung |
|---|---|
| [0. Tính năng & giới hạn](TINH_NANG.md) | Cái gì đang chạy, cái gì chạy một phần, cái gì chưa có — kèm issue theo dõi. |
| [0b. Đã biết](DA_BIET.md) | **Sinh tự động** từ GitHub Issues. Đừng sửa tay. |
| [1. Cài đặt](CAI_DAT.md) | Tải và cài đặt trên Linux, Windows, macOS; kiểm tra SHA-256 và gỡ cài đặt. |
| [2. Hướng dẫn sử dụng](HUONG_DAN_SU_DUNG.md) | Các tham số CLI, thao tác trên GUI (`egui`), phím tắt TUI (`ratatui`) và công cụ kiểm thử. |
| [3. Kiến trúc hệ thống](KIEN_TRUC_HE_THONG.md) | Mô hình phân tách UI thread / Tokio worker, đồng bộ WebSocket + HTTP polling và lưu trữ SQLite. |
| [4. Đặc tả giao thức Zalo](GIAO_THUC_ZALO.md) | Cấu trúc tham số `zpw_ver=694`, `ParamCipher v2`, `PasswordCipher`, `signkey` và cơ chế mã hóa AES. |
| [5. Xử lý sự cố](KHAC_PHUC_SU_CO.md) | Bảng mã lỗi máy chủ Zalo (`18012`, `18060`, `18022`, `600`, `102`, `114`, `429`) và cách khắc phục. |

## Tiêu chuẩn kỹ thuật (Engineering Standards)

Bộ quy chuẩn bắt buộc cho mọi thay đổi mã nguồn, nằm tại [`docs/standards/`](standards/) và được tóm tắt trong [`AGENTS.md`](../AGENTS.md):

| Tiêu chuẩn | Nội dung |
|---|---|
| [Issue tracking](standards/issue-tracker.md) | Quy trình GitHub Issues, nhãn triage, sub-issue. |
| [Domain docs](standards/domain-docs.md) | `GLOSSARY.md`, `INV-N`, ADR tại [`docs/adr/`](adr/). |
| [Code comments](standards/code-comment-taxonomy.md) | Rustdoc 4 tầng, `// SAFETY:`, `// TODO(#id):`. |
| [API & errors](standards/api-design-and-error-handling.md) | Endpoint Zalo, `error_code`, retry, IPC worker. |
| [Database](standards/database-and-migrations.md) | SQLite, `PRAGMA user_version`, expand/contract. |
| [Concurrency](standards/concurrency-and-locking.md) | Luồng UI/worker, channel, lock, deadlock. |
| [Testing](standards/testing-and-fixtures.md) | `cargo test`, fixture, builder, benchmark. |
| [Security](standards/security-and-cryptography.md) | `session.json`, secret, crypto, logging. |
| [Git flow](standards/git-flow-and-pr-matrix.md) | Branch, Conventional Commits, 3-tier PR. |

## Biên dịch và chạy nhanh

```bash
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite
cargo build --release

# Chạy giao diện đồ họa (mặc định)
./target/release/zalo-rust --gui

# Chạy giao diện terminal
./target/release/zalo-rust --tui --qr

# Bật thêm bảng kiểm tra mạng (Dev Inspector)
./target/release/zalo-rust --gui --dev
```

## Xây dựng trang tài liệu

Tài liệu trong thư mục này cũng được dựng thành một site tĩnh bằng [mdBook](https://rust-lang.github.io/mdBook/). Danh mục trang nằm ở `docs/SUMMARY.md` — file nào không có trong đó sẽ không được dựng.

```bash
cargo install mdbook --locked     # chỉ cần một lần
./scripts/build-docs.sh           # sinh trang "Đã biết" + dựng site
# Kết quả: book/index.html
```

- `scripts/gen-known-issues.sh` sinh `docs/DA_BIET.md` từ GitHub Issues (`gh`/`curl` + `jq`). Không có mạng thì giữ nguyên file cũ.
- `book/` là output, đã nằm trong `.gitignore`.
- Việc đưa site lên `ctdozlite.ctdo.net/docs/` đang chờ quyền truy cập Hostinger — xem [#20](https://github.com/ctdoteam1110/ctdozlite/issues/20).
