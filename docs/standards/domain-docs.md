# Domain Documentation Standards

Single-context repository: one glossary, one ADR log.

## 1. Files

| File | Purpose |
| :--- | :--- |
| `GLOSSARY.md` | Ubiquitous language: session, protocol, chat, runtime terms + `INV-N` invariants. |
| `docs/adr/NNNN-<kebab-name>.md` | One architectural decision per file, numbered from `0001`. |
| `docs/adr/README.md` | ADR template + index table. |

Do not create `GLOSSARY-MAP.md` or per-module glossaries; this repo is a single context.

## 2. Consumption Rules (for agents)

Before exploring or changing the codebase:

1. Read `GLOSSARY.md`. Use its terms verbatim in issue titles, ADR text, test names and identifiers.
2. Read the ADRs touching the area you are about to modify.
3. If a file is missing, proceed silently — do not flag its absence or propose creating it. `/domain-modeling` creates entries lazily when a term or decision is actually resolved.

If your proposal contradicts an accepted ADR, surface it explicitly:

> _Contradicts ADR-0003 (single worker owns all network I/O) — reopening because…_

## 3. Glossary Rules

- **One term, one definition.** Add a row only for domain concepts with a precise meaning in this repo.
- **Record avoided synonyms** in the `Avoid` column — that is what prevents drift.
- **Every term is testable or observable.** Protocol constants, storage fields and runtime roles qualify; generic programming words do not.
- **English only**, matching the standards language. Vietnamese remains for user-facing docs and UI strings.

## 4. Invariant Traceability (`INV-N`)

Invariants are declared once in `GLOSSARY.md`, then referenced from code and tests.

```
GLOSSARY.md (INV-N SSOT)
        ↕
Rustdoc `# Invariants` section on the enforcing function
        ↕
Test name / assertion tagged "(INV-N)"
```

Rules:

- **Declaration**: `INV-N` lives only in `GLOSSARY.md`; IDs are never reused after removal.
- **Enforcement**: the public function that enforces an invariant documents it under `# Invariants` in rustdoc.
- **Verification**: at least one test names the invariant, e.g. `fn upsert_is_idempotent_by_msg_id_inv4()` or an assertion comment `// (INV-4)`.
- **Changing an invariant** requires an ADR when it alters externally observable behaviour.

## 5. ADR Rules

| Status | Meaning |
| :--- | :--- |
| `proposed` | Under discussion; not binding. |
| `accepted` | Binding; code must comply or explicitly supersede. |
| `superseded by ADR-NNNN` | Historical only; do not edit the decision body. |
| `deprecated` | No longer relevant; kept for history. |

- ADRs are append-only: correct with a new ADR, never rewrite an accepted one.
- One ADR per decision. Bundle only tightly coupled choices.
- Include the evidence that justified the decision (capture date, server `error_code`, upstream observation) — protocol work is not self-evident.
- Link the ADR from the issue and the PR (`docs/adr/0007-...`).
