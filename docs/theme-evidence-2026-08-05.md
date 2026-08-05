# Theme evidence — 2026-08-05

The automated sanitized buffer matrix passed for 160x45, 110x30, 80x24, and
60x18 plus the 59x17 minimum screen across `tailscale-dark`,
`tailscale-light`, and `terminal` with four color capabilities. Five scenes
(device inventory, invalid command, action transient, help sheet, and Settings
preview) produce 300 complete frames. Additional assertions cover filter cursor
and error cells, Settings preview labels, no-color Reset-only cells,
semantic source/state/risk signals, and terminal restoration regressions. All
data is fictional mock data.

The release-profile `theme_switch_to_160x45_frame` Criterion run measured
819.65–823.90 µs on this host (20 samples after a one-second warm-up), well
inside the 33 ms next-frame budget. The benchmark swaps only the immutable
theme value and renders; it performs no adapter or filesystem operation.

Host observation: macOS `aarch64-apple-darwin`, Cargo 1.97.0, Tale 0.1.0 JJ
change `vootmlny`, `TERM=dumb` classification. An immutable Tale build hash is
not claimed for the working change. This environment provides no named
terminal emulator or interactive screenshot capture, so it does **not** prove
manual truecolor dark/light, terminal, ANSI-16, no-color, or focused-modal
rendering on a Supported platform. `docs/support.md` continues to claim no
Supported platform or terminal rows. Promotion requires the named-terminal
captures and reviewer outcome required by Specification 13.
