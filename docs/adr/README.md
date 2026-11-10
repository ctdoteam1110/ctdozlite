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
| [0004](0004-account-history-and-sync-failure-policy.md) | Account history ownership and unsupported sync handling | accepted |
| [0005](0005-native-call-video.md) | Native call video and cancellable camera isolation | local implementation; release checks pending |
| [0006](0006-native-video-on-v010.md) | Native video on upstream v0.1.0 | accepted for PR #129 integration |

Related domain language lives in [`/GLOSSARY.md`](../../GLOSSARY.md).
