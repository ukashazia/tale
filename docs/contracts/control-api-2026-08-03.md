# Tale Control API contract ledger — 2026-08-03

This ledger records only contracts checked against Tailscale's current public
documentation and the interactive OpenAPI document returned by
`https://api.tailscale.com/api/v2?outputOpenapiSchema=true` on 2026-08-03.
It is not generated client code. Tale sends no mutation request in Phase 5.

The fixed production origin is `https://api.tailscale.com`. Every path segment
and query value is encoded structurally. Requests use `Authorization: Bearer`
and `Accept`/`Content-Type` only where an entry below requires them.

## Evidence

- [Tailscale API reference](https://tailscale.com/docs/reference/tailscale-api)
- [Interactive API documentation](https://tailscale.com/api)
- [Trust credential scopes](https://tailscale.com/docs/reference/trust-credentials)
- [OAuth clients](https://tailscale.com/docs/features/oauth-clients)
- [Configuration audit logging](https://tailscale.com/docs/features/logging/audit-logging)
- [Tailnet policy file](https://tailscale.com/docs/features/tailnet-policy-file)
- [OpenAPI schema](https://api.tailscale.com/api/v2?outputOpenapiSchema=true)

The current OpenAPI schema and trust-scope page disagree about the user-detail
path (`/users/{userId}` versus `/user/{userID}`). Tale therefore does not adopt
that method. The collection remains supported because both sources agree on
`/tailnet/{tailnet}/users` and `users:read`.

## Common contract

| Item | Contract |
| --- | --- |
| origin | `https://api.tailscale.com` |
| authorization | `Authorization: Bearer <access token>`; never a URL or query value |
| user agent | `Tale/<package version>`; no device, user, or profile data |
| timeout | `admin.request_timeout`, default 15 seconds, range 1 second–2 minutes |
| body cap | 4 MiB for successful decoded responses; 64 KiB retained for redacted error bodies |
| retries | idempotent reads only; at most two retries for transport, 429, and documented transient 5xx/504 responses |
| pagination | none documented for the adopted endpoints; one response per refresh, no synthetic page parameters |
| safe metadata | `x-tailscale-request-id` when present, observed time, HTTP status, `Retry-After` and standard rate-limit headers when present, page count 1 |
| fixtures | fictional IDs, reserved addresses/domains, and fixed timestamps; source date 2026-08-03 |

An endpoint's documented 400/403/404/500/502/504 responses are classified by
status and retained as bounded redacted errors. A 401 is always
`Unauthenticated`; a 403 changes only that endpoint's observed capability.

## Adopted read methods

### Devices

| Operation | Method and path | Scope | Request | Success and response | Errors | Consumed fields |
| --- | --- | --- | --- | --- | --- | --- |
| list devices | `GET /api/v2/tailnet/{tailnet}/devices` | `devices:core:read` | no query parameters; bearer header | 200 `application/json`; object `{devices: Device[]}` | 404 tailnet, 500, 504 | `nodeId` (preferred), `id`, `addresses`, `user`, `name`, `hostname`, `clientVersion`, `updateAvailable`, `os`, `created`, `connectedToControl`, `lastSeen`, `keyExpiryDisabled`, `expires`, `authorized`, `isExternal`, `tags`, `isEphemeral`, `advertisedRoutes`, `enabledRoutes` when returned |
| device detail | `GET /api/v2/device/{deviceId}` | `devices:core:read` | `fields=all` query parameter; no other query | 200 `application/json`; `Device` | 400 invalid ID, 404 device, 500, 504 | same `Device` fields as list plus documented optional detail fields |
| posture | `GET /api/v2/device/{deviceId}/attributes` | `devices:posture_attributes:read` | no query | 200 `application/json`; `{attributes: object, expiries: object}` | 404 device, 500, 504 | attribute presence and bounded key/value display; values are opaque JSON |
| routes | `GET /api/v2/device/{deviceId}/routes` | `devices:routes:read` | no query | 200 `application/json`; `{advertisedRoutes: string[], enabledRoutes: string[]}` | 404 device, 500, 504 | advertised and enabled route strings |

All device methods are non-paginated. The API describes `nodeId` as preferred
and `id` as legacy; composition uses only the exact preferred stable value when
available, with no name/address/user fallback.

### Users

| Operation | Method and path | Scope | Request | Success and response | Errors | Consumed fields |
| --- | --- | --- | --- | --- | --- | --- |
| list users | `GET /api/v2/tailnet/{tailnet}/users` | `users:read` | no query | 200 `application/json`; object `{users: User[]}` | 400, 403, 404, 500 | `id`, `displayName`, `loginName`, `tailnetId`, `created`, `type`, `role`, `status`, `deviceCount`, `lastSeen`, `currentlyConnected` |
| user detail | **unsupported** | unresolved scope/path disagreement | not sent | not implemented | current OpenAPI says `/users/{userId}`; trust-scope documentation says `/user/{userID}` | no detail fields consumed |

### DNS

| Operation | Method and path | Scope | Request | Success and response | Errors | Consumed fields |
| --- | --- | --- | --- | --- | --- | --- |
| nameservers | `GET /api/v2/tailnet/{tailnet}/dns/nameservers` | `dns:read` | no query | 200 `application/json`; `{dns: string[]}` | 404, 500 | ordered resolver strings |
| preferences | `GET /api/v2/tailnet/{tailnet}/dns/preferences` | `dns:read` | no query | 200 `application/json`; `{magicDNS: boolean}` | 404, 500 | MagicDNS value |
| search paths | `GET /api/v2/tailnet/{tailnet}/dns/searchpaths` | `dns:read` | no query | 200 `application/json`; `{searchPaths: string[]}` | 404, 500 | ordered search paths |
| split DNS | `GET /api/v2/tailnet/{tailnet}/dns/split-dns` | `dns:read` | no query | 200 `application/json`; object mapping domain to `string[]` or `null` | 404, 500 | exact ordered domain-to-resolver mapping |

The API currently also exposes `/dns/configuration`, but it is not in the
Phase 5 inventory and is not adopted. No DNS method is paginated.

### Access policy

| Operation | Method and path | Scope | Request | Success and response | Errors | Consumed fields |
| --- | --- | --- | --- | --- | --- | --- |
| policy source | `GET /api/v2/tailnet/{tailnet}/acl` | `policy_file:read` (the scope also requires `devices:posture_attributes:read` and `devices:core:read`) | `Accept: application/hujson`; no `details` query | 200 `application/hujson`; exact response bytes | 400, 403, 404, 500 | bytes, content type, observed time, content hash, ETag when present |

The JSON/details representation is not used because Phase 5 must preserve
HuJSON bytes, comments, line endings, and the trailing newline exactly.

### Credentials

| Operation | Method and path | Scope | Request | Success and response | Errors | Consumed fields |
| --- | --- | --- | --- | --- | --- | --- |
| list credential metadata | `GET /api/v2/tailnet/{tailnet}/keys` | `auth_keys:read`, `api_access_tokens:read`, `oauth_keys:read`, or `federated_keys:read` for the relevant subset; `all:read` is never requested | `all=false`; no other query | 200 `application/json`; object `{keys: Key[]}` | 404, 500 | `id`, `keyType`, `created`, `updated`, `expires`, `revoked`, `scopes`, `tags`, `description`, `invalid`, `userId`, non-secret capabilities |
| credential detail | `GET /api/v2/tailnet/{tailnet}/keys/{keyId}` | matching credential-specific read scope | no query | 200 `application/json`; `Key` | 404, 500 | same metadata; the secret `key` field is rejected/redacted and never decoded into domain state |

The API documents `all` as the broad listing switch. Tale sends `all=false`
and never requests the `all` or `all:read` scope automatically.

### Settings and contacts

| Operation | Method and path | Scope | Request | Success and response | Errors | Consumed fields |
| --- | --- | --- | --- | --- | --- | --- |
| tailnet settings | `GET /api/v2/tailnet/{tailnet}/settings` | `feature_settings:read` for feature settings; `logs:network:read`, `policy_file:read`, or other explicitly documented field scope only when independently adopted | no query | 200 `application/json`; `TailnetSettings` | 400, 404, 500 | only documented read fields returned under observed scope: approval flags, key duration, update/flow/route/posture/HTTPS flags, external policy metadata |
| contacts | `GET /api/v2/tailnet/{tailnet}/contacts` | `account_settings:read` | no query | 200 `application/json`; `{account?, support?, security?}` Contact objects | 403, 404, 500 | contact type, email/fallback email, verification state; email is redacted in diagnostics/tasks |

The current OpenAPI description mentions `networking_settings:read` for one
settings field, while the current trust-scope table does not list that scope.
That field remains unsupported unless the profile has an observed documented
scope; the settings method itself remains read-only and partial.

### Activity

| Operation | Method and path | Scope | Request | Success and response | Errors | Consumed fields |
| --- | --- | --- | --- | --- | --- | --- |
| configuration audit | `GET /api/v2/tailnet/{tailnet}/logging/configuration` | `logs:configuration:read` | required `start` and `end` RFC3339 UTC query values; no page/cursor parameters | 200 `application/json`; `{version, tailnet, logs: ConfigurationAuditLog[]}` ordered chronologically | 400 invalid window, 403 forbidden, 404 logging unsupported | `eventTime`, `type`, `deferredAt`, `eventGroupID`, `origin`, `actor`, `target`, `action`, `old`, `new`, `actionDetails`, `error` |

The documented retention is the most recent 90 days. Tale defaults to the
previous 24 hours ending at refresh start and bounds decoded events at 50,000.
This endpoint is explicitly non-paginated.

## Authentication transport

| Operation | Method and path | Scope | Request | Success and response |
| --- | --- | --- | --- | --- |
| OAuth client credentials | `POST https://api.tailscale.com/api/v2/oauth/token` | scopes requested by the stored OAuth client; no scopes are added | `Content-Type: application/x-www-form-urlencoded`; encoded `client_id`, `client_secret`, `grant_type=client_credentials`, and space-delimited `scope` | 200 JSON OAuth response with access token, bearer type, `expires_in` (currently 3600), and optional granted scope |

This is an authentication exchange, not a Control API resource mutation. The
client secret and resulting token exist only in zeroizing memory/keyring
buffers and are never included in URLs, error text, task data, fixtures, or
debug output.
