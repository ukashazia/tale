# Specification 07 — Access, credentials, and audit security

- Implementation phase: 7
- JJ change description: `feat: add secure policy and credential workflows`
- Depends on: Specifications 01–06 complete
- Produces: safe policy editing, auth-key creation, credential revocation, and
  audit investigation

This phase handles Tale's highest-risk data. Server validation and preview are
authoritative. Tale preserves policy bytes and never becomes a policy evaluator
or secret archive.

## 07.0 Phase contract

### User-visible result

An authorized operator can edit the exact remote HuJSON policy in their editor,
validate it, inspect authoritative preview/test results, review a textual diff,
save without overwriting concurrent changes, create auth keys with a view-once
secret, revoke supported credentials, and investigate configuration audit
events.

### In scope

- shell-free external-editor handoff;
- candidate/base/remote policy workflow;
- server validation, declared policy tests, and permission preview;
- exact textual diff and guarded save;
- auth-key creation;
- documented credential revocation;
- view-once secret display and explicit clipboard copy;
- audit filtering and cross-resource investigation.

### Explicitly out of scope

- a local ACL/grants parser used to determine reachability;
- automatic policy formatting, normalization, conversion, merge, or repair;
- visual policy generation;
- ACL-to-grants conversion;
- Tailnet Lock, recovery material, OAuth apps, or workload federation;
- storing created secrets in the Tale keyring automatically;
- audit export, flow logs, webhooks, or log streaming;
- arbitrary editor commands executed through a shell.

### Required new ownership

```text
src/admin/policy_mutations.rs
src/admin/key_mutations.rs
src/domain/policy_workflow.rs
src/domain/secret_result.rs
src/ui/views/policy_editor.rs
src/ui/views/secret_result.rs
src/ui/views/audit.rs
src/clipboard.rs
src/temporary.rs
tests/fixtures/admin/policy/
tests/fixtures/admin/credentials/
tests/fixtures/admin/audit/
```

Extend `terminal.rs` for editor handoff and the Phase-6 mutation protocol for
save/revoke. Do not create generic scripting, document-management, or secret-
storage frameworks.

## 07.1 Contract gates

Before code, extend the Control API ledger with exact contracts for:

| Operation | Method and path | Scope |
| --- | --- | --- |
| fetch policy | `GET /api/v2/tailnet/{tailnet}/acl` | `policy_file:read` |
| validate policy | `POST /api/v2/tailnet/{tailnet}/acl/validate` | `policy_file:read` |
| preview policy | `POST /api/v2/tailnet/{tailnet}/acl/preview` | `policy_file:read` |
| save policy | `POST /api/v2/tailnet/{tailnet}/acl` | `policy_file` |
| create auth key | `POST /api/v2/tailnet/{tailnet}/keys` | `auth_keys` |
| inspect key | `GET /api/v2/tailnet/{tailnet}/keys/{key_id}` | type-specific read scope |
| revoke key | `DELETE /api/v2/tailnet/{tailnet}/keys/{key_id}` | type-specific write scope or `all` |

For policy endpoints record exact `Accept`, `Content-Type`, query parameters,
raw-versus-envelope response, validation detail fields, preview selectors, and
save response. For key creation record the complete capability/expiry request
and which response field contains the secret.

Do not begin policy or key implementation from endpoint paths alone. If the
public contract cannot prove byte-preserving HuJSON fetch/save or a credential
type's revocation, leave that action unsupported.

## 07.2 Policy workflow state

Use an explicit, single-owner workflow:

```text
PolicyWorkflow
  profile
  workflow_id
  base: PolicyDocument
  candidate: PolicyDocument
  latest_remote: PolicyDocument?
  temporary_path
  editor_outcome
  validation
  preview
  diff
  state

PolicyDocument
  bytes
  sha256
  content_type
  observed_at

PolicyState
  Opening
  EditingExternally
  CandidateReady
  RemoteConflict
  Validating
  Invalid
  Previewing
  ReadyToApply
  Applying
  Verifying
  Succeeded
  FailedRetained
  Closed
```

Only one policy workflow may be open per profile. Policy bytes are held only by
that workflow and excluded from tasks, logs, snapshots, crash context, and
debug output. The regular Access snapshot retains the latest remote document as
already specified, but never a candidate.

Every validation or preview result is bound to the candidate SHA-256. Changing
one byte invalidates both. A save is permitted only for the exact hash most
recently validated successfully and previewed when preview is supported.

## 07.3 Secure temporary-file lifecycle

