# Zalo Desktop V2 backup cipher compatibility

The locally installed Windows Zalo Desktop 26.10.20 bundle uses SHA-256 of
the account-specific UIN text as the AES-256 key and `zie` followed by the
first 13 characters of that value as the CBC IV for `.zl.zip` V2 imports.
The encrypted content is a TAR stream. This matches the independently
published [V2 format investigation](https://imtien.com/software-development/decrypting-zalo-desktop-backups/).

The header verifier checks both the `ustar` marker and the TAR checksum.
Streaming decryption validates PKCS#7 padding. Neither check authenticates
the archive: AES-CBC does not have an authentication tag. Callers must stage
the output, inspect and validate every TAR entry and record, and discard the
staging output on any error before modifying user data.

Read-only inspection of a Windows 26.10.20 backup found JSON Lines databases
for messages and conversations. Message `toUid` values matched conversation
`userId` values. Desktop `msgType` 4 carries sticker data, while type 18 has
video-shaped data; these differ from the existing client-side wire mappings.
Some photo records use `oriUrl` and `thumbUrl`, and the `content` field can be
null even when `message` contains the content. Tests use synthetic records.
The installed bundle also identifies outgoing messages by either `fromUid` 0
or the current account ID. Its V2 media naming code combines the client message
ID, sender and conversation IDs, and an MD5 of the source URL, with additional
handling for quality variants and legacy paths. The importer implements the
verified identity and URL-hash matching rules and reports unmatched media.

The importer verifies the archive account, retains unsupported records, and
stores matched media transactionally alongside messages. Unsupported message
semantics are reported rather than guessed. No real account values or message
contents are included in the test fixtures.

A second read-only comparison against the Windows backup verified that the
archive root matches the signed-in numeric account ID. It also confirmed that local media records matched the installed bundle's
client/sender/thread identity and full source-URL MD5 naming rule. The comparison printed only
aggregate counts; no real records or account values are included here.
