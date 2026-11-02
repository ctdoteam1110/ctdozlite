# ctdozlite

**[Tiếng Việt](README.md)** · **[English](README_EN.md)** · **[한국어](README_KO.md)** · **[日本語](README_JA.md)** · **[简体中文](README_ZH.md)**

ElectronやChromiumを使用せず、Rust（`eframe`/`egui` + `ratatui`）を用いてZalo Web/PC APIおよびWebSocketと直接通信する、超軽量な純Rust製Zaloデスクトップクライアントです。

- **プロジェクト公式サイト（ダウンロード）:** [https://ctdozlite.ctdo.net/](https://ctdozlite.ctdo.net/)
- **公式リリースパッケージ:** [https://github.com/ctdoteam1110/ctdozlite/releases](https://github.com/ctdoteam1110/ctdozlite/releases)
- **ドキュメント・トラブルシューティング:** [機能と制限事項](docs/TINH_NANG.md) · [既知の問題](docs/DA_BIET.md) · [トラブルシューティング](docs/KHAC_PHUC_SU_CO.md)

---

## ⚡ 主な特徴

- **純Rustによる圧倒的な性能:** 約0.18秒の超高速コールドスタート (<200ms)、待機時RAM約149.5 MB（公式Electron版の1.8〜1.9 GBに比べ92%以上削減）、単一ネイティブプロセス、アイドル時CPU使用率ほぼ0%。
- **クロスプラットフォーム対応:** Windows 10/11（MSVCおよびGNU）、macOS（Apple Silicon arm64およびIntel x86_64ユニバーサル）、Linux（Debian、Ubuntu、Fedora Wayland/X11）ネイティブ対応。
- **デュアルモード搭載:** フル機能GUI（Atom One Darkテーマ）およびターミナルベースTUI（`ratatui`）。
- **VoIP音声・ビデオ通話:** Annex-B H.264デコード、RTP/RTCPプロトコル、Opusオーディオリサンプリングをサポートする純Rust製VoIPエンジン。
- **セキュリティとプライバシー:** ローカルSQLite（`zalo_history.db`）への保存、`--data-dir`によるデータパスの自由設定、テレメトリ（外部追跡）ゼロ。

---

## 📥 ダウンロードとインストール

[GitHub Releases](https://github.com/ctdoteam1110/ctdozlite/releases)からビルド済みバイナリを入手できます:

| OS | パッケージ形式 | インストール手順 |
| :--- | :--- | :--- |
| **Windows** | `.exe` インストーラー / ポータブル `.zip` | インストーラーを実行するか、展開して `ctdozlite.exe` を起動 |
| **macOS** | Universal `.dmg`（Apple SiliconおよびIntel） | `.dmg` を開き、`ctdozlite.app` を `/Applications` へドラッグ |
| **Linux** | `.deb` / `.rpm` / `.AppImage` / `.tar.gz` | パッケージマネージャーで導入するか、`chmod +x` を付与して直接実行 |

### CLIコマンドラインオプション

| オプション | 説明 | 実行例 |
| :--- | :--- | :--- |
| `--data-dir <PATH>` | セッション、データベース、キャッシュ、ログの保存先を指定 | `ctdozlite --data-dir ./my-data --gui` |
| `--print-data-dir` | 解決されたデータディレクトリのパスを表示して終了 | `ctdozlite --print-data-dir` |
| `--gui` | グラフィカルUI（GUI）モードで起動（デフォルト） | `ctdozlite --gui` |
| `--tui` | ターミナルUI（TUI）モードで起動 | `ctdozlite --tui` |
| `--qr` | TUI起動時に直ちにログインQRコードを生成 | `ctdozlite --tui --qr` |
| `--logout` | 保存されたセッションを破棄して終了 | `ctdozlite --logout` |
| `--version` / `-V` | ストレージの準備を行わずにバージョンを表示して終了 | `ctdozlite --version` |

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
./target/release/ctdozlite --gui
```

---

## 🏆 主な貢献者（Key Contributors）

リポジトリに代表的なコードコミットを行ったコアコントリビューター:

| 貢献者 | GitHub | 主な担当領域 | コントリビューション規模 |
| :--- | :--- | :--- | :---: |
| **TIPC1110** | [@TIPC1110](https://github.com/TIPC1110) | コアアーキテクチャ、VoIP通話エンジン、eguiフレームワーク、ソーシャルフィード、マルチOS対応 | **> 73,300+ 行** |
| **Đỗ Trung Thành (thanhdo1110)** | [@thanhdo1110](https://github.com/thanhdo1110) | モジュラーGUIリファクタリング、Atom One Dark、MCP/REST API、予約送信、GitHub API自動更新、20件以上のIssue修正とv0.0.5リリース | **> 35,100+ 行** 🏆 |
| **Thuong Tin** | [@thuongtin](https://github.com/thuongtin) | 7件のPR: OSタイムゾーン、データディレクトリ、macOSヘルパー、セッションアトミック保存、メディアキャッシュ、アップデータ、メッセージモデル | **> 6,000+ 行** 🏆 |
| **ngxccc** | [@ngxccc](https://github.com/ngxccc) | 5件のPR: エンジニアリング基準策定、セッション安全性向上、ネットワークドメイン解決 | **> 3,400+ 行** |
| **GrootTheDeveloper** | [@GrootTheDeveloper](https://github.com/GrootTheDeveloper) | 8件のPR: SQLite高速起動最適化、履歴同期再開(#45)、ワーカーポーリング(#46)、チャット描画仮想化、CJKフォント遅延読み込み | **~950+ 行** |

> *注: バグ報告、機能提案、プルリクエストを含むすべての貢献者の名簿は[貢献者一覧ウォール](https://ctdozlite.ctdo.net/contributors.html)で公開されています。*

---


---

## 🙏 謝辞

`ctdozlite` は、ベトナムで数千万人に利用されている優れたメッセージングプラットフォームを構築した **VNG Corporation** および **Zalo** 開発チームに深く感謝いたします。

### 本プロジェクトの目的

- 本プロジェクトは **ただ一つの目的** のために存在します。それは学習とシステムエンジニアリング研究であり、現代のメッセージングアプリがネットワークプロトコル、暗号化、ネイティブデスクトップアーキテクチャの各層でどのように動作するかを理解することです。
- すべてのソースコードは、Zalo Web/PC とサーバー間の公開データフローを観察して **Rust でゼロから記述** されています。本プロジェクトは Zalo の独自資産（オリジナルのアイコン、スタンプ、音声、画像、JS/WASM コード）を **逆コンパイル、抽出、再配布することはありません**。
- 当方は `Zalo` の商標、ロゴ、その他 VNG の知的財産を **所有しておらず、所有権を主張しません**。`Zalo` という名称は、本ソフトウェアが **どのサービスのプロトコルと互換性があるか** を示すための記述的用途にのみ使用されています。
- 本プロジェクトは **完全に非営利** です。100% オープンソース、料金なし、広告なし、データ販売なし。

### データプライバシー

- すべてのメッセージとユーザーデータは **お使いのマシンにローカル保存** されます（`zalo_history.db`、`session.json`、`threads_cache.json`）。
- 本アプリケーションは、管理者のサーバーや第三者に **いかなるデータも送信しません**。唯一のネットワーク接続は、ブラウザで `chat.zalo.me` を開く場合と同一の、公式 Zalo システムへの接続です。

### 削除要請ポリシー

VNG Corporation または Zalo の権限ある代理人が本プロジェクトに関して何らかの要請（削除、名称変更、内容修正など）を行う場合は、**GitHub Issues** または管理者のメールアドレスまでご連絡ください。当方は **72時間以内に回答** し、**あらゆる正当な要請に従う** ことを約束します。

> **要約:** VNG と Zalo チームが素晴らしい製品を生み出したことに深く感謝します。本プロジェクトは、Rust を愛し現代のメッセージングシステムをより深く理解したい小規模な開発者グループによる学習の取り組みに過ぎません。

## 📜 ライセンス

本ソフトウェアは **[MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)** のデュアルライセンスの下で配布されています。学術目的およびシステムプログラミングの研究として開発されています。