Create a private temporary directory under the platform temporary location
using a maintained library and user-only permissions. Create the candidate file
with mode `0600` on Unix before writing. On Windows, use the narrowest user-only
ACL exposed safely by the selected maintained library; if that cannot be
established, disable policy editing with an explicit platform capability.

Required sequence:

1. Create private directory and file without following an existing symlink.
2. Write the exact base bytes, flush, and close the write handle.
3. Record file identity/metadata needed to detect replacement safely.
4. Hand the explicit path to the editor.
5. Reopen without following a symlink where the platform API permits.
6. Reject non-regular files and candidates above 4 MiB.
7. Read bounded bytes and compute the candidate hash.
8. Retain the file while the workflow is open or failed reconciliation is
   needed.
9. Remove the file and directory when the user explicitly closes the workflow
   and on ordinary application shutdown.

Do not claim secure erasure from SSDs or journaling filesystems. If removal
fails, show the exact path and a safe remediation after the TUI is restored.

## 07.4 External editor handoff

Select `$VISUAL`, then `$EDITOR`. If neither is non-empty, the action is
unavailable with setup guidance.

An editor environment value may contain an executable and arguments. Parse it
with a maintained platform-appropriate command-line parser into an explicit
argument vector. Do not evaluate variables, substitutions, redirections,
pipes, separators, aliases, or shell builtins. Append the candidate path as one
argument. Never invoke `sh -c`, `cmd /C`, PowerShell command text, or a terminal
shell.

Use the interactive handoff lifecycle from Specification 03:

1. pause Tale input and rendering;
2. leave raw mode and alternate screen;
3. launch the editor directly with inherited terminal streams;
4. forward supported signals and wait;
5. restore the terminal and force a full redraw;
6. read the candidate even after a nonzero editor exit, then let the user keep,
   reopen, or discard it.

Spawn failure preserves the candidate and restores Tale. No editor failure may
strand the terminal or automatically save policy.

## 07.5 Concurrent-change protection

After editor return and again immediately before apply:

1. fetch the remote policy in the exact same representation as the base;
2. hash its raw bytes;
3. compare with the base hash;
4. if different, enter `RemoteConflict` before validation/apply.

If the API later exposes a documented conditional-write token, it may be added
only through a new ledger decision. Until then, byte/hash comparison is
mandatory but is not represented as a server-atomic compare-and-swap guarantee.
The final remote fetch must be as close to dispatch as possible, and the UI must
state this limitation.

On conflict show a three-document summary and paths for candidate and latest
remote copies. Offer reopen candidate, replace candidate with remote after
confirmation, or close. Do not auto-merge. Do not offer blind overwrite.

## 07.6 Validation and declared tests

Register `admin.policy.validate`. Submit the exact candidate using the ledger's
documented format. Never strip comments, convert HuJSON to JSON, reorder fields,
or reserialize through a local model.

`PolicyValidation` contains:

```text
candidate_hash
validated_at
valid
diagnostics[]
declared_test_results[]
bounded_safe_detail?
```

Preserve server-provided severity, message, line/column/range, test identity,
source, destination, and expected/actual result when present. Unknown location
is valid. Map byte/line positions only against the exact candidate bytes.

The policy file's declared tests are authoritative and may be evaluated as part
of validation or save according to the public contract. Tale does not interpret
the `tests` or `sshTests` sections itself. A candidate with any server-reported
validation/test failure cannot reach ReadyToApply.

Keep the workflow open after invalid results and allow reopening the editor.

## 07.7 Permission preview

Register `admin.policy.preview`. The form exposes only selector dimensions
accepted by the documented preview endpoint. At minimum preserve server-
supported user/source selection and returned destination permissions. Add
ports, posture, SSH, routing, or application capability selectors only when the
ledger proves them.

The result is authoritative server output associated with candidate hash and
preview input. Render matched destinations/rules and source locations when
returned. Render an explicit limitation when the endpoint omits a dimension.

Tale must never:

- calculate reachability by parsing grants or ACLs;
- claim that preview proves runtime service health;
- turn a missing destination into a locally inferred deny;
- reuse a preview after candidate bytes change.

## 07.8 Textual diff

Compute a line-oriented diff from base bytes to candidate bytes using a
maintained Rust diff library. The diff is presentation only; it never produces
save content. Preserve original line text, comments, whitespace, and newline
status.

The diff view includes file hashes, observation times, counts, and unified
context. It is bounded for rendering and virtualized for large files; the
underlying candidate remains complete up to the 4 MiB cap. Search and copy are
allowed. Copied diff content is not persisted.

