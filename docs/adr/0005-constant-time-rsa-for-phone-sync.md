# ADR-0005: Constant-time RSA for phone-sync key unwrapping

- **Status:** accepted for 0.0.9 distribution (2026-11-09)
- **Date:** 2026-11-07
- **Deciders:** maintainer (authorized distribution of the current 0.0.9 build)

## Context

`cargo audit` reports RUSTSEC-2023-0071 ("Marvin Attack") against `rsa` 0.9.10. The crate computes
RSA private-key operations with variable-time big-integer arithmetic, so decryption timing can leak
key material to an attacker who submits many chosen ciphertexts and measures how long each takes.
RustSec lists no patched release.

`rsa` is used in exactly one place, `src/history_sync.rs` (phone message sync, `pullMobileMsg`
cmd 12000):

1. `generate_sync_public_key` creates an **ephemeral** RSA-2048 key for one sync run and sends its
   SubjectPublicKeyInfo DER, Base64, to the server.
2. `import_mobile_backup_container` decrypts **one** `encrypted_key` (RSAES-PKCS1-v1_5) that the
   phone wraps to that public key, then uses the result as the AES key for the backup container.
3. `clear_sync_keys` drops the key when the sync finishes.

Practical exposure is low: the key lives for one sync run, it is used for a single decryption, and no
timing or result is returned to the party supplying the ciphertext. However, the security standard
(`docs/standards/security-and-cryptography.md` §7) says no known-vulnerable dependency ships, and an
`ignore` entry would only hide the finding.

## Decision

Replace `rsa` with `aws-lc-rs` 1.16 (AWS-LC, a BoringSSL derivative) for this path:

- Key generation: `PrivateDecryptingKey::generate(KeySize::Rsa2048)`. The public key is exported
  with `PublicEncryptingKey::as_der()` (X.509 SubjectPublicKeyInfo), the same DER `rsa` produced with
  `to_public_key_der()`, so the wire format is unchanged (prefix `MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A…`,
  294 bytes, e = 65537).
- Decryption: `Pkcs1PrivateDecryptingKey::decrypt`, the same RSAES-PKCS1-v1_5 padding previously
  used through `rsa::Pkcs1v15Encrypt`. AWS-LC runs the private-key operation in constant time with
  blinding.
- The private key is held as PKCS#8 DER in `zeroize::Zeroizing` inside the key mutex and never
  copied out: `decrypt_with_sync_key` decrypts in place. aws-lc-rs zeroes its own DER buffers on
  drop, the unwrapped backup key is returned as `Zeroizing`, and `clear_sync_keys` drops (wipes) the
  slot.
- The DMG ships AWS-LC's notices (`ThirdPartyLicenses/AWS-LC-LICENSE`, `aws-lc-rs-LICENSE`), copied
  from the locked crate sources; the bundle validator fails when they are missing.

The cipher, padding, key size, parameters and request signing all stay the same. Only the library
that implements them changes.

## Evidence

- `rsa_pkcs1_v15_decrypt_matches_openssl_known_answer`: a ciphertext produced independently by
  LibreSSL 3.3.6 (`openssl pkeyutl -encrypt -pkeyopt rsa_padding_mode:pkcs1`) under a throwaway
  test-only key decrypts to the exact expected plaintext. A tampered ciphertext does not yield it.
- `sync_keypair_round_trips_through_public_key`: data encrypted to the exported SPKI key decrypts with
  the stored PKCS#8 key.
- `stored_sync_key_decrypts_in_place_and_clear_removes_it`: the stored key decrypts without leaving
  the slot, and after `clear_sync_keys` neither decryption nor the public key is available.
- `generate_sync_public_key_produces_valid_rsa2048_spki_der_base64` (existing) still passes
  unchanged: SPKI prefix, 294-byte length, exponent bytes.
- `cargo audit` (cargo-audit 0.22.2, advisory DB of 2026-11-06) reports no vulnerabilities after the
  change.

## Consequences

- Positive: RUSTSEC-2023-0071 no longer applies; the dependency closure for RSA shrinks (`rsa`,
  `num-bigint-dig`, `pkcs1`, `pkcs8`, `der`, … removed).
- Negative: `aws-lc-sys` adds a C library compiled at build time (more build time and binary size)
  and its OpenSSL/ISC/Apache notices must ship with the app.
- Not verified: a live phone-backup import against a real Zalo phone. Before release, run one
  phone sync with a test account and confirm that the backup container decrypts and imports.

## Alternatives Considered

| Option | Why rejected |
| :--- | :--- |
| Keep `rsa` and add an audit `ignore` | Hides a known finding; the standard forbids shipping it. |
| `ring` (already in the tree via rustls) | Has no RSA decryption API. |
| Switch to RSA-OAEP | The phone chooses the padding; changing it breaks the protocol. |
| Wait for a patched `rsa` | No release is published; it blocks 0.0.9 indefinitely. |
