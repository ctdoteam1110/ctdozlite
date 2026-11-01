# ctdozlite

**[Tiếng Việt](README.md)** · **[English](README_EN.md)** · **[한국어](README_KO.md)** · **[日本語](README_JA.md)** · **[简体中文](README_ZH.md)**

ElectronやChromiumを使用せず、Rust（`eframe`/`egui` + `ratatui`）を用いてZalo Web/PC APIおよびWebSocketと直接通信する、超軽量な純Rust製Zaloデスクトップクライアントです。

- **プロジェクト公式サイト（ダウンロード）:** [https://ctdozlite.ctdo.net/](https://ctdozlite.ctdo.net/)
- **公式リリースパッケージ:** [https://github.com/ctdoteam1110/ctdozlite/releases](https://github.com/ctdoteam1110/ctdozlite/releases)
- **ドキュメント・トラブルシューティング:** [機能と制限事項](docs/TINH_NANG.md) · [既知の問題](docs/DA_BIET.md) · [トラブルシューティング](docs/KHAC_PHUC_SU_CO.md)

---

## ⚡ 主な特徴

- **純Rustによる圧倒的な性能:** ミリ秒単位での高速起動、極めて軽量なメモリ消費（RAM ~20〜50MB）、アイドル時のCPU使用率はほぼ0%。
- **クロスプラットフォーム対応:** Windows 10/11（MSVCおよびGNU）、macOS（Apple Silicon arm64およびIntel x86_64ユニバーサル）、Linux（Debian、Ubuntu、Fedora Wayland/X11）ネイティブ対応。
- **デュアルモード搭載:** フル機能GUI（Atom One Darkテーマ）およびターミナルベースTUI（`ratatui`）。
- **VoIP音声・ビデオ通話:** Annex-B H.264デコード、RTP/RTCPプロトコル、Opusオーディオリサンプリングをサポートする純Rust製VoIPエンジン。
- **セキュリティとプライバシー:** ローカルSQLite（`zalo_history.db`）への保存、`--data-dir`によるデータパスの自由設定、テレメトリ（外部追跡）ゼロ。

---

## 📥 ダウンロードとインストール

[GitHub Releases](https://github.com/ctdoteam1110/ctdozlite/releases)からビルド済みバイナリを入手できます:

| OS | パッケージ形式 | インストール手順 |
| :--- | :--- | :--- |
| **Windows** | `.exe` インストーラー / ポータブル `.zip` | インストーラーを実行するか、展開して `zalo-rust.exe` を起動 |
| **macOS** | Universal `.dmg`（Apple SiliconおよびIntel） | `.dmg` を開き、`ctdozlite.app` を `/Applications` へドラッグ |
| **Linux** | `.deb` / `.rpm` / `.AppImage` / `.tar.gz` | パッケージマネージャーで導入するか、`chmod +x` を付与して直接実行 |

### CLIコマンドラインオプション

| オプション | 説明 | 実行例 |
| :--- | :--- | :--- |
| `--data-dir <PATH>` | セッション、データベース、キャッシュ、ログの保存先を指定 | `zalo-rust --data-dir ./my-data --gui` |
| `--print-data-dir` | 解決されたデータディレクトリのパスを表示して終了 | `zalo-rust --print-data-dir` |
| `--gui` | グラフィカルUI（GUI）モードで起動（デフォルト） | `zalo-rust --gui` |
| `--tui` | ターミナルUI（TUI）モードで起動 | `zalo-rust --tui` |
| `--qr` | TUI起動時に直ちにログインQRコードを生成 | `zalo-rust --tui --qr` |
| `--logout` | 保存されたセッションを破棄して終了 | `zalo-rust --logout` |
| `--version` / `-V` | ストレージの準備を行わずにバージョンを表示して終了 | `zalo-rust --version` |

---

## 🛠️ ソースコードからのビルド

### 必要な環境
- **Rustツールチェーン:** Stable Rust（1.80以上推奨、`rustup` で導入）。
- **システムライブラリ:**
  - **Linux:** `libasound2-dev`, `libfontconfig1-dev`, `libx11-dev`, `libxcursor-dev`, `libxi-dev`, `libgl1-mesa-dev`, `ffmpeg`.
  - **Windows / macOS:** 標準Cコンパイラ環境および `ffmpeg`（通話機能用）。

```bash
# リポジトリのクローン
git clone https://github.com/ctdoteam1110/ctdozlite.git
cd ctdozlite

# リリースビルド
cargo build --release

# GUI起動
./target/release/zalo-rust --gui
```

---

## 🏆 主な貢献者（Key Contributors）

リポジトリに代表的なコードコミットを行ったコアコントリビューター:

| 貢献者 | GitHub | 主な担当領域 | コントリビューション規模 |
| :--- | :--- | :--- | :---: |
| **TIPC1110** | [@TIPC1110](https://github.com/TIPC1110) | コアアーキテクチャ、VoIP通話エンジン、eguiフレームワーク、マルチOS対応 | **> 73,000+ 行** |
| **thanhdo1110** | [@thanhdo1110](https://github.com/thanhdo1110) | モジュラーGUIリファクタリング、Atom One Darkデザインシステム | **> 14,000+ 行** |
| **ngxccc** | [@ngxccc](https://github.com/ngxccc) | エンジニアリング基準策定、セッション安全性向上、ネットワークドメイン解決 | **> 3,000+ 行** |
| **Thuong Tin** | [@thuongtin](https://github.com/thuongtin) | OSローカルタイムゾーン修正、macOSデータディレクトリ分離 | **~840+ 行** |
| **GrootTheDeveloper** | [@GrootTheDeveloper](https://github.com/GrootTheDeveloper) | SQLite高速起動最適化、チャット描画仮想化、CJKフォント遅延読み込み | **~370+ 行** |

> *注: バグ報告、機能提案、プルリクエストを含むすべての貢献者の名簿は[貢献者一覧ウォール](https://ctdozlite.ctdo.net/contributors.html)で公開されています。*

---

## 📜 ライセンス

本ソフトウェアは **[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)** のデュアルライセンスの下で配布されています。学術目的およびシステムプログラミングの研究として開発されています。