Do not apply syntax normalization to reduce the diff. A formatting-only change
is still a real change the user must confirm.

## 07.9 Policy apply and verification

Register `admin.policy.apply` as Tier 3. The confirmation shows:

- profile and tailnet;
- base/candidate hashes and timestamps;
- complete diff access;
- current validation/test summary;
- permission-preview summary or documented unavailability;
- the generated confirmation phrase.

At dispatch recheck read-only state, scope, candidate hash, validation freshness
of at most five minutes, and remote base equality. Validate the exact candidate
one final time, then submit exactly one save request. Never retry it.

After a success response, fetch remote policy and compare exact bytes to the
candidate. Exact match is verified success. A semantically equivalent but
byte-different response is `SucceededUnverified` because formatting
preservation is a product contract. A timeout enters outcome unknown and uses
the same fetch comparison.

Keep candidate and latest remote files after failure or mismatch until the user
closes the workflow. Correlate audit asynchronously using Phase 6 rules and
render the server-recorded diff when available.

## 07.10 Auth-key creation

### Form

Register `admin.credential.auth_key.create`. Collect only fields in the current
public request contract:

- description when supported;
- expiry duration within documented limits;
- reusable;
- ephemeral;
- preauthorized/preapproved using current API terminology;
- complete tag set.

Validate incompatible combinations from the documented API. The profile's
OAuth tag grants constrain what the server will accept; Tale does not broaden
them or silently remove denied tags.

### Preview and dispatch

This is Tier 3 because the response contains a credential secret. Preview every
property, expiry timestamp, tags, profile, tailnet, endpoint, and required
scope. Confirmation uses a generated phrase rather than the future secret.

Submit once. Do not retry after timeout. A response without a valid secret is a
failure even if metadata decodes. Never call the list endpoint to “recover” a
secret; it is not recoverable.

### Result

Move the returned secret directly into `SecretResult`, which:

- owns a non-cloning zeroizing secret buffer;
- has no revealing `Debug`, `Display`, serialization, or equality;
- can render only inside the active ephemeral overlay;
- supports one or more explicit copy actions while open;
- records only non-secret metadata and whether copy was requested;
- is destroyed when Close, Esc confirmation, profile switch, shutdown, or fatal
  rendering failure closes the overlay;
- cannot be reconstructed from task history.

Do not automatically store the auth key in Tale's keyring. Refresh credential
metadata only after the secret overlay closes.

## 07.11 Clipboard contract for secrets

Choose a maintained cross-platform clipboard library after checking its current
API. Clipboard access is an effect; the reducer never reads secret contents
back. Secret copy requires an explicit action inside the ephemeral overlay.

Before copying, explain that the operating system clipboard is outside Tale's
memory guarantees and may be observed or retained by clipboard managers. Do not
auto-copy, print OSC 52 sequences, invoke `pbcopy`, `xclip`, PowerShell, or a
shell as an undocumented fallback.

On success show `copied` without the value. On failure keep the overlay open and
allow manual viewing; never place the secret in an error. Tale does not clear
the clipboard automatically because it cannot prove it still owns the current
clipboard value.

## 07.12 Credential revocation

Register `admin.credential.revoke` only when the selected key's type and the
profile scope authorize the documented DELETE endpoint. Fetch detail
immediately before confirmation and show:

- credential type and ID;
- description/owner when returned;
- scopes and tags;
- created, expiry, and last-use metadata;
- known Tale profile references by exact credential ID/reference only;
- known dependents returned by the API, without invented dependency claims.

Revocation is Tier 3 and requires the credential ID suffix or generated phrase.
Submit once and verify the key is absent or documented revoked. A timeout uses
read verification and may remain outcome unknown.

Keep these actions separate:

```text
admin.credential.revoke       remote Control API operation
profile.credential.remove     local keyring record operation
```

Their labels, previews, task records, and help must never use “delete key”
ambiguously. Revoking the credential currently authenticating Tale can end the
session; warn, dispatch once, then clear the token and reauthenticate only if a
different configured credential exists. Do not loop.

## 07.13 Audit investigation

Expand Activity's audit tab with structured filters:

- inclusive UTC start/end time;
- actor stable ID and display value;
- action value;
- target type and stable ID;
- free text over already-decoded safe summary fields.

Server query parameters are used only when documented. Otherwise retrieve the
explicit bounded window and filter locally without claiming server filtering.
Preserve raw stable IDs alongside resolved current names, because resources can
be renamed or deleted after an event.

Cross-resource actions open current Device, User, Route, DNS, Credential, or
Access content only on exact IDs/types. Missing/forbidden resources retain the
event and show the limitation. Policy events render server-provided old/new
source diff without reserialization.

