# Architecture Decision Records

One file per decision: `NNNN-<kebab-name>.md`, numbered from `0001`, never renumbered.

## Template

```markdown
# ADR-0001: <Decision title>

- **Status:** proposed | accepted | superseded by ADR-NNNN | deprecated
- **Date:** YYYY-MM-DD
- **Deciders:** <names/roles>

## Context

Forces at play, constraints, evidence (capture note, upstream behaviour).

## Decision

What we will do, in one paragraph, present tense.

## Consequences

- Positive:
- Negative:
- Follow-ups:

## Alternatives Considered

| Option | Why rejected |
| :--- | :--- |
```

## Index

| ADR | Title | Status |
| :--- | :--- | :--- |
| [0001](0001-history-sync.md) | History sync | Accepted |
| [0002](0002-self-updater.md) | Self updater | Accepted |
| [0003](0003-changelog-policy.md) | Changelog policy | Accepted |
| [0004](0004-account-owned-history-store.md) | Separate account-owned incremental history storage | Accepted |

| [0005](0005-server-aware-deletion-tombstones.md) | Preserve verified server identity in deletion tombstones | Accepted |

Related domain language lives in [`/GLOSSARY.md`](../../GLOSSARY.md).
