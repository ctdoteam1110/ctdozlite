# ADR-0006: Keep seen-v2 transport bound to primary evidence

- **Status:** proposed
- **Date:** 2026-11-02
- **Deciders:** Pending maintainer protocol review

## Context

The generalized INV-1 requires ParamCipher v2 and signkey for every request.
The primary client bundle instead uses session AES JSON in a form `params`
field for direct/group seen-v2, authenticated cookies, common query parameters,
a ten-second timeout and no retries. It does not provide a signing tag.
The exact source spans and bundle SHA-256 are recorded in
[the protocol note](../../re-notes/visible-read-seen-v2.md) and
[its primary excerpts](../../re-notes/visible-read-seen-v2-primary.txt).

## Decision

The proposed implementation deliberately narrows INV-1 for the two verified
seen-v2 endpoints only. It centralizes this evidence-bound contract in
`Session::post_receipt_params`; other outbound endpoints retain their standard
ParamCipher v2/signkey path. No guessed signing tag or new protocol field is
introduced. Maintainer approval of this ADR is required for protocol acceptance.

A wire receipt requires exact live wire identities and a continuously sampled,
foreground, bottom-of-chat viewport. The account storage lane rechecks the
short-lived visibility ticket before committing a local read. Wire delivery and
local persistence are separate outcomes. Submitted requests with uncertain
results are not retried automatically in the current account epoch. The bounded
4096-identity attempt ledger stops new wire work at capacity, announces that
state, and leaves local reads operational. It is not durable across epochs.

## Consequences

- Synthetic and loopback tests can verify the actual request construction without
  contacting a real account; they do not establish live provider acceptance.
- Protocol review must assess the explicit exception, rather than approving an
  inaccurate claim that seen-v2 uses ParamCipher v2/signkey.
- Disabling the visible-read command and reverting its Session helper restores
  the prior no-automatic-seen behavior. Local read rows already committed remain
  valid local state; rollback never rewrites a newer database schema.

## Alternatives Considered

| Option | Why rejected |
| :--- | :--- |
| Invent a `seenv2` signing tag | The primary source supplies no such tag. |
| Send seen when history is loaded | Loading is not evidence of visible reading. |
| Retry an unknown result | It silently changes write semantics. |
| Treat fixture success as provider acceptance | It tests a different surface. |