Old/new values pass recursive key-name and type-aware redaction before domain
storage. Unknown potentially secret fields are omitted from detail rather than
rendered optimistically. Export remains unavailable until Phase 8.

## 07.14 Required action IDs

```text
admin.policy.edit
admin.policy.editor.reopen
admin.policy.candidate.discard
admin.policy.remote.refresh
admin.policy.validate
admin.policy.preview
admin.policy.diff
admin.policy.apply
admin.policy.workflow.close
admin.credential.auth_key.create
secret_result.copy
secret_result.close
admin.credential.revoke
profile.credential.remove
audit.filter.time
audit.filter.actor
audit.filter.action
audit.filter.target
audit.open.target
audit.open.policy_diff
```

Candidate discard, workflow close with retained files, apply, auth-key creation,
and revocation must use explicit confirmation appropriate to their effects.

## 07.15 Verification specification

### Unit tests

Cover editor argv parsing without shell semantics, temp permissions and
symlink/non-regular rejection, exact-byte hashing, candidate-bound validation,
conflict transitions, location mapping, diff newline behavior, validation
freshness, final apply guards, auth-key field combinations, secret-safe trait
behavior, clipboard failures, credential-type capability, redaction, and audit
filter/cross-link rules.

### Contract tests

For every policy/key endpoint assert the complete ledger contract. Fixtures
cover valid HuJSON with comments/trailing commas, invalid locations, failing
declared tests, preview limitations, concurrent remote change, formatting-only
candidate, successful exact save, server-normalized mismatch, timeout with
verified save, auth-key success, malformed/absent secret, and every supported
credential revocation type.

No fixture contains a real credential. Secret canaries must be unmistakably
fictional and must fail the test if they reach logs, tasks, errors, snapshots,
debug output, persisted history, or config.

### Terminal and filesystem tests

Use a fake editor executable. Test zero/nonzero exit, spawn failure, signal,
candidate replacement, oversized file, app shutdown during edit, and terminal
restoration. Use task-owned temporary directories and prove ordinary close
removes them. Never inspect or alter user editor files.

### UI tests

Snapshot policy source, validation diagnostics, declared-test failures,
preview, diff, remote conflict, Tier 3 apply, view-once secret, clipboard
warning/failure, credential revocation, and audit investigation at the four
reference sizes. Snapshot tests receive redacted placeholder secrets, while
separate behavioral tests prove the real secret buffer never enters snapshots.

### Required commands

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

Run all earlier checks, forbidden-pattern scans, and secret canary suites.

### Manual acceptance journeys

With fictional API and editor fixtures:

1. Edit a comment only and observe an exact whitespace-preserving diff.
2. Produce a server validation location and reopen the editor at the retained
   candidate.
3. Fail a declared policy test and prove apply is impossible.
4. Preview permissions and see only server-returned conclusions.
5. Change the remote after editing and enter conflict without an auto-merge.
6. Validate, preview, confirm, save, byte-verify, and correlate an audit diff.
7. Return a normalized-but-different policy and report unverified mismatch.
8. Create an auth key, copy it explicitly, close it, and prove it cannot reopen.
9. Fail clipboard copy without including the secret in the error.
10. Revoke a supported credential while keeping keyring removal separate.
11. Filter audit events and open renamed, deleted, and forbidden targets safely.

## 07.16 Exit gate

Phase 7 is complete only when:

- exact remote policy bytes survive every unchanged step;
- concurrent remote change always blocks apply;
- validation, tests, and previews are server-authoritative and candidate-bound;
- no local policy evaluator or automatic merge exists;
- secret results are view-once, nonpersistent, non-debuggable, and absent from
  every non-ephemeral channel;
- clipboard risk is explicit and no shell fallback exists;
- remote revocation and local keyring removal remain distinct;
- cancellation/failure at every workflow step is tested;
- all verification and acceptance journeys pass.

### Primary contract sources

- [Trust credential scopes](https://tailscale.com/docs/reference/trust-credentials)
- [Tailnet policy file](https://tailscale.com/docs/features/tailnet-policy-file)
- [Manage tailnet policies](https://tailscale.com/docs/features/tailnet-policy-file/manage-tailnet-policies)
- [Policy syntax and declared tests](https://tailscale.com/kb/1337/policy-syntax)
- [OAuth clients and auth keys](https://tailscale.com/docs/features/oauth-clients)
- [Configuration audit logging](https://tailscale.com/docs/features/logging/audit-logging)
