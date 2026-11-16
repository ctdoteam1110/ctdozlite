# ADR-0004: Authentication and CORS for the Local REST API

- **Status:** proposed
- **Date:** 2026-11-15
- **Deciders:** project maintainers
- **Related:** Issue #35

## Context

The local HTTP server in `src/api_mcp.rs` binds to `127.0.0.1`, but previously skipped authentication when `CTDOZLITE_API_TOKEN` was absent and returned wildcard CORS headers. The REST and HTTP MCP routes include local history export and message-sending operations. The server also accepted arbitrary `Host` values, so loopback binding alone did not reject a browser request addressed through a rebinding hostname.

The old behavior is evidenced by the server implementation before this change. The new TCP-level regression tests in `src/api_mcp.rs` exercise missing and invalid tokens, authenticated clients, configured and untrusted origins, preflight behavior and untrusted hosts. No Zalo protocol behavior or network request signing changes in this decision.

## Decision

Require a non-empty `CTDOZLITE_API_TOKEN` before binding the HTTP server and authenticate every REST and HTTP MCP request using a Bearer token with constant-time comparison. Continue binding only to `127.0.0.1`. Accept only `localhost` or `127.0.0.1` in the `Host` header for the configured port. Emit CORS headers only for origins listed in `CTDOZLITE_API_ALLOWED_ORIGINS`; an empty list allows no browser origins. Permit unauthenticated `OPTIONS` only for a configured origin and supported preflight method and headers; each follow-up request still requires the Bearer token. Requests without an `Origin` remain available to authenticated non-browser clients.

## Consequences

- Positive: browser origins outside the explicit allowlist cannot read responses or invoke API operations, and requests without a token fail closed.
- Negative: existing clients must configure a token and send `Authorization: Bearer <token>`. Browser clients must also configure their exact origin.
- Follow-ups: startup error reporting in GUI/background server launch paths remains part of the separate API lifecycle change.

## Verification

- Automated evidence: `cargo test --locked` includes TCP-level checks for token, origin, preflight and Host behavior; tests use a loopback listener and an in-memory SQLite database.
- Human verification: run the API with a token, verify an authenticated request succeeds, then verify missing and invalid tokens receive `401`. Send a preflight from a configured origin and verify the follow-up without a token still receives `401`; send an untrusted origin and verify `403` with no `Access-Control-Allow-Origin` header. Confirm an authenticated client without `Origin` can connect.

## Migration and Rollback

Set `CTDOZLITE_API_TOKEN` before enabling the API. Existing browser clients must additionally set `CTDOZLITE_API_ALLOWED_ORIGINS` to their exact origin and send the Bearer token on each request. There is no data or schema migration. To roll back, disable the local API before reverting this change; reverting restores unauthenticated access when the token is unset and wildcard CORS, so it weakens the security boundary.

## Alternatives Considered

| Option | Why rejected |
| :--- | :--- |
| Keep optional authentication and wildcard CORS | Exposes sensitive local operations to browser-origin attacks when callers omit configuration. |
| Allow every origin but require a token | A browser page can still reach the loopback service, so explicit origin and Host validation remain useful defense in depth. |
| Add an HTTP framework for this change | The security policy fits the existing local server boundary; request framing and resource limits are handled as a separate change. |
