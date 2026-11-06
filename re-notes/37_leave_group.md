# 37 — Leave group (Rời nhóm)

Source: Zalo PC bundle extracted from `/Applications/Zalo.app/Contents/Resources/app.asar`
(`pc-dist/compact-app-pc.*.js`, `pc-dist/lazy/default-login-main-startup-shared-worker-znotification.*.js`),
read 2026-11-06.

## Request
- Service map: `11189:{domainType: group, commandId: 11189, path: "/api/group/leave"}`.
- Builder (`static leaveGroup(e, t = 0)`):
  `POST getGroupDomain() + "/api/group/leave?" + _getCommonParams()`
  with body `params=encodeURIComponent(encodeAES(JSON.stringify({grids: [groupId], imei,
  silent: t == 1 ? 1 : 0, language})))`.
- `groupId` is the bare id (`getGroupIdFromConversationId`, i.e. without the `g` / `group_` prefix).
- `silent` = 1 is "Rời nhóm trong im lặng" (`group_privacy.leave_group_silently`): only the owner and
  deputies are told. The PC client reports `leaveType: "SILENTLY" | "NORMAL"` to its own analytics
  (`leaveGroupReport`); ctdozlite does not send that.
- On success the PC client deletes the group locally (`storage.deleteGroup`). The owner of a group
  with other members is first sent to "change owner" (`_changeGroupOwnerV2`); ctdozlite does not
  transfer ownership and surfaces the server's error instead.

## Implementation
- `chat::leave_group` / `chat::leave_group_body`, test `leave_group_body_matches_pc_bundle`.
- GUI: "Rời nhóm" in the conversation context menu and in the info drawer (Thiết lập bảo mật),
  confirmation dialog with the silent switch; on success the row is removed and tombstoned.

## Not yet verified
- No live capture yet. Manual check: leave a throwaway test group from ctdozlite and confirm the
  group disappears in the phone app as well and no further messages arrive.
