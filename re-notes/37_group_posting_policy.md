# Group posting restriction

Observed 2026-11-06 in installed Zalo PC app.asar: groupInfo.setting.lockSendMsg; send/reply paths invoke verifySetting with field lockSendMsg. Creator/deputy IDs come from creatorId/adminIds, already parsed by get_group_members.

Use existing getmg-v2 metadata and standard signed request path. Verify posting permission before group text, quotes, stickers, forwards and attachments; reject on lookup failure without sending. Unit tests cover owner, deputies, ordinary members and old cache defaults. No real locked-community send attempted.
