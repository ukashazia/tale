# Tale release checklist

This checklist defines the verification and custody required for a release. It
does not authorize publication. A candidate is blocked from a Supported 1.0
claim until every applicable support-matrix row has current platform, client,
terminal, keyring, memory, and release-runner evidence.

## Required verification

Run sequentially on the documented stable toolchain and locked dependency graph:

```text
cargo fmt --all -- --check
cargo check --all-targets --locked
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo doc --no-deps --locked
cargo deny check
cargo run --locked --bin generate-artifacts -- --output-dir .
cargo test --locked --test acceptance --test compatibility --test hardening
cargo bench --locked --bench performance -- --noplot
```

The last two commands are evidence runs, not substitutes for real platform or
terminal evidence. If `cargo-deny` is not installed or its advisory database is
unavailable, the release remains blocked.

## Artifact process

1. Start from an exact source revision with the committed `Cargo.lock` and the
   toolchain pinned in `rust-toolchain.toml`.
2. Generate completions and the man page from the typed CLI definition.
3. Build only target rows that are Supported in `docs/support.md`, using
   isolated target directories and `--locked`.
4. Assemble an archive containing the executable, `LICENSE`, `NOTICE`, README,
   install/support/security/troubleshooting docs, man page, and relevant
   completions. Normalize archive ordering, file modes, and timestamps from a
   release-owned `SOURCE_DATE_EPOCH`; do not embed workspace paths or dirty
   VCS metadata.
5. Produce a SHA-256 manifest beside the archive, then repeat the build from
   identical source and compare archive bytes and checksum bytes.
6. Record target, toolchain, runner, source revision, archive hash, checksum
   hash, and any excluded target in the release evidence.

The current repository has no Supported target row, so no Supported archive is
prepared. A dry run may build the local host binary, but it must not be called a
release artifact.

The deterministic local packager accepts an already-built binary and refuses to
overwrite either output path:

```text
cargo run --locked --bin package-artifact -- \
  --target TARGET \
  --binary target/release/tale \
  --output release/tale-TARGET.tar \
  --checksum release/tale-TARGET.sha256 \
  --source-date-epoch SOURCE_DATE_EPOCH
```

It writes a fixed-order POSIX tar archive containing only the allowlisted
release files and a checksum manifest beside it. It does not sign, publish, or
contact a remote service.

## Tag release automation

Pushing a version tag beginning with `v` starts the
GitHub release workflow. It builds native `aarch64` and `x86_64` artifacts for
macOS and Linux, generates the CLI artifacts, creates raw executables and gzip
and Zstandard payload archives, and builds Linux `.deb` packages. It then
publishes or refreshes a GitHub release with those assets and their SHA-256
files. The publish job uses the protected `release` environment; configure its
required reviewers to retain the approval gate. The workflow uses a fixed
release-owned `SOURCE_DATE_EPOCH` so archive timestamps do not depend on the
runner clock.

Tagging is not an authorization to promote an Experimental support row. Before
approving the protected publish job, a maintainer must verify the assets, sign
them, and record the evidence required to clear every release blocker.

## Fifteen acceptance journeys

The mock/fake adapter suite covers the deterministic portions of these journeys;
real-environment evidence must be recorded separately and must contain no
secrets. Each applicable journey is run at 80x24, keyboard-only, ASCII, and
no-color, followed by the full snapshot matrix. The current row-by-row status
is in `tests/acceptance/journeys.md`.

1. Launch without config and observe a local tailnet.
2. Diagnose direct versus relay behavior and copy a redacted report.
3. Change and verify a local exit node.
4. Configure and remove a private Serve mapping.
5. Enable and disable Funnel with public-exposure confirmation.
6. Add a read-only admin profile and inspect permitted resources.
7. Approve a device and route, then locate audit events.
8. Edit ordered DNS configuration and refresh local diagnosis.
9. Suspend and restore a user.
10. Edit, fail, repair, preview, save, and audit a policy diff.
11. Create an auth key, copy it once, close it, and prove it cannot reopen.
12. Investigate a fleet finding and export filtered evidence.
13. Lose the local daemon while admin mode remains usable.
14. Lose API authentication while local mode remains usable.
15. Cancel process, HTTP, CPU, editor, and streaming tasks and exit with the
    terminal intact.

## Custody, rollback, and blockers

Signing identity, package publication, remote release creation, push, and
announcement are manual maintainer steps. The implementation agent must not
access credentials or perform them. The maintainer verifies checksums before
signing, retains the unsigned artifacts, records the signing identity, and
keeps the prior artifact available for rollback. A failed support row, missing
advisory/license evidence, failed reproducibility comparison, or unavailable
signing authority blocks release; it is not waived in code.
