# Session Security Notice

`session.json` contains live authentication credentials for your Zalo account: the session cookie `zpw_sek`, the key `zpw_enk`, the session AES key, and the device identifier (IMEI). Anyone holding this file can use your Zalo session without logging in again.

## Where it is stored

QTzalo keeps it in the per-user data directory (not in the working directory any more):

| OS | Path |
|---|---|
| Windows | `%APPDATA%\QTzalo\session.json` |
| Linux | `$XDG_DATA_HOME/qtzalo/session.json` (default `~/.local/share/qtzalo/session.json`) |
| macOS | `~/Library/Application Support/QTzalo/session.json` |

On Linux/macOS the file is created with mode `0600` inside a `0700` directory and written atomically. `QTZALO_DATA_DIR` overrides the location.

## Requirements

- **Never commit** `session.json` (or `zalo_history.db`, `threads_cache.json`, `logs/`, `dump/`) to version control. They are listed in `.gitignore` for legacy working-directory copies.
- **Never share** the file or its contents, and never paste it into issue trackers or public chats.
- If the file is exposed, log the session out from the Zalo mobile app (*Settings → Account & Security → Login devices*), then run `zalo-rust --logout`.

See also [SECURITY.md](SECURITY.md).
