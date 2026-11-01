# Hướng dẫn cài đặt ctdozlite

Tải bản mới nhất tại **[GitHub Releases](https://github.com/ctdoteam1110/ctdozlite/releases)**. Mỗi bản phát hành đính kèm đầy đủ gói cho Linux, Windows và macOS; tên file chứa số phiên bản (dạng `x.y.z`).

> Trong các lệnh bên dưới, thay `<version>` bằng số phiên bản trong tên file bạn đã tải.

## Kiểm tra toàn vẹn (khuyến nghị)

Tải kèm `SHA256SUMS.txt`, đặt cùng thư mục với gói cài rồi chạy:

```bash
sha256sum -c SHA256SUMS.txt --ignore-missing
```

## Linux

Binary Linux được liên kết tĩnh `libopus`, `sqlite3` và `rustls`, hỗ trợ cả distro đời mới lẫn đời cũ (`glibc >= 2.31`).

### Debian / Ubuntu / Linux Mint / Kali / Pop!_OS / Zorin (glibc 2.35+)

Tải `ctdozlite_<version>_amd64.deb`:

```bash
sudo apt install ./ctdozlite_<version>_amd64.deb
```

### Debian 11 / Ubuntu 20.04 / Linux Mint 20 (glibc 2.31+)

Tải `ctdozlite_<version>_legacy_glibc2.31_amd64.deb`:

```bash
sudo apt install ./ctdozlite_<version>_legacy_glibc2.31_amd64.deb
```

### Fedora / RHEL / Rocky Linux / AlmaLinux / CentOS Stream / openSUSE

Tải `ctdozlite-<version>-1.x86_64.rpm`:

```bash
sudo dnf install ./ctdozlite-<version>-1.x86_64.rpm
```

Trên hệ dùng `yum` hoặc `zypper`, thay `dnf` bằng trình quản lý gói tương ứng.

### AppImage (chạy ngay, không cần quyền root)

Tải `ctdozlite-<version>-x86_64.AppImage`:

```bash
chmod +x ctdozlite-<version>-x86_64.AppImage
./ctdozlite-<version>-x86_64.AppImage
```

Nếu hệ thống thiếu FUSE 2, chạy trực tiếp không cần mount:

```bash
./ctdozlite-<version>-x86_64.AppImage --appimage-extract-and-run
```

### Arch Linux / Manjaro / EndeavourOS / Gentoo / Void / NixOS / distro khác (`.tar.gz`)

Tải `ctdozlite-<version>-linux-x86_64.tar.gz` (hoặc bản `-legacy-glibc2.31` cho máy cũ):

```bash
tar -xzf ctdozlite-<version>-linux-x86_64.tar.gz
cd ctdozlite-<version>-linux-x86_64
./install.sh
```

Gói `.tar.gz` kèm sẵn `install.sh` và `uninstall.sh`.

## Windows (x64)

- **Bộ cài đặt:** `ctdozlite-<version>-setup-windows-x86_64.exe` — chạy và làm theo hướng dẫn.
- **Bản chạy ngay (Portable):** `ctdozlite-<version>-portable-windows-x86_64.exe` hoặc gói nén `ctdozlite-<version>-portable-windows-x86_64.zip` — không cần cài đặt.

Nếu Windows SmartScreen hoặc Microsoft Defender cảnh báo ứng dụng mã nguồn mở chưa mua chứng chỉ EV Code Signing thương mại:
1. Đối chiếu mã băm SHA-256 của file tải về với `SHA256SUMS.txt` trên trang GitHub Releases (`Get-FileHash .\ctdozlite-<version>-setup-windows-x86_64.exe -Algorithm SHA256`).
2. Chọn **More info → Run anyway** (hoặc dùng bản `portable-windows-x86_64.exe` / `.zip`). Từ bản `v0.0.4`, bộ cài Inno Setup đã chuyển sang chuẩn nén `zip/9` không đóng khối đặc (`SolidCompression=no`), nhúng đầy đủ `VS_VERSIONINFO` + `RT_MANIFEST` (`asInvoker`) và giữ bảng ký hiệu PE để tránh cảnh báo nhầm (false positive) của bộ máy học Microsoft Defender (#58).

## Tự động thông báo & cập nhật qua GitHub Releases API

`ctdozlite` tích hợp sẵn bộ kiểm tra và cập nhật tự động qua REST API chính thức của GitHub (`https://api.github.com/repos/ctdoteam1110/ctdozlite/releases/latest`, kèm fallback sang `latest.json`):
- **Khi khởi động & định kỳ mỗi 30 phút:** Ứng dụng tự động truy vấn GitHub Releases API ở luồng nền.
- **Thông báo tức thì:** Khi phát hiện phiên bản mới (`vLatest > vCurrent`), ứng dụng tự động bắn thông báo hệ thống (OS Notification trên macOS / Windows / Linux), hiển thị huy hiệu **🚀 Cập nhật mới** trên thanh tiêu đề, thanh thông báo ở đầu khung chat và mở hộp thoại **Có bản cập nhật mới** kèm tóm tắt Changelog & nút **⬇ Cập nhật ngay**.
- **Xác thực SHA-256 & Hoàn tác nguyên tử:** Gói tải về được kiểm tra kích thước, chữ ký thực thi (`MZ` / `ELF`) và mã băm `SHA-256` trước khi thay thế thực thi hoặc mở bộ cài đặt.
- **Kiểm tra từ dòng lệnh (CLI):**
  ```bash
  ctdozlite --check-update
  ```

## macOS (Apple Silicon M1/M2/M3/M4 & Intel)

- **Mac Apple Silicon (Chip M1 / M2 / M3 / M4 — ARM64):** Tải `ctdozlite-<version>-macos-arm64.dmg`.
- **Mac Intel (`x86_64`):** Tải `ctdozlite-<version>-macos-x86_64.dmg`.
- **Bản Universal (chạy chung cho cả chip M & Intel):** Tải `ctdozlite-<version>-macos.dmg`.

Mở file `.dmg` vừa tải và kéo `ctdozlite.app` vào `/Applications`.

Nếu macOS chặn ứng dụng chưa ký Apple Developer ID trả phí, bấm chuột phải vào app và chọn **Open**, hoặc chạy:

```bash
xattr -dr com.apple.quarantine /Applications/ctdozlite.app
```
## Gỡ cài đặt

| Nền tảng | Cách gỡ |
|---|---|
| Debian / Ubuntu (`.deb`) | `sudo apt remove ctdozlite` |
| Fedora / RHEL (`.rpm`) | `sudo dnf remove ctdozlite` |
| `.tar.gz` | Chạy `./uninstall.sh` trong thư mục đã giải nén |
| AppImage | Xoá file `.AppImage` |
| Windows | Gỡ trong **Settings → Apps**, hoặc xoá file portable |
| macOS | Xoá `ctdozlite.app` khỏi `/Applications` |

## Tự biên dịch & đóng gói cục bộ từ mã nguồn

Xem mục **Tự biên dịch từ mã nguồn** trong [README](../README.md#tự-biên-dịch-từ-mã-nguồn-windows--linux--macos).

Để tự đóng gói bộ cài `.dmg` (macOS), `.tar.gz` (Linux) hoặc `.exe` (Windows) trên máy cá nhân với số phiên bản tự động đọc từ `Cargo.toml` (không làm bẩn sentinel `0.0.0` trong `packaging/macos/Info.plist` và `packaging/windows/installer.iss`):

- **macOS / Linux:**
  ```bash
  ./packaging/package-local.sh
  ```
- **Windows (PowerShell):**
  ```powershell
  pwsh -File packaging\package-local.ps1
  ```

Gói sinh ra nằm trong thư mục `dist/`.
