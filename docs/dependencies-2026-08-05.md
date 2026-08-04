# Dependency, advisory, source, and license review

Reviewed 2026-08-05 with `cargo-deny 0.20.2` and the committed `Cargo.lock`.
The command was:

```text
cargo deny check
```

Advisory and source checks passed. The policy rejects unknown registries and
git sources, wildcards, OpenSSL crates, and known unaccepted advisories. No
advisory exception is present.

The license check is intentionally not marked passed. It found these transitive
licenses that require an explicit maintainer/legal decision before release:

| License | Crates | Dependency path |
| --- | --- | --- |
| `BSL-1.0` | `clipboard-win`, `error-code` | `arboard` |
| `Zlib` | `foldhash` | `ratatui-core` and `serde_json` paths |
| `CDLA-Permissive-2.0` | `webpki-roots` | `reqwest`/`hyper-rustls` |

They remain rejected by `deny.toml`; no legal acceptance is fabricated. The
existing allowed list is limited to reviewed Apache, BSD, ISC, MIT, and
Unicode licenses. Once a maintainer accepts or removes each dependency, the
policy must be rerun from the same lockfile and the decision recorded.

The checker reported transitive duplicate-version warnings for `getrandom`,
`hashbrown`, `syn`, and `windows-sys`. The graph has one `ring`/rustls line;
the warnings are not silently escalated to a broad compatibility change. A
maintainer may tighten `multiple-versions` after reviewing those paths.

Release status: blocked on the four explicit license decisions. This is a
release-readiness blocker, not an advisory pass claim.
