# ADR-0004: Account history ownership and unsupported sync handling

- Status: accepted
- Date: 2026-11-08
- Deciders: user approval to fix the seven review findings

## Context

The v0.0.9 account switch reads account-specific history while several writers,
startup paths and API operations still use a shared database. Live SQLite file
copies omit WAL rows, and asynchronous results can arrive after the visible
account changes. Logout also leaves a reusable saved session.

Phone sync v2 depends on an unpackaged Node/WASM helper. Its fallback invents
public-key bytes without retaining private keys. Official desktop key recovery
cannot determine all key bytes from the recorded known plaintext and previously
fell back to a personal hardcoded key. Android backup direction relies on a
development account's numeric ID.

## Decision

Use accounts/<uin>_history.db as the canonical store for each validated account.
Read legacy history through SQLite, including committed WAL rows, and merge it
once into the first owning account. Canonical rows win on message-ID collisions;
scheduler rows retain their content with newly allocated local IDs. Versioned
metadata records ownership and migration completion. Wipe operations delete
history rows and owned caches while preserving the completion marker.

All startup, GUI, TUI and API paths use a captured account identity. Worker task
lifetimes and GUI completion generations follow the account lifetime. Drop
guards release poll/sync running flags even on cancellation. App-wide settings
authorization and update operations remain independent of account changes.

Explicit logout removes active and saved credentials while retaining history.
Adding another account is a separate command that preserves the existing saved
session. Account switching validates the target before replacing credentials;
live databases are never copied as ordinary files.

Sync v2 fails with an explicit Vietnamese unsupported error before network or
helper execution. Supported v1 synchronization and Cloud/group history remain
available. Desktop decryption requires a supplied passphrase; incomplete key
recovery returns an error. Android import requires an unambiguous self identity
from the existing numeric-to-web backup mapping and refuses to guess otherwise.
No signing, cipher algorithm or wire-format replacement is introduced.

## Consequences

- History and scheduled operations use the same account boundary in every mode.
- Delayed completions cannot merge another account's conversations into the UI.
- Accounts without a verified backup identity cannot import that Android backup.
- Sync v2 and automatic desktop key recovery remain explicitly unsupported until
  evidence and a native implementation are available.
- Regression coverage includes WAL migration, account isolation, credential
  removal, queued completions, cancellation, unsupported crypto and backup
  identity. Live-device protocol behavior has not been verified.

## Alternatives considered

- Restoring shared database snapshots was rejected because it permits ownership
  confusion and unsafe copies of live WAL databases.
- Guessing identities, keys or protocol behavior was rejected because it can
  silently corrupt message direction or claim unsupported synchronization.
- Runtime Node/WASM execution was rejected because the project ships a native
  application without those dependencies.
