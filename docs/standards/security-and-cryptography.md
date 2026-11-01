# Security & Cryptography Standards

Threat model: local desktop client holding a live Zalo session. The main risks are credential leakage, session theft, and protocol misuse.

---

## 1. Secret Inventory

| Secret | Location | Rule |
| :--- | :--- | :--- |
| `zpw_sek` (session cookie) | `session.json` | Never log, never commit, never send anywhere except Zalo hosts. |
| `zpw_enk` (encryption key) | `session.json` | Same; derive AES key in memory only. |
| Cookies / IMEI | `session.json` | Same; IMEI is a hardware identifier. |
| Password | memory only | Never persist, never log. |
| Raw captures | `re-notes/`, `dump/` | Must be sanitized before commit; `dump/` is gitignored. |

`session.json` and `*.db` are gitignored and documented in `SESSION_SECURITY.md`. Both rules are non-negotiable.

---

## 2. Secret Handling in Code

- **Never** format a secret into an error, log line, panic message or UI toast.
  ```rust
  // Bad
  anyhow!("login failed with cookie {cookie}")
  // Good
  anyhow!("login failed: session cookie rejected (error_code 600)")
  ```
- **Never** `Debug`-print a session struct; give it a manual `Debug` that redacts fields.
- **Zeroize** derived key material after use where practical (`zeroize` crate) or keep its scope minimal.
- Compare MACs, hashes and tokens with a **constant-time** comparison; never `==` on secret bytes.
- Do not add a dependency that phones home; audit new crates for network behaviour.

---

## 3. `session.json` Lifecycle

1. Write atomically (temp file + rename) with restrictive permissions (`0600` on Unix).
2. On read failure/corruption, treat as logged out; never fall back to a default key.
3. On expiry (`600` / `-102`), delete the file and restart QR login.
4. On explicit logout, delete the file.
5. Never copy it into logs, crash reports, backups under the repo, or issue trackers.

---

## 4. Cryptography Rules

| Primitive | Usage | Rule |
| :--- | :--- | :--- |
| AES-CBC + PKCS#7, zero IV | Zalo `params` compatibility | Must match the observed server contract; do not "improve" it (INV-1). |
| AES-GCM | Local at-rest payloads where applicable | Fresh random nonce per message; never reuse a nonce. |
| MD5 | `signkey` only | Protocol compatibility only; never for password or integrity decisions. |
| Random | IDs, salts, nonces | Use `OsRng` / `rand` CSPRNG; never time-seeded PRNGs. |

- Crypto changes require an ADR (`docs/adr/`) and a known-answer test.
- Constant-time comparison is mandatory for any secret comparison.
- Do not invent a new cipher flow; the server rejects it (`18060`).

---

## 5. Input & Content Safety

- **Untrusted content**: message text, names, sticker payloads, link previews, file names, raw JSON.
- Render as text; never interpret remote content as markup, commands or paths.
- Validate and bound: lengths, counts, nesting depth, archive entry paths (no path traversal when unpacking stickers/attachments).
- Treat URLs from messages as untrusted: parse and restrict schemes before opening.
- File writes go to app-owned directories with sanitized names; never to an attacker-chosen absolute path.

---

## 6. Logging Rules

- Log identifiers (`thread_id`, `msg_id`) and `error_code`; never cookies, tokens, passwords, phone numbers or message bodies of others.
- Truncate raw payloads (`truncate(&text, 200)`) and redact known secret fields before logging.
- Logs are local only; do not add remote telemetry without an ADR and explicit user consent.

---

## 7. Dependencies & Supply Chain

- `cargo audit` before a release; no known-vulnerable dependency ships.
- Prefer `rustls` (already used) over native TLS for reproducibility.
- Pin versions in `Cargo.lock`; review lockfile diffs in PRs.
- New crypto, network or parsing crates need a one-line justification in the PR.

---

## 8. Release & Distribution

- Release artifacts are built by CI from tagged commits; no local unsigned binary is published.
- Do not embed credentials, dev tokens or debug endpoints in release builds.
- Keep `dump/`, `qr.png`, `call_relay.json`, `session*.json` out of packaging and artifacts.

---

## 9. Incident Response

If a secret leaks (commit, issue, screenshot, log):

1. Revoke the session from the Zalo mobile app (Login History / Active Sessions).
2. Delete the exposed file/comment.
3. Rotate anything derived from it.
4. Record the incident and the prevention step in an ADR or issue.

---

## 10. Review Checklist

- [ ] No new secret in code, logs, errors or tests.
- [ ] New file writes are atomic and permission-restricted.
- [ ] Secret comparisons are constant-time.
- [ ] No nonce reuse; randomness from a CSPRNG.
- [ ] Untrusted content is bounded and never interpreted as code/paths.
- [ ] `cargo audit` clean for the change set.
