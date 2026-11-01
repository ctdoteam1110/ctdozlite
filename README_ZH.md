# ctdozlite

**[Tiếng Việt](README.md)** · **[English](README_EN.md)** · **[한국어](README_KO.md)** · **[日本語](README_JA.md)** · **[简体中文](README_ZH.md)**

一个完全使用 Rust (`eframe`/`egui` + `ratatui`) 编写的极轻量 Zalo 桌面客户端。直接与 Zalo Web/PC API 及 WebSocket 服务通信，不依赖 Electron 或 Chromium。

- **项目官网 (主页与快速下载):** [https://ctdozlite.ctdo.net/](https://ctdozlite.ctdo.net/)
- **预构建安装包 (GitHub Releases):** [https://github.com/ctdoteam1110/ctdozlite/releases](https://github.com/ctdoteam1110/ctdozlite/releases)
- **文档与故障排除:** [功能与限制说明](docs/TINH_NANG.md) · [已知问题](docs/DA_BIET.md) · [故障排除指南](docs/KHAC_PHUC_SU_CO.md)

---

## ⚡ 核心亮点

- **极致 Rust 性能:** 毫秒级极速冷启动，极低内存开销（~20-50MB RAM），待机 CPU 占用近乎为零。
- **全平台支持:** 原生支持 Windows 10/11 (MSVC 与 GNU)、macOS (Apple Silicon arm64 与 Intel x86_64 通用二进制) 以及 Linux (Debian, Ubuntu, Fedora Wayland/X11)。
- **双模式切换:** 全功能图形界面 GUI (Atom One Dark 经典深色主题) 以及终端控制台模式 TUI (`ratatui`)。
- **VoIP 音视频通话:** 纯 Rust 原生实现的 VoIP 通话引擎，支持 Annex-B H.264 视频解码、RTP/RTCP 协议及 Opus 音频重采样。
- **安全与本地隐私:** 本地 SQLite 数据库存储 (`zalo_history.db`)，支持 `--data-dir` 自定义数据路径，无任何外部数据回传或遥测。

---

## 📥 下载与安装

请前往 [GitHub Releases](https://github.com/ctdoteam1110/ctdozlite/releases) 下载适合您操作系统的预构建版本：

| 操作系统 | 安装包类型 | 安装说明 |
| :--- | :--- | :--- |
| **Windows** | `.exe` 安装程序 / 便携版 `.zip` | 运行安装程序或解压后直接执行 `zalo-rust.exe` |
| **macOS** | Universal `.dmg` (支持 M 系列及 Intel) | 打开 `.dmg` 文件，将 `ctdozlite.app` 拖入 `/Applications` |
| **Linux** | `.deb` / `.rpm` / `.AppImage` / `.tar.gz` | 通过包管理器安装，或赋予 `chmod +x` 权限后直接运行 |

### 命令行参数

| 参数 | 功能说明 | 使用示例 |
| :--- | :--- | :--- |
| `--data-dir <PATH>` | 指定会话、数据库、缓存与日志存储的绝对或相对目录 | `zalo-rust --data-dir ./my-data --gui` |
| `--print-data-dir` | 输出当前解析生效的数据目录路径并退出 | `zalo-rust --print-data-dir` |
| `--gui` | 以图形界面 GUI 模式启动（默认） | `zalo-rust --gui` |
| `--tui` | 以终端控制台 TUI 模式启动 | `zalo-rust --tui` |
| `--qr` | 启动 TUI 模式时立即请求并渲染登录二维码 | `zalo-rust --tui --qr` |
| `--logout` | 清除本地保存的登录凭据并退出 | `zalo-rust --logout` |
| `--version` / `-V` | 查看版本信息并直接退出（不创建数据文件夹） | `zalo-rust --version` |

---

## 🛠️ 从源码编译

### 前置环境
- **Rust 工具链:** Stable Rust (推荐 1.80+，可通过 `rustup` 安装)。
- **系统依赖项:**
  - **Linux:** `libasound2-dev`, `libfontconfig1-dev`, `libx11-dev`, `libxcursor-dev`, `libxi-dev`, `libgl1-mesa-dev`, `ffmpeg`.
  - **Windows / macOS:** 标准 C 编译器环境及 `ffmpeg` (用于音视频通话编解码)。

```bash
# 克隆仓库
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite

# 编译优化发布版本
cargo build --release

# 运行客户端
./target/release/zalo-rust --gui
```

---

## 🏆 核心贡献者 (Key Contributors)

感谢为本项目提交核心代码贡献的代表性开发者：

| 贡献者 | GitHub | 核心职责 | 贡献代码量 |
| :--- | :--- | :--- | :---: |
| **TIPC1110** | [@TIPC1110](https://github.com/TIPC1110) | 核心协议架构、VoIP 通话引擎、egui 客户端框架、跨平台编译 | **> 73,000+ 行** |
| **thanhdo1110** | [@thanhdo1110](https://github.com/thanhdo1110) | 模块化 GUI 重构、Atom One Dark 设计系统 | **> 14,000+ 行** |
| **ngxccc** | [@ngxccc](https://github.com/ngxccc) | 工程化规范设计、会话安全保护、网络域名动态解析 | **> 3,000+ 行** |
| **Thuong Tin** | [@thuongtin](https://github.com/thuongtin) | 本地操作系统时区修复、macOS 数据目录沙箱隔离 | **~840+ 行** |
| **GrootTheDeveloper** | [@GrootTheDeveloper](https://github.com/GrootTheDeveloper) | SQLite 启动性能调优、消息虚拟化渲染、CJK 中日韩字体按需加载 | **~370+ 行** |

> *注: 涵盖所有报告缺陷、提出建议和提交 PR 的完整贡献者荣誉墙请查阅 [在线贡献者名录](https://ctdozlite.ctdo.net/contributors.html)。*

---

## 📜 开源许可证

本项目基于 **[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)** 双重开源许可证分发。项目仅用于学术交流及系统级网络工程研究。
