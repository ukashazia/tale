# Specification 09 — Tale 1.0 hardening

- Implementation phase: 9
- JJ change description: `chore: harden Tale for 1.0`
- Depends on: Specifications 01–08 complete
- Produces: a tested, documented, packageable Tale 1.0 release candidate

No new product domain enters this phase. Every change must improve verified
compatibility, resilience, performance, security, accessibility, packaging, or
operator recovery for already shipped behavior.

## 09.0 Phase contract

### User-visible result

Tale behaves predictably across its declared platforms and Tailscale client
versions, remains responsive at representative fleet sizes, restores terminals
after failures, protects secrets through every path, ships complete CLI/help
artifacts, and has a reproducible release process with evidence-backed support
claims.

### In scope

- exact platform/client support matrix and fixtures;
- parser/protocol compatibility gates;
- performance measurement and hot-path improvement;
- fault injection and lifecycle resilience;
- security and dependency review;
- color/symbol/terminal/accessibility matrix;
- completions, man page, doctor/support bundle, installation documentation;
- release automation and dry-run artifacts;
- complete 1.0 acceptance journeys.

### Explicitly out of scope

- new Tailscale resources or API endpoints;
- new customization, plugin, theme, or key-remapping systems;
- compatibility aliases, migrations, fallback parsers, or legacy config repair;
- publishing a release, pushing, signing with user credentials, or changing
  remote state without separate user authorization;
- claiming support for an untested platform or client family;
- weakening a bound or security rule merely to make a test pass.

### Required deliverables

```text
docs/decisions/0003-supported-platform-client-matrix.md
docs/support.md
docs/install.md
docs/security.md
docs/troubleshooting.md
docs/release-checklist.md
docs/cli/tale.1
completions/tale.bash
completions/_tale
completions/tale.fish
tests/compatibility/
tests/acceptance/
benches/
```

CI/release workflow files may be added only for the repository's actual hosting
platform. They prepare and verify artifacts; they must not embed signing,
registry, or publishing credentials.

## 09.1 Support-matrix decision gate

Before changing compatibility code, create Decision 0003 with:

- exact Rust target triples proposed for 1.0;
- minimum tested OS/runtime assumptions;
- exact Tailscale client versions and platforms represented by fixtures;
- Control API contract snapshot date;
- keyring backend and external-editor expectations per platform;
- terminal/input/signal limitations;
- features disabled per platform and evidence;
- CI and manual test evidence required to mark a row supported.

The candidate target set to evaluate is:

```text
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
x86_64-apple-darwin
aarch64-apple-darwin
x86_64-pc-windows-msvc
```

This list is not itself a support claim. A target enters `docs/support.md` as
Supported only after its core-flow matrix passes. Otherwise mark it Experimental
or omit the artifact. Do not add platform fallbacks to preserve a failed claim.

### Client range

Declare a minimum Tailscale version from actual Phase 2–4 fixtures and tests.
Test the minimum patch, every intentionally supported output family, and the
latest client available at release-candidate cut. An untested version is not
listed as supported merely because semantic version comparison accepts it.

For structured outputs, unknown additive fields remain permitted where the DTO
contract already allows them. Missing or changed required fields return
`UnsupportedOutput` with version/platform and preserve last-good state. There
is one selected parser per proven output family—no legacy fallback chain.

### API range

Record a frozen copy of the endpoint ledger source date and fixture shapes. The
hosted API is not version-pinned by Tale; runtime decode failures remain scoped
to the affected resource. A contract change discovered during hardening updates
the ledger and fixtures directly, removing obsolete shapes rather than layering
compatibility.

## 09.2 Cross-platform compatibility suite

### Core-flow matrix

Every Supported row runs:

- CLI/config parsing and platform path resolution;
- terminal enter/restore, resize, Ctrl+C, ordinary error, and fatal render
  error;
- local executable discovery including paths with spaces/non-ASCII text;
- direct process execution, timeout, cancellation, stdout/stderr caps;
- current platform's Tailscale DTO/parser fixtures;
- keyring add/status/remove against an isolated test credential namespace;
- external editor and interactive child handoff;
- temp-file permissions and cleanup;
- JSON/CSV export atomic writes;
- mock-mode acceptance suite;
- admin fake-server suite.

Tests must never use the operator's real keyring records, config, daemon state,
tailnet, or network credentials.

### Platform behavior

Use platform modules only where system behavior genuinely differs. Public
domain/action types remain shared. Reject unsupported signal or permission
semantics explicitly instead of emulating Unix on Windows.

Paths remain native `Path`/`OsStr` values until an API requires UTF-8. Invalid
Unicode paths receive a precise capability error only at that boundary. Never
round-trip paths through lossy strings for process or filesystem operations.

