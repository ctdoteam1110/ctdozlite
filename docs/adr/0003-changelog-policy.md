# ADR 0003: Chính sách Changelog & Release Notes

- **Trạng thái:** Đã chấp nhận (Accepted)
- **Issues liên quan:** [#14](https://github.com/ctdoteam1110/ctdozlite/issues/14), [#16](https://github.com/ctdoteam1110/ctdozlite/issues/16), [#20](https://github.com/ctdoteam1110/ctdozlite/issues/20)

## Bối cảnh

Dự án cần cả ghi chú phát hành tự động trên trang GitHub Releases lẫn tệp `CHANGELOG.md` được biên tập rõ ràng trong kho mã nguồn.

## Quyết định

1. **Tự động phân nhóm trên GitHub Releases (`.github/release.yml`):**
   - Các Pull Request được phân loại tự động theo nhãn (`feat`/`enhancement`, `fix`/`bug`, `perf`, `docs`, `ci`/`build`/`release`, và `*`).
   - Phần mở đầu của mỗi GitHub Release dẫn liên kết trực tiếp tới `docs/CAI_DAT.md` và `docs/TINH_NANG.md`.
2. **Tệp `CHANGELOG.md` trong kho mã:**
   - Duy trì theo định dạng [Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), tóm tắt các thay đổi hướng tới người dùng cuối theo từng phiên bản `Cargo.toml`.
