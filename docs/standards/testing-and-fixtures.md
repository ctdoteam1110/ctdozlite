# Testing & Fixture Standards

Runner: `cargo test --locked` (CI runs it on Linux, Windows and macOS). Tests live in the crate they cover.

---

## 1. Test Hierarchy

| Level | Location | Naming |
| :--- | :--- | :--- |
| **Unit** | `#[cfg(test)] mod tests` at the bottom of the module | `mod tests` + descriptive `fn` names |
| **Module integration** | `tests/` directory when multiple modules must cooperate | `tests/<area>_integration.rs` |
| **Protocol fixture** | `tests/` + `re-notes/` fixtures | `tests/protocol_<topic>.rs` |

Naming pattern: `fn <behaviour>_when_<condition>()`.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_is_idempotent_for_same_msg_id() { /* INV-4 */ }
}
```

Rules:

- One behaviour per test; a failure must name the broken behaviour.
- Tests must not touch the network. Zalo responses are fixtures, never live calls.
- Tests must not read or write the user's real `session.json` or database.
- Tag assertions that verify a domain invariant with `// (INV-N)`.

---

## 2. Fixtures & Builders

There is no DB-backed factory system; use small builders and static fixtures.

| Pattern | Location | Rule |
| :--- | :--- | :--- |
| **Static JSON fixture** | `tests/fixtures/<name>.json` | Sanitized capture; never contains cookies, `zpw_sek`, `zpw_enk`, IMEI or phone numbers. |
| **Builder** | `tests/common/mod.rs` or inline `fn sample_message()` | Defaults are valid; override only the field under test. |
| **Object mother** | `fn standard_thread()`, `fn group_thread()` | Named domain presets for readability. |
| **In-memory store** | `MessageStore::open(":memory:")` | Mandatory for store tests. |

Rules:

- Fixture files are committed only after redaction; state the source date and `error_code` in a companion comment or filename (`get_last_msgs_2026-10-21.json`).
- No fixture may contain real user data.
- Builders return plain structs; no hidden global state.

---

## 3. SUT Isolation

- Test the module under test directly; do not route through GUI or worker unless the boundary is the SUT.
- GUI/worker tests assert on emitted `WorkerEvent`s, not rendered pixels.
- Crypto tests are known-answer tests: fixed key + fixed plaintext → fixed ciphertext.
- Network-dependent behaviour is tested by parsing recorded frames, not by calling Zalo.

---

## 4. What Must Be Tested

| Area | Minimum coverage |
| :--- | :--- |
| `crypto.rs` | AES roundtrip, known-answer vectors, `md5_hex` vector, `sign_key` parameter set. |
| `param_cipher.rs` | Encode/decode roundtrip + known-answer vector. |
| `store.rs` | Upsert idempotency, delete-for-me, thread ordering (INV-4). |
| `socket.rs` | Frame encode/decode, unknown-frame survival, inflate path. |
| `session.rs` | `session.json` roundtrip, cookie attachment, expiry handling. |
| `chat.rs` | Response parsing for success + each documented `error_code`. |
| `endpoints.rs` | Service lookup for every key in `SERVICE_MAP_V3`. |

Protocol parsing changes require a fixture-backed test. If a case cannot be tested automatically, document manual verification steps in the PR (device, flow, observed `error_code`).

---

## 5. Test Data & Secrets

- Never use a live session in tests.
- Never commit captured traffic that still contains credentials.
- Use obviously fake identifiers (`user-1`, `thread-1`, `msg-0001`).
- If real data is needed for reproduction, keep it out of Git and describe how to obtain it in `re-notes/`.

---

## 6. Running Tests

```bash
cargo test --locked            # full suite
cargo test crypto              # filter by module/name
cargo test -- --nocapture      # show logs for a failing test
```

- A PR must not reduce coverage of the module it touches.
- Flaky tests are bugs: fix or quarantine with a tracked `// TODO(#id)` and an issue.

---

## 7. Benchmarks & Performance Checks

- Any change to the polling loop, media cache or history pagination must include a measurement (before/after) in the PR.
- Benchmark output is a deterministic table or JSON: `task`, `iterations`, `min_ms`, `avg_ms`, `p50_ms`, `p95_ms`, `p99_ms`.
- No emojis or decorative banners in benchmark output.
- Report the hardware/OS and the dataset size alongside the numbers.

---

## 8. Review Checklist

- [ ] New behaviour has a test that fails before the change.
- [ ] Fixtures are sanitized and dated.
- [ ] Store tests use `:memory:`.
- [ ] Invariant assertions are tagged `(INV-N)`.
- [ ] No network, no real `session.json`, no real user database in tests.
