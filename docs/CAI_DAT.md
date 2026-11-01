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
- **Bản chạy ngay (Portable):** `ctdozlite-<version>-portable-windows-x86_64.exe` — không cần cài đặt.

Nếu Windows SmartScreen cảnh báo ứng dụng chưa ký, chọn **More info → Run anyway**.

## macOS

Tải `ctdozlite-<version>-macos.dmg`, mở file và kéo `ctdozlite.app` vào `/Applications`.

Nếu macOS chặn ứng dụng chưa ký, bấm chuột phải vào app và chọn **Open**, hoặc chạy:

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

## Tự biên dịch từ mã nguồn

Xem mục **Tự biên dịch từ mã nguồn** trong [README](../README.md#tự-biên-dịch-từ-mã-nguồn-windows--linux--macos).
