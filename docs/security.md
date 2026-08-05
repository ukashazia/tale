# Tale security review

Reviewed 2026-08-05. Secret memory is zeroized on drop where the owning type
uses `zeroize`; zeroization is best-effort and is not presented as protection
from an already privileged process, allocator copies, or OS paging.

## Secret-flow inventory

| Secret or sensitive class | Owner and memory | Allowed effects | Redaction/persistence rule | Destruction or error behavior |
| --- | --- | --- | --- | --- |
| OAuth client secret | `SecretValue` in the credential record; zeroizing string | Credential validation and keyring storage | Never displayed, logged, exported, or placed in argv | Dropped with the record; validation errors are classified |
| Access token and environment override | `SecretValue`/temporary environment read | Bearer authentication only | Environment values are never included in doctor, task output, or diagnostics | Temporary value is dropped after the request path; missing/invalid token is an auth error |
| OAuth access-token response | Auth adapter’s secret value | Constructing a short-lived authenticated client | Response body is not persisted as diagnostic data | Dropped after token-manager ownership ends |
| Auth-key result | `SecretResult` with `SecretBuffer` | One-time display and explicit clipboard copy | Redacted metadata only; close makes reopening impossible | Buffer is zeroized on drop; copy failure keeps the result inspectable |
| Webhook signing secret | Secret result produced by the rotate action | One-time display/copy | Not included in webhook inventory, audit text, task output, or doctor | Dropped when the result closes or its owner is dropped |
| Log-stream destination credentials | Secret-bearing log-stream draft/action boundary | Authenticated destination request | Destination inventory and errors contain classification, not secret material | Dropped after request construction; failure is scoped to the stream |
| Clipboard copy | Clipboard adapter receives only the selected secret bytes | Explicit user-triggered copy | No clipboard contents are read back or included in diagnostics | The result buffer remains until close; clipboard lifetime is owned by the OS |
| Private certificate key path/content boundary | Certificate request accepts a path; content is bounded and temporary | Certificate issuance request | Path is not a secret value in doctor; key bytes never enter ordinary logs/exports | Temporary file cleanup and bounded-read errors are explicit |
| Policy and audit content | Domain DTOs and bounded file buffers | Preview, validation, audit correlation, filtered export | Sensitive content is redacted before reports; not included in doctor | Bounded collections are released with their task/state owner |

Every diagnostic and support path is allowlisted. `doctor` includes only safe
metadata, pseudonymized profile/credential names, classifications, and the
documented resolved application paths. It excludes environment values,
keyring content, tokens, policy/audit/flow rows, command stdout/stderr, file
contents, device/user names, addresses, IDs, domains, clipboard contents, and
private certificate material.

## Transport and URL controls

The hosted HTTP client uses maintained `reqwest` rustls defaults for certificate
and hostname verification. The local daemon client uses maintained HTTP/1
machinery over the configured Unix socket or Windows named pipe; it sends the
pinned capability and Host headers, bounds bodies and watch frames at 32 MiB,
and never logs peer data. Credentials are Bearer headers only. URL paths and
queries are constructed through typed URL APIs; request failures are scoped to
the affected resource. Response bodies are bounded before storing error text,
and redaction occurs before diagnostic persistence. No credential-bearing
redirect is accepted as an origin change.

## Filesystem and process controls

Configuration, state, keyring records, and sensitive temporary files use
user-only permissions where the platform exposes them. Atomic writes reject
existing output paths and symlink targets. Process arguments are native
`Path`/`OsStr` values; output is bounded; editor commands are parsed into an
executable plus arguments and are not run through a shell. Tale does not invoke
`sudo`, shell command strings, or downloaded scripts. Terminal handoffs restore
the terminal on ordinary errors and cancellation.

## Static and dependency policy

Repository-authored Rust is checked for executable `unsafe`, `unwrap`,
`expect`, `panic!`, `todo!`, and `unimplemented!` uses, shell launch patterns,
`sudo`, token/Authorization trace fields, and broad debug dumps. Comments,
strings, generated artifacts, and non-Rust fixtures are not treated as
executable source by the scanner.

`Cargo.lock` is committed. `deny.toml` is the policy input for `cargo-deny`:
unknown registries and git sources are denied, and only reviewed permissive
licenses are allowed. The 2026-08-05 run passes advisory and source checks but
has four explicit transitive license decisions pending; see
`docs/dependencies-2026-08-05.md`. An unavailable advisory database or checker
is a release blocker; it is not silently treated as a pass.