### Fixture manifest

Each local fixture directory includes a manifest:

```text
tailscale_version
platform
command
arguments
exit_code
stdout_file
stderr_file
captured_at
redaction_reviewed = true
```

Fixtures must be fictional or mechanically redacted and then reviewed for
names, addresses, IDs, domains, file paths, and keys. Never commit a raw capture
from a real tailnet. A helper may transform a capture only when the user
explicitly provides and authorizes it outside normal test runs.

## 09.3 Performance budgets

### Reference datasets

Commit deterministic generators, not giant hand-written fixtures, for:

- 5,000 devices and associated users/routes;
- 50,000 audit events;
- 250,000 bounded flow messages;
- 5,000 health findings;
- 4 MiB policy source/diff;
- maximum configured task history.

Generators use fixed seeds and fictional reserved data.

### Interactive budgets

On the documented reference CI runner in release mode:

| Operation | Budget |
| --- | --- |
| input dispatch to next render request | p95 ≤ 16 ms |
| 80x24 frame render from prepared state | p95 ≤ 16 ms |
| 160x45 frame render from prepared state | p95 ≤ 33 ms |
| filter 5,000 devices | p95 ≤ 100 ms off UI thread |
| stable sort 5,000 devices | p95 ≤ 100 ms off UI thread |
| aggregate 250,000 flow messages | p95 ≤ 1 s, cancellable |
| cancellation observed by CPU task | ≤ 50 ms at a cancellation checkpoint |
| mock startup to first frame | p95 ≤ 500 ms |

Use statistically appropriate benchmark tooling and document runner CPU, OS,
Rust version, iterations, and variance. CI may use regression thresholds looser
than the release budget but the release report must satisfy this table.

Rendering never performs filtering, sorting, aggregation, network/process I/O,
filesystem I/O, hashing large documents, or full-list cloning.

### Memory and bounds

Audit every queue, buffer, collection, cache, and history. At minimum preserve:

- event channel capacity from Specification 01;
- process/task output caps;
- 64 KiB admin error-body cap;
- 64 MiB flow-response and 250,000-message cap;
- 4 MiB policy candidate cap;
- configured task-history maximum;
- endpoint page/record limits.

With the largest supported fixture, document peak resident memory on the
reference runner and its component attribution. Do not set an arbitrary pass
number before measurement; the release gate is no unbounded growth across ten
identical refresh/close cycles and no more than 10% retained-memory growth after
returning to the same idle state.

Use borrowing, indexes, interned immutable values, or shared ownership only
when measurements demonstrate the need. Remove avoidable hot-path clones
without introducing unsafe code or lifetime complexity disproportionate to the
measured gain.

## 09.4 Resilience and fault injection

### Fault matrix

Inject at every I/O boundary:

- executable missing between probe and action;
- daemon exit, permission change, hung child, broken stdout/stderr;
- DNS/TLS/connect timeout, disconnect during headers/body, malformed/truncated
  body, oversized body;
- token expiry and keyring unavailability;
- `401`, endpoint-specific `403`, plan restriction, `404`, conflict, `429`, and
  `5xx`;
- config/state/export short write, disk full, permission denial, atomic rename
  failure;
- terminal write failure, resize storm, signal during overlay/handoff;
- editor/SSH child spawn failure and nonzero exit;
- cancellation during every task stage.

Prove last-good snapshots, error isolation, cleanup, and bounded recovery.

### Mutation truth

For every mutation endpoint, test timeout:

1. before request dispatch;
2. while request outcome is unknown;
3. after server apply but before response;
4. during verification;
5. during audit correlation.

No case may duplicate the mutation automatically. UI state is updated only by
fresh reads. An unknown outcome remains inspectable until a later read proves
state or the user dismisses it.

### Refresh storms

Simulate sustained local/admin failure, repeated manual refresh, profile
switching, resume, and recovery. Assert bounded tasks/channels, generation
discard, cancellation, capped backoff, no starvation of input, and one active
refresh per resource generation.

## 09.5 Security review

### Secret-flow inventory

In `docs/security.md`, trace each secret class from entry to destruction:

- OAuth client secret;
- access token and environment override;
- OAuth access token response;
- auth-key result;
- webhook signing secret;
- log-stream destination credentials;
- clipboard copy;
- private certificate key path/content boundary;
- policy and audit content classified as sensitive but not authentication
  secret.

For every transition list owner type, memory container, allowed effects,
redaction, persistence prohibition, error behavior, and destruction trigger.
Back claims with tests; describe memory zeroization as best-effort only.

### Static prohibitions

CI fails repository-authored Rust, including tests, containing executable uses
of:

```text
unsafe
unwrap
expect
panic!
todo!
unimplemented!
```

