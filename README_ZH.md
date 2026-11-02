# ctdozlite

**[Tiếng Việt](README.md)** · **[English](README_EN.md)** · **[한국어](README_KO.md)** · **[日本語](README_JA.md)** · **[简体中文](README_ZH.md)**

一个完全使用 Rust (`eframe`/`egui` + `ratatui`) 编写的极轻量 Zalo 桌面客户端。直接与 Zalo Web/PC API 及 WebSocket 服务通信，不依赖 Electron 或 Chromium。

- **项目官网 (主页与快速下载):** [https://ctdozlite.ctdo.net/](https://ctdozlite.ctdo.net/)
- **预构建安装包 (GitHub Releases):** [https://github.com/ctdoteam1110/ctdozlite/releases](https://github.com/ctdoteam1110/ctdozlite/releases)
- **文档与故障排除:** [功能与限制说明](docs/TINH_NANG.md) · [已知问题](docs/DA_BIET.md) · [故障排除指南](docs/KHAC_PHUC_SU_CO.md)

---

## ⚡ 核心亮点

- **极致 Rust 性能:** 约0.18秒毫秒级极速冷启动 (<200ms)，待机 Working Set 内存仅约 149.5 MB（相比官方 Electron 版 1.8~1.9 GB 节省超 92% 内存），单一原生进程，待机 CPU 占用近乎为零。
- **全平台支持:** 原生支持 Windows 10/11 (MSVC 与 GNU)、macOS (Apple Silicon arm64 与 Intel x86_64 通用二进制) 以及 Linux (Debian, Ubuntu, Fedora Wayland/X11)。
- **双模式切换:** 全功能图形界面 GUI (Atom One Dark 经典深色主题) 以及终端控制台模式 TUI (`ratatui`)。
- **VoIP 音视频通话:** 纯 Rust 原生实现的 VoIP 通话引擎，支持 Annex-B H.264 视频解码、RTP/RTCP 协议及 Opus 音频重采样。
- **安全与本地隐私:** 本地 SQLite 数据库存储 (`zalo_history.db`)，支持 `--data-dir` 自定义数据路径，无任何外部数据回传或遥测。

---

## 📥 下载与安装

请前往 [GitHub Releases](https://github.com/ctdoteam1110/ctdozlite/releases) 下载适合您操作系统的预构建版本：

| 操作系统 | 安装包类型 | 安装说明 |
| :--- | :--- | :--- |
| **Windows** | `.exe` 安装程序 / 便携版 `.zip` | 运行安装程序或解压后直接执行 `ctdozlite.exe` |
| **macOS** | Universal `.dmg` (支持 M 系列及 Intel) | 打开 `.dmg` 文件，将 `ctdozlite.app` 拖入 `/Applications` |
| **Linux** | `.deb` / `.rpm` / `.AppImage` / `.tar.gz` | 通过包管理器安装，或赋予 `chmod +x` 权限后直接运行 |

### 命令行参数

| 参数 | 功能说明 | 使用示例 |
| :--- | :--- | :--- |
| `--data-dir <PATH>` | 指定会话、数据库、缓存与日志存储的绝对或相对目录 | `ctdozlite --data-dir ./my-data --gui` |
| `--print-data-dir` | 输出当前解析生效的数据目录路径并退出 | `ctdozlite --print-data-dir` |
| `--gui` | 以图形界面 GUI 模式启动（默认） | `ctdozlite --gui` |
| `--tui` | 以终端控制台 TUI 模式启动 | `ctdozlite --tui` |
| `--qr` | 启动 TUI 模式时立即请求并渲染登录二维码 | `ctdozlite --tui --qr` |
| `--logout` | 清除本地保存的登录凭据并退出 | `ctdozlite --logout` |
| `--version` / `-V` | 查看版本信息并直接退出（不创建数据文件夹） | `ctdozlite --version` |

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
./target/release/ctdozlite --gui
```

---

## 🏆 核心贡献者 (Key Contributors)

感谢为本项目提交核心代码贡献的代表性开发者：

| 贡献者 | GitHub | 核心职责 | 贡献代码量 |
| :--- | :--- | :--- | :---: |
| **TIPC1110** | [@TIPC1110](https://github.com/TIPC1110) | 核心协议架构、VoIP 通话引擎、egui 客户端框架、社交动态页、跨平台编译 | **> 73,300+ 行** |
| **Đỗ Trung Thành (thanhdo1110)** | [@thanhdo1110](https://github.com/thanhdo1110) | 模块化 GUI 重构、Atom One Dark 设计系统、MCP/REST API、定时消息、GitHub API 自动更新、修复 20+ 项 Issue 并发布 v0.0.5 | **> 35,100+ 行** 🏆 |
| **Thuong Tin** | [@thuongtin](https://github.com/thuongtin) | 提交 7 项 PR：本地时区修复、数据目录沙箱、macOS 辅助组件、会话原子写入、媒体缓存、自动更新器、消息状态模型 | **> 6,000+ 行** 🏆 |
| **ngxccc** | [@ngxccc](https://github.com/ngxccc) | 提交 5 项 PR：工程化规范设计、会话安全保护、网络域名动态解析 | **> 3,400+ 行** |
| **GrootTheDeveloper** | [@GrootTheDeveloper](https://github.com/GrootTheDeveloper) | 提交 8 项 PR：SQLite 启动性能调优、群组/云端历史断点同步(#45)、后台轮询计时器(#46)、消息虚拟化渲染、CJK 字体按需加载 | **~950+ 行** |

> *注: 涵盖所有报告缺陷、提出建议和提交 PR 的完整贡献者荣誉墙请查阅 [在线贡献者名录](https://ctdozlite.ctdo.net/contributors.html)。*

---


---

## 🙏 致谢

`ctdozlite` 由衷感谢 **VNG Corporation** 及 **Zalo** 开发团队，他们打造了这一服务越南数千万用户的优秀即时通讯平台。

### 关于本项目的目的

- 本项目存在的 **唯一目的** 是学习与系统级工程研究，即理解现代即时通讯应用在网络协议、加密以及原生桌面架构层面是如何运作的。
- 所有源代码均基于对 Zalo Web/PC 与服务器之间公开数据流的观察，**使用 Rust 从零编写**。本项目 **不会** 反编译、提取或重新打包任何 Zalo 专有资源（不内置原始图标、贴纸、音频、图像或 Zalo 的 JS/WASM 代码）。
- 我们 **不拥有** 也 **不主张拥有** `Zalo` 商标、Logo 或任何 VNG 知识产权。`Zalo` 名称仅作描述性使用，用于说明本软件 **与哪个服务的协议相兼容**，以便用户理解产品性质。
- 本项目 **完全非商业**：100% 开源、不收费、无广告、不出售数据、无任何形式的盈利。

### 关于数据隐私

- 所有消息和用户数据均 **保存在您本机**（`zalo_history.db`、`session.json`、`threads_cache.json`）。
- 本应用 **不会** 向项目维护者的服务器或任何第三方发送任何数据。唯一的网络连接指向 Zalo 官方系统，与您在浏览器中打开 `chat.zalo.me` 完全一致。

### 关于下架政策

若 VNG Corporation 或 Zalo 的合法代表对本项目提出任何要求（包括但不限于下架、更名或调整内容），请通过 **GitHub Issues** 或维护者邮箱与我们联系。我们承诺 **在 72 小时内回复** 并 **遵守一切合理要求**。

> **总之：** 我们非常感谢 VNG 与 Zalo 团队打造了如此优秀的产品。本项目只是一小群热爱 Rust、希望更深入理解现代即时通讯系统的开发者的学习尝试。

## 📜 开源许可证

本项目基于 **[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)** 双重开源许可证分发。项目仅用于学术交流及系统级网络工程研究。
