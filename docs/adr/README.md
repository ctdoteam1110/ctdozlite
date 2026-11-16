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
| [0001](0001-history-sync.md) | History sync and backup import | accepted |
| [0002](0002-self-updater.md) | Self-updater and `latest.json` contract | accepted |
| [0003](0003-changelog-policy.md) | Changelog and release notes policy | accepted |
| [0004](0004-local-api-authentication.md) | Local REST API authentication and CORS | proposed |

Related domain language lives in [`/GLOSSARY.md`](../../GLOSSARY.md).
