# ADR-0005: Preserve verified server identity in deletion tombstones

- Status: Accepted
- Date: 2026-11-02
- Supersedes: the schema version 2 tombstone representation in ADR-0004

## Context

A legacy canonical message key can be a client ID. Two packets can reuse that
client ID while carrying distinct verified server IDs. A canonical-only deletion
tombstone cannot distinguish a deleted server packet from a fresh server packet.

## Decision

Account-owned history schema version 3 adds nullable `server_id` to
`message_tombstones` and an account-local `(thread_id, server_id)` index. The
forward-only 2-to-3 migration runs under an immediate transaction and preserves
all rows. Existing tombstones remain NULL: lost provenance is not reconstructed.
Owner and newer-version checks still precede mutation. The database filename and
legacy MessageStore schema remain unchanged.

Exact message deletion captures the verified server identity from the stored
message. Conversation deletion uses one SQL INSERT SELECT over typed `chat_json`
server metadata without loading all message bodies into Rust memory. Server
identity is retained transactionally before deleting messages, FTS and metadata.

A replay matching a known deleted server ID remains deleted even if its canonical
key changes. A different verified server ID may use its server ID as the storage
key when the old canonical key is tombstoned. Client and wire aliases stay intact.
A legacy tombstone without server provenance raises an explicit ambiguity error
rather than silently discarding or resurrecting a packet.

## Consequences

- Distinct new server packets survive reused client IDs after deletion and reopen.
- Old schema-2 tombstones remain conservative and may require explicit recovery.
- No network protocol, signing, endpoint, account-root or legacy import changes.
- Downgrade requires a pre-upgrade source backup, not destructive reverse migration.
