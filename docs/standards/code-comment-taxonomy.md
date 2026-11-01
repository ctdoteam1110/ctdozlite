# Code Comment Taxonomy & Standards

## Core Philosophy

> **Code tells you HOW. Comments tell you WHY.**
> Expressive naming beats explanatory comments. A comment that restates the code is a defect.

---

## 1. The 4-Tier Taxonomy

| Tier | Form | Scope | Mandatory |
| :--- | :--- | :--- | :--- |
| **1. Contract** | `///` rustdoc, `//!` module docs | Public items, shared helpers, non-obvious domain rules | Yes for every `pub` item |
| **2. Rationale** | `//` prose, `// SAFETY:` | Non-obvious decisions, protocol quirks, failure modes | Yes where the "why" is not inferable |
| **3. Tracked debt** | `// TODO(owner-or-issue):` | Known gaps | Only when tracked |
| **4. Prohibited** | — | Echoes, dead code, changelog tags, non-English prose | Never |

---

## 2. Tier 1 — Rustdoc Contracts

Every `pub` item in `lib.rs`/modules and every shared test helper carries rustdoc:

```rust
/// Decode a Base64 AES-CBC payload using a 16- or 32-byte key.
///
/// # Errors
/// Returns an error when the Base64 is malformed, the key length is not 16/32,
/// or the PKCS#7 padding is invalid.
///
/// # Invariants
/// - INV-1: callers must pass a key derived from `zpw_enk`, never a raw password.
pub fn decode_aes(key: &[u8], b64: &str) -> anyhow::Result<String> { ... }
```

Required rustdoc sections when applicable:

| Section | Use |
| :--- | :--- |
| `# Errors` | Any function returning `Result`. |
| `# Panics` | Any `panic!`, `unwrap`, `expect`, index or slice that can panic. |
| `# Safety` | Every `pub unsafe fn`. |
| `# Invariants` | Every `INV-N` the function enforces (see `GLOSSARY.md`). |

Module docs explain the module's role and the protocol surface it implements, in 1–4 lines:

```rust
//! WebSocket transport for Zalo push/pull frames.
//!
//! Frame layout and command IDs are documented in `docs/GIAO_THUC_ZALO.md`.
```

---

## 3. Tier 2 — Rationale Comments

Write concise, professional English prose. Explain the constraint, the observed server behaviour, or the failure the code prevents.

```rust
// PC bundle ships a stale service map; the web/QR session returns *.chat.zalo.me hosts.
let host = service("chat").unwrap_or(MSG_CHAT);
```

Rules:

- No artificial prefix such as `// WHY:` — the prose itself must be clear.
- Put the comment next to the decision, not at the top of a 200-line function.
- Reference the evidence when it is protocol-derived: `// Observed 2026-10-21, error_code 0 (re-notes/...)`.
- **`// SAFETY:` is mandatory** immediately above every `unsafe` block, naming the invariant that makes it sound.

---

## 4. Tier 3 — Tracked Debt

```rust
// TODO(#142): drop this fallback once password login supports 2FA.
// TODO(protocol): revisit when Zalo bumps zpw_ver past 694.
```

- A bare `// TODO: fix this` is banned.
- Use an issue number when one exists; otherwise a stable context token (`protocol`, `perf`, `ui`).
- TODOs that outlive a release without an issue must be converted into one.

---

## 5. Tier 4 — Prohibited

| Banned | Why | Do instead |
| :--- | :--- | :--- |
| `// increment counter` | Echoes code. | Delete it. |
| `// get user by id` above `get_user_by_id()` | Noise. | Delete it. |
| Commented-out code | Git preserves history. | Delete it. |
| `// added by X on 2026-01-01` | Git blame exists. | Delete it. |
| `// v2 rewrite` | Changelog in source. | Put it in the PR/ADR. |
| Non-English prose in source | Mixed-language drift. | English comment; Vietnamese in user-facing strings. |
| Banner art (`///// ===== /////`) | Decoration, not information. | Module docs or plain comments. |

---

## 6. Language Policy

- **Rust source comments and doc comments: English only.**
- **User-facing strings: Vietnamese** (UI labels, error toasts, CLI help).
- **Log messages**: English keys/structure, Vietnamese values only when quoting server text.
- **Legacy Vietnamese comments** are grandfathered, but any file you touch must convert the comments in the region you modify.

---

## 7. Review Checklist

- [ ] Every new `pub` item has Tier 1 docs with `# Errors` / `# Panics` where applicable.
- [ ] Every `unsafe` block has a `// SAFETY:` justification.
- [ ] Every non-obvious branch has a Tier 2 rationale or a clearer name.
- [ ] Every `TODO` names an issue or a context token.
- [ ] No commented-out code, banners, author tags or non-English comments added.
