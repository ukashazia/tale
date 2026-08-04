# Installing Tale

Tale is a local terminal application. This repository currently declares no
Supported release artifact; the commands below are for local development and
for a maintainer’s evidence run.

## Prerequisites

- a documented stable Rust toolchain;
- an installed Tailscale client when local observation or operation is wanted;
- a terminal with keyboard input and an alternate-screen implementation for the
  interactive UI.

The current fixture evidence covers Tailscale 1.98.9 on Linux only. Admin mode
also requires a Control API credential with the least-privilege scopes needed
for the resources being inspected or changed. Read-only profiles should use
read-only credentials whenever the Control API permits it.

## Build or install from this checkout

Use the locked dependency graph:

```text
cargo install --locked --path .
cargo run --locked --bin generate-artifacts -- --output-dir .
```

The second command regenerates the checked-in Bash, Zsh, Fish, and `tale(1)`
artifacts from the typed CLI definition. It does not download runtime assets.

For a local release build, use `cargo build --release --locked` and follow the
artifact procedure in `docs/release-checklist.md`. Only target rows with dated
support evidence may be packaged as Supported artifacts.

## Configuration and credentials

`tale config path` prints the resolved config, state, and cache locations.
`tale config check` validates the selected configuration. Profiles name a
tailnet and a credential reference; credential values belong in the OS keyring
or the explicitly documented environment override, never in a support bundle
or command-line argument.

`--mock` is an internal test route and is hidden from user help and the man
page. It is not a production transport and must not be used as platform
evidence.

## Updates and uninstall

Install a new checkout with the same locked process, then rerun the complete
verification sequence. Do not copy credentials into a new config file.

Removing the binary does not remove user data. To uninstall, remove the
installed executable and separately remove Tale’s profile entries and keyring
records using the explicit `tale auth remove PROFILE` command when they are no
longer needed. Inspect `tale config path` first; never delete the entire state
directory merely to repair a damaged config.
