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
| [0001](0001-history-sync.md) | History sync | accepted |
| [0002](0002-self-updater.md) | Self-updater contract | accepted |
| [0003](0003-changelog-policy.md) | Changelog policy | accepted |
| [0004](0004-apple-silicon-fork-release.md) | Apple Silicon fork distribution and update channel | accepted |
| [0005](0005-constant-time-rsa-for-phone-sync.md) | Constant-time RSA for phone-sync key unwrapping | accepted |
| [0006](0006-acknowledged-phone-sync.md) | Acknowledged, session-scoped phone sync | accepted, experimental |

Related domain language lives in [`/GLOSSARY.md`](../../GLOSSARY.md).
