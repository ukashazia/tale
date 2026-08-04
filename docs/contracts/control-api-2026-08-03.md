# Tale Control API contract ledger — 2026-08-03

This ledger records only contracts checked against Tailscale's current public
documentation and the interactive OpenAPI document returned by
`https://api.tailscale.com/api/v2?outputOpenapiSchema=true` on 2026-08-03 and
rechecked on 2026-08-04 before Phase 6 adoption. It is not generated client
code. The Phase 5 entries are read-only; the Phase 6 entries below are the
complete set of mutation contracts Tale adopts.

The fixed production origin is `https://api.tailscale.com`. Every path segment
and query value is encoded structurally. Requests use `Authorization: Bearer`
and `Accept`/`Content-Type` only where an entry below requires them.

The Phase 7 entries below are the complete adopted policy and credential
workflow contract. Policy request bodies are sent as the original HuJSON bytes;
they are never passed through a JSON serializer. The schema also documents an
optional policy `ETag`/`If-Match` conditional write. Phase 7 deliberately does
not adopt that token: adding it to the save protocol requires a separate
ledger decision. Byte/hash comparison before validation and immediately before
save therefore remains mandatory, and is not a server-atomic compare-and-swap
claim.

## Evidence

- [Tailscale API reference](https://tailscale.com/docs/reference/tailscale-api)
- [Interactive API documentation](https://tailscale.com/api)
- [Trust credential scopes](https://tailscale.com/docs/reference/trust-credentials)
- [OAuth clients](https://tailscale.com/docs/features/oauth-clients)
- [Configuration audit logging](https://tailscale.com/docs/features/logging/audit-logging)
- [Tailnet policy file](https://tailscale.com/docs/features/tailnet-policy-file)
- [OpenAPI schema](https://api.tailscale.com/api/v2?outputOpenapiSchema=true)

The current OpenAPI schema and trust-scope page disagree about user paths
(`/users/{userId}` and `/users/{userId}/...` in the interactive schema versus
`/user/{userID}` and `/user/{userID}/...` in the trust-scope page). The current
interactive operation definitions provide the request, response, and error
contracts for the plural paths, while the trust-scope page provides the
`users` permission. Tale records the discrepancy and adopts only the plural
paths from the current interactive schema; it does not silently fall back to
the singular spelling. If a future interactive schema removes the plural
paths, these actions become unsupported until the ledger is re-reviewed.

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
| user detail | `GET /api/v2/users/{userId}` | `users:read` | no query; bearer header | 200 `application/json`; `User` object | 400, 403, 404, 500; common JSON error body | exact `id`, status, role, login/display name, and device count; the plural path is adopted from the current interactive schema and the singular trust-scope spelling is not used as a fallback |

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
| policy source | `GET /api/v2/tailnet/{tailnet}/acl` | `policy_file:read` (the scope also requires `devices:posture_attributes:read` and `devices:core:read`) | `Accept: application/hujson`; no query parameters | 200 `application/hujson`; exact response bytes | 400, 403, 404, 500 | bytes, content type, observed time, content hash, optional `ETag` response header |

The JSON/details representation is not used because Phase 5 must preserve
HuJSON bytes, comments, line endings, and the trailing newline exactly.

### Adopted Phase 7 policy and credential methods

These entries were added before the Phase 7 adapters. The request body for
each policy operation is the exact candidate byte sequence supplied by the
workflow. Tailscale's `application/json` request alternative is intentionally
not used because serializing a HuJSON candidate would lose comments and source
formatting. Policy validation and preview responses are JSON envelopes; policy
fetch and save responses are raw HuJSON bytes.

| Operation | Method and path | Scope | Request headers, query, and body | Success and response | Documented errors | Verification/semantic contract |
| --- | --- | --- | --- | --- | --- | --- |
| validate policy and run declared tests | `POST /api/v2/tailnet/{tailnet}/acl/validate` | `policy_file:read` | `Accept: application/json`; `Content-Type: application/hujson`; no query; exact candidate HuJSON bytes | 200 `application/json`; `{}` means validation/tests passed; otherwise optional `message` and `data[]` entries may include `user`, `errors[]`, and `warnings[]` | 400, 403, 404, 500; common JSON error body | The object mode validates the hypothetical candidate and evaluates its declared tests without modifying the remote policy. The result is bound to the candidate hash by Tale. Unknown detail fields are bounded and safe-redacted. |
| preview policy rule matches | `POST /api/v2/tailnet/{tailnet}/acl/preview` | `policy_file:read` | `Accept: application/json`; `Content-Type: application/hujson`; required query `type=user|ipport` and `previewFor`; exact candidate HuJSON bytes | 200 `application/json`; `{matches:[{users:string[],ports:string[],lineNumber:integer}],type,previewFor}` | 400, 403, 404, 500; common JSON error body | Only the documented `user` and `ipport` selectors are exposed. Returned destinations/ports and line numbers are authoritative; omitted dimensions are rendered as unavailable. The result is bound to the candidate hash and selector by Tale. |
| save policy | `POST /api/v2/tailnet/{tailnet}/acl` | `policy_file` (also requires the documented policy read dependencies) | `Accept: application/hujson`; `Content-Type: application/hujson`; no query; exact candidate HuJSON bytes; documented optional `If-Match` is not sent by Phase 7 | 200 `application/hujson`; raw server-returned policy bytes | 400 validation/test failure, 403, 404, 412 If-Match mismatch, 500; common JSON error body | Tailscale validates and runs declared tests on save. Tale sends exactly one request only after fresh hash/validation guards, then fetches raw remote bytes and compares them byte-for-byte with the candidate. |
| create auth key | `POST /api/v2/tailnet/{tailnet}/keys` | `auth_keys` | `Accept: application/json`; `Content-Type: application/json`; no query; JSON object with explicit `keyType:"auth"`, optional `description`, `expirySeconds` for 1–90 days, and complete `capabilities.devices.create` fields `reusable`, `ephemeral`, `preauthorized`, and `tags[]` | 200 `application/json`; `Key` object; the one-time secret is only the response `key` field | 404, 500, plus authorization/scope failures classified by status; common JSON error body | The request must preserve every selected capability and tag. The response must contain a non-empty `key` and `keyType:"auth"`; the secret is moved directly to the ephemeral result and is never recoverable from a list/detail call. |
| inspect credential | `GET /api/v2/tailnet/{tailnet}/keys/{key_id}` | matching `auth_keys:read`, `api_access_tokens:read`, `oauth_keys:read`, or `federated_keys:read` (or `all:read`) | `Accept: application/json`; no query; bearer header | 200 `application/json`; `Key` metadata; `invalid:true` means revoked/deleted or expired; a returned `key` field is rejected | 404, 500, plus authorization/scope failures classified by status | Detail is fetched immediately before revocation confirmation. Tale preserves exact ID/type and only renders documented metadata; no secret is decoded into domain state. |
| revoke credential | `DELETE /api/v2/tailnet/{tailnet}/keys/{key_id}` | `auth_keys` for `auth`, `api_access_tokens` for `api`, `oauth_keys` for `client`, `federated_keys` for `federated`, or `all` | `Accept: application/json`; no query or body | 200 with an empty response body | 403 insufficient access, 404 tailnet/key, 500; common JSON error body | The endpoint deletes auth/API keys and supported trust credentials. Tale supports documented non-federated types in Phase 7; federated/workload-identity revocation remains explicitly unsupported. Verify by detail: 404 or documented `invalid:true` is revoked; existing valid metadata is not success. |

### Credentials

| Operation | Method and path | Scope | Request | Success and response | Errors | Consumed fields |
| --- | --- | --- | --- | --- | --- | --- |
| list credential metadata | `GET /api/v2/tailnet/{tailnet}/keys` | `auth_keys:read`, `api_access_tokens:read`, `oauth_keys:read`, or `federated_keys:read` for the relevant subset; `all:read` is never requested | `all=false`; no other query | 200 `application/json`; object `{keys: Key[]}` | 404, 500 | `id`, `keyType`, `created`, `updated`, `expires`, `revoked`, `lastUsed` when returned, `scopes`, `tags`, `description`, `invalid`, `userId`, `knownDependents` when returned, and non-secret capabilities |
| credential detail | `GET /api/v2/tailnet/{tailnet}/keys/{keyId}` | matching credential-specific read scope | no query | 200 `application/json`; `Key` | 404, 500 | same metadata; the secret `key` field is rejected/redacted and never decoded into domain state |

The API documents `all` as the broad listing switch. Tale sends `all=false`
and never requests the `all` or `all:read` scope automatically.

## Adopted Phase 6 mutation methods

These entries were added before the corresponding adapter methods. The
interactive schema describes the common error response as JSON
`{"message": "..."}`. The endpoint-specific error statuses below are the
statuses documented by the schema; an undocumented status is still classified
and displayed as an error, never treated as success.

No entry documents an idempotency key or an idempotency guarantee. Tale sends
one request, never automatically retries a mutation after transport failure,
timeout, `429`, or `5xx`, and performs only the listed safe verification read
when the outcome may be unknown.

### Devices

| Operation | Method and path | Scope | Request | Success and response | Documented errors | Verification read and predicate |
| --- | --- | --- | --- | --- | --- | --- |
| delete device | `DELETE /api/v2/device/{deviceId}` | `devices:core` | no body; bearer header; no query | `200`; empty response body | `400`, `500`, `501` for a device not owned by the tailnet, `504`; common JSON error body | `GET /api/v2/device/{deviceId}?fields=all`; `404` verifies absence. A present device is not reported as deleted. |
| set device approval | `POST /api/v2/device/{deviceId}/authorized` | `devices:core` | `Content-Type: application/json`; `{"authorized": true|false}`; required field | `200`; empty response body | `404`, `500`, `504`; common JSON error body | `GET /api/v2/device/{deviceId}?fields=all`; compare `authorized` exactly. |
| configure key expiry | `POST /api/v2/device/{deviceId}/key` | `devices:core` | `Content-Type: application/json`; `{"keyExpiryDisabled": true|false}`; required field | `200`; empty response body | `404`, `500`, `504`; common JSON error body | `GET /api/v2/device/{deviceId}?fields=all`; compare `keyExpiryDisabled` and retain the returned `expires` value. |
| expire current key | `POST /api/v2/device/{deviceId}/expire` | `devices:core` | no body; no query | `200`; empty response body | `404`, `500`, `504`; common JSON error body | `GET /api/v2/device/{deviceId}?fields=all`; require the server-returned expiry state/timestamp to show the key is expired. No reauthentication is attempted. |
| set device name | `POST /api/v2/device/{deviceId}/name` | `devices:core` | `Content-Type: application/json`; `{"name": "..."}`; required field; empty resets to hostname-generated name | `200`; empty response body | `404`, `500`, `504`; common JSON error body | `GET /api/v2/device/{deviceId}?fields=all`; compare the canonical returned `name` and use that value for labels. |
| set device tags | `POST /api/v2/device/{deviceId}/tags` | `devices:core` | `Content-Type: application/json`; `{"tags": ["tag:..."]}`; complete replacement list | `200`; empty response body | `400`, `500`, `504`; common JSON error body | `GET /api/v2/device/{deviceId}?fields=all`; compare the complete returned `tags` set. |

The device name contract says that a name may be a base name or fully
qualified MagicDNS name and that changing it immediately invalidates existing
MagicDNS URLs using the old name. The tags documentation says that tags become
the device owner identity and that applying tags removes a user identity; Tale
shows that consequence as observed ownership context, never as a policy
reachability prediction.

### Routes

| Operation | Method and path | Scope | Request | Success and response | Documented errors | Verification read and predicate |
| --- | --- | --- | --- | --- | --- | --- |
| replace enabled routes for one advertiser | `POST /api/v2/device/{deviceId}/routes` | `devices:routes` | `Content-Type: application/json`; `{"routes": ["CIDR", ...]}`; complete replacement list | `200` `application/json`; `DeviceRoutes` with `advertisedRoutes` and `enabledRoutes` | `404`, `500`, `504`; common JSON error body | `GET /api/v2/device/{deviceId}/routes`; compare the complete `enabledRoutes` set and retain `advertisedRoutes`. Only advertised routes may be newly enabled. |

The public operation explicitly says advertised routes cannot be set through
the API and that routes must be both advertised and enabled to be usable. A
route containing `0.0.0.0/0` or `::/0` is displayed as exit-node capability
according to the returned route data; this action never advertises a route
locally.

### DNS

| Operation | Method and path | Scope | Request | Success and response | Documented errors | Verification read and predicate |
| --- | --- | --- | --- | --- | --- | --- |
| replace nameservers | `POST /api/v2/tailnet/{tailnet}/dns/nameservers` | `dns` | `Content-Type: application/json`; `{"dns": ["resolver", ...]}`; complete ordered replacement list | `200` `application/json`; object `{ "dns": string[], "magicDNS": boolean }` | `404`, `500`; common JSON error body | `GET /api/v2/tailnet/{tailnet}/dns/nameservers`; compare the complete ordered `dns` list. Also refresh preferences because an empty list disables MagicDNS. |
| set DNS preferences | `POST /api/v2/tailnet/{tailnet}/dns/preferences` | `dns` | `Content-Type: application/json`; `{"magicDNS": true|false}` | `200` `application/json`; `DnsPreferences` `{ "magicDNS": boolean }` | `404`, `500`; common JSON error body; enabling without a nameserver is rejected by the documented operation | `GET /api/v2/tailnet/{tailnet}/dns/preferences`; compare `magicDNS`. |
| replace search paths | `POST /api/v2/tailnet/{tailnet}/dns/searchpaths` | `dns` | `Content-Type: application/json`; `{"searchPaths": ["domain", ...]}`; complete ordered replacement list | `200` `application/json`; `DnsSearchPaths` `{ "searchPaths": string[] }` | `404`, `500`; common JSON error body | `GET /api/v2/tailnet/{tailnet}/dns/searchpaths`; compare the complete ordered list. |
| update one split-DNS mapping | `PATCH /api/v2/tailnet/{tailnet}/dns/split-dns` | `dns` | `Content-Type: application/json`; `{"suffix.example": ["resolver", ...]}` to create/replace, or `{"suffix.example": null}` to remove; only named mappings are changed | `200` `application/json`; complete `SplitDns` mapping | `404`, `500`; common JSON error body | `GET /api/v2/tailnet/{tailnet}/dns/split-dns`; compare the complete returned mapping. The shared DNS lock prevents concurrent subresource writes. |

The interactive schema also documents `PUT` for full split-DNS replacement.
Tale deliberately adopts `PATCH` for the three mapping actions because their
intended semantics are one create/edit/remove mapping while preserving other
freshly observed mappings; it does not use `PUT` as a fallback.

### Users

| Operation | Method and path | Scope | Request | Success and response | Documented errors | Verification read and predicate |
| --- | --- | --- | --- | --- | --- | --- |
| approve user | `POST /api/v2/users/{userId}/approve` | `users` | no body | `200`; empty response body | `400`, `403`, `404`, `500`; common JSON error body; user access tokens cannot approve themselves | `GET /api/v2/users/{userId}`; require the exact user status to be no longer `needs-approval`. |
| change user role | `POST /api/v2/users/{userId}/role` | `users` | `Content-Type: application/json`; `{"role": "owner|member|admin|it-admin|network-admin|billing-admin|auditor"}` | `200`; empty response body | `400`, `403`, `404`, `500`; common JSON error body; user access tokens cannot change their own role | `GET /api/v2/users/{userId}`; compare the exact returned `role`. |
| suspend user | `POST /api/v2/users/{userId}/suspend` | `users` | no body | `200`; empty response body | `400`, `403`, `404`, `500`; common JSON error body; user access tokens cannot suspend themselves | `GET /api/v2/users/{userId}`; require `status = suspended`. |
| restore user | `POST /api/v2/users/{userId}/restore` | `users` | no body | `200`; empty response body | `400`, `403`, `404`, `500`; common JSON error body; user access tokens cannot restore themselves | `GET /api/v2/users/{userId}`; require a documented non-suspended status. |
| delete user | `POST /api/v2/users/{userId}/delete` | `users` | no body | `200`; empty response body | `400`, `403`, `404`, `500`; common JSON error body; user access tokens cannot delete themselves | `GET /api/v2/users/{userId}`; exact `id` must be absent. Owned devices and local records are refreshed separately and are never locally deleted as a side effect. |

User role values are the enum in the current `User` schema; Tale does not
invent a hierarchy. The user-roles documentation supplies the role meanings and
the role-management limitations. The audit documentation confirms that these
successful operations are logged, while secondary node/key effects of user
suspension or deletion are not guaranteed to appear as individual audit events.

### Mutation evidence

The exact request/response/error definitions above are from the current
interactive schema:
`https://api.tailscale.com/api/v2?outputOpenapiSchema=true` (rechecked
2026-08-04). Public explanatory sources used for semantics and consequences:

- [Tailscale API](https://tailscale.com/docs/reference/tailscale-api)
- [Trust credential scopes](https://tailscale.com/docs/reference/trust-credentials)
- [Device approval](https://tailscale.com/docs/features/access-control/device-management/device-approval)
- [Remove a device](https://tailscale.com/docs/features/access-control/device-management/how-to/remove)
- [Tags](https://tailscale.com/docs/features/tags)
- [Auth keys and key expiry](https://tailscale.com/docs/features/access-control/auth-keys)
- [User roles](https://tailscale.com/docs/reference/user-roles)
- [User approval](https://tailscale.com/docs/features/access-control/user-approval)
- [DNS in Tailscale](https://tailscale.com/docs/reference/dns-in-tailscale)
- [Configuration audit logging](https://tailscale.com/docs/features/logging/audit-logging)

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
