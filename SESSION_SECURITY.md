# Session Security Notice

`session.json` contains live authentication credentials, session tokens (`zpw_sek`, `zpw_enk`), encryption keys, and hardware identifiers (IMEI) for your Zalo account.

**Security Requirements:**
- **Never commit** `session.json` to version control (ensure it remains in `.gitignore`).
- **Never share** the file or its contents with anyone or paste it into issue trackers or public chats.
- Anyone with access to this file can access your active Zalo chat session without re-authenticating.
- If this file is accidentally exposed, log out of Zalo immediately from your mobile app (Settings > Account & Security > Login History / Active Sessions) to invalidate all session keys and cookies.