Use syntax-aware linting or a source scanner that excludes comments, strings,
generated files, and non-Rust fixtures only where justified. Tests must follow
the same Rust safety/error-handling rules as production. Also fail on shell
launch patterns, `sudo`, Authorization/token trace fields, and broad debug dumps
of config/domain structs containing sensitive fields.

No production dependency may require Tale-authored unsafe code. Dependency
internal unsafe is reviewed through maintenance/advisory policy rather than
claimed absent.

### HTTP and URL review

Verify TLS certificate/hostname checking uses maintained library defaults,
tokens are Bearer headers only, redirects with credentials cannot change
origin, paths/query are structurally encoded, response decompression cannot
bypass body caps, and error bodies are redacted before storage.

### Filesystem/process review

Verify user-only config/state/keyring/temp permissions where supported,
symlink-safe sensitive temp handling, atomic writes, no shell, no sudo, native
path arguments, terminal restoration, bounded child output, and no secret in
argv when a secure body/stdin/API mechanism exists.

### Dependencies and licenses

Add a maintained advisory/license/source checker and a committed policy file.
Pin through `Cargo.lock`. Deny known unaccepted advisories, unknown registries,
git dependencies without an explicit documented exception, duplicate critical
security crates where avoidable, and licenses not reviewed in the policy.

The implementation agent may document an advisory exception with package,
version, exposure analysis, compensating control, owner, and expiry; it may not
silently ignore one. Legal license acceptance remains a maintainer decision.

## 09.6 Accessibility and terminal matrix

### Presentation modes

Test complete core journeys in:

- `NO_COLOR`/color `none`;
- ANSI 16;
- ANSI 256;
- TrueColor;
- ASCII symbols;
- Unicode symbols.

Every semantic state has text or a stable symbol in addition to color. Focus,
selection, stale, forbidden, public exposure, risk, errors, and success remain
distinguishable in monochrome snapshots.

### Size and input

At 60x18 show the defined minimum-size or compact interface without panic or
out-of-bounds rendering. Every core read and mutation journey must be completable
at 80x24 through drill-down layouts. Test resize during forms, confirmation,
secret overlay, diff, live task, and terminal handoff.

Keyboard-only operation is mandatory. Mouse remains optional and cannot reveal
an action unavailable to keyboard users. Focus order is deterministic; overlays
trap focus; Esc/back and confirmation behavior remain consistent.

### Terminal evidence

Record manual or automated evidence for the release environment's available
versions of:

- macOS Terminal and iTerm2;
- WezTerm and Alacritty on at least one Unix platform;
- Windows Terminal;
- tmux wrapping one supported Unix terminal.

Check width, Unicode fallback, color, paste/input, mouse opt-in, resize,
clipboard capability, alternate screen, and terminal restoration. A terminal
without evidence is not named supported. Missing access to a platform is a
release blocker or support-scope reduction, not a guessed pass.

## 09.7 CLI artifacts and doctor

### Generated artifacts

Generate Bash, Zsh, and Fish completions plus a `tale(1)` man page from the same
typed CLI command definition used at runtime. Commit generated artifacts and a
test that regenerates them into a temporary directory and requires byte equality.

Help/man/completions include only shipped routes, flags, subcommands, and value
vocabularies. They must not contain secrets, internal mock transport flags, or
research features.

### Doctor command

Complete:

```text
tale doctor [--config PATH] [--mock] [--output PATH]
```

Without `--output`, print a bounded redacted report. With `--output`, preview
and write a deterministic JSON support bundle to an explicit new path; existing
files require confirmation in the interactive TUI or a non-interactive error.

The bundle allowlists:

- Tale version/build target;
- OS/architecture and terminal capability classifications;
- resolved config/state paths without file contents;
- validated non-secret configuration values with profile/credential names
  pseudonymized;
- local executable/client/daemon capability and safe version metadata;
- admin endpoint capability/error classes without URLs containing identifiers,
  bodies, tokens, or tailnet ID;
- bounded recent task metadata without output;
- redaction/truncation manifest.

It excludes environment values, keyring content, tokens, policy, audit/flow
rows, command stdout/stderr, file contents, device/user names, addresses, IDs,
domains, clipboard, and private paths beyond the documented resolved app paths.

Doctor is non-mutating except for an explicitly requested output file and does
not automatically contact support or upload anything.

## 09.8 Packaging and release artifacts

### Build contract

Use locked dependencies and a documented stable Rust toolchain. Build release
artifacts for Supported target triples only. Each archive contains:

- `tale` executable;
- license/notices;
- README/install/support documentation;
- man page;
- relevant shell completions;
- SHA-256 checksum manifest outside the archive.

Strip symbols only when crash/error diagnostics remain actionable. Do not embed
credentials, absolute workspace paths, timestamps not controlled by the release
process, or dirty VCS metadata.

