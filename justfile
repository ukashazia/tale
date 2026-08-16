set shell := ["sh", "-eu", "-c"]

check: check-artifacts
    cargo fmt --all -- --check
    cargo check --locked --all-targets --all-features
    cargo clippy --locked --all-targets --all-features -- -D warnings
    cargo test --locked --all-targets
    cargo deny check
    dist generate --mode ci --check

check-artifacts:
    #!/usr/bin/env sh
    temporary_directory="$(mktemp -d "${TMPDIR:-/tmp}/tale-artifacts.XXXXXX")"
    trap 'rm -rf "$temporary_directory"' EXIT HUP INT TERM
    cargo run --locked --bin generate-artifacts -- --output-dir "$temporary_directory"
    diff -ru completions "$temporary_directory/completions"
    diff -u docs/cli/tale.1 "$temporary_directory/docs/cli/tale.1"

[arg("bump", long, help="SemVer release level or version")]
prepare-release bump:
    cargo run --locked --bin generate-artifacts -- --output-dir .
    cargo release version {{ bump }} --execute --no-confirm --allow-branch '*'