### Reproducibility

On the same pinned runner/toolchain, build twice from identical source and
`Cargo.lock` with isolated target directories and require byte-identical
archives/checksums. Cross-runner reproducibility is documented separately and
is not claimed without evidence.

`cargo install --locked --path .` must work when the selected assets can be
generated/embedded deterministically. If it cannot, document the concrete
reason and make it a 1.0 release decision rather than adding runtime downloads.

### Signing and publishing

Prepare commands/workflows for platform signing and release publication, but do
not execute them, push, create remote releases, or access maintainer credentials
without explicit authorization. `docs/release-checklist.md` identifies every
manual custody step, checksum verification, signing identity, rollback, and
announcement responsibility.

## 09.9 Documentation and recovery

`docs/install.md` covers every Supported artifact, `cargo install` if accepted,
Tailscale prerequisite by mode, profile/keyring setup, least-privilege scopes,
updates, and uninstall without deleting user data by default.

`docs/support.md` is the sole support claim and contains exact target/client/
terminal rows, feature limitations, and evidence date.

`docs/troubleshooting.md` covers executable/daemon permissions without sudo,
authentication/scope/plan errors, unsupported output, damaged config without
migration, secret-result loss, unknown mutation outcome, export/temp cleanup,
and terminal recovery commands. Recovery instructions never require executing
a downloaded script or revealing a support bundle publicly.

README links these documents and clearly distinguishes local installation from
admin-only operation.

## 09.10 Continuous verification gates

The required local/CI sequence is:

```text
cargo fmt --all -- --check
cargo check --all-targets --locked
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo doc --no-deps --locked
```

Also run:

- forbidden-pattern/security scans;
- dependency advisory/license/source policy;
- generated completion/man equality;
- Markdown link and formatting validation;
- secret canary and redaction tests;
- compatibility matrix on applicable runners;
- release-mode acceptance and performance suites;
- artifact double-build reproducibility check.

Checks run sequentially when they share a Cargo target directory. Failures are
not waived in code; any release exception is documented with evidence, owner,
scope, and expiry.

## 09.11 Complete 1.0 acceptance suite

Automate against mock/fake adapters wherever possible and record separate real-
environment evidence without secrets for:

1. Launch with no config and observe a local tailnet.
2. Diagnose direct-versus-relay behavior and copy a redacted report.
3. Change and verify a local exit node.
4. Configure and remove a private Serve mapping.
5. Enable and disable Funnel with public-exposure confirmation.
6. Add a read-only admin profile and inspect every permitted resource.
7. Approve a device and route and later locate their audit events.
8. Edit ordered DNS configuration and refresh local diagnosis.
9. Suspend and restore a user.
10. Edit policy, fail a declared test, repair, preview, save, and inspect the
    audit diff.
11. Create an auth key, copy once, close, and prove it cannot reopen.
12. Investigate a fleet finding and export filtered evidence.
13. Lose the local daemon while admin mode remains usable.
14. Lose API authentication while local mode remains usable.
15. Cancel process, HTTP, CPU, editor, and streaming tasks and exit with the
    terminal intact.

Run applicable journeys at 80x24 with keyboard only, ASCII, and no color. Run
the full visual snapshot set at all reference sizes/modes.

## 09.12 Release checklist

The release candidate cannot be declared ready until a maintainer reviews:

- all prior phase exit gates;
- support-matrix evidence;
- API/client contract ledger dates;
- performance report and retained-memory cycles;
- security/secret-flow review;
- dependency/advisory/license report;
- terminal/accessibility evidence;
- acceptance results;
- install/uninstall/recovery docs;
- artifact contents, double-build hashes, and signing plan;
- known limitations and research-gated exclusions.

The implementation agent may prepare and verify this evidence. Only the user
may authorize publication, signing with real credentials, remote release
creation, or pushing.

## 09.13 Exit gate

Phase 9 and Tale 1.0 are complete only when:

- every support claim has passing evidence and a date;
- every parser/API contract is fixture- and ledger-backed;
- performance budgets and bounded-memory cycles pass;
- no mutation duplicates under injected failure;
- the security review finds no secret path into persistent or diagnostic data;
- no shell, sudo, unsafe, panic, unwrap, expect, or placeholder path exists in
  repository-authored Rust;
- monochrome, ASCII, compact, keyboard-only, resize, and restoration tests pass;
- completions, man page, doctor bundle, install/support/security/recovery docs,
  and release checklist are complete;
- release artifacts reproduce on the defined runner;
- all 15 acceptance journeys pass;
- no new feature was smuggled into hardening.

Actual signing, publishing, pushing, and remote release creation remain outside
this specification until the user gives explicit authorization.
