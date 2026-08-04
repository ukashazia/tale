# Terminal matrix evidence

Review date: 2026-08-05. The release process had `TERM=dumb` and no attached
named terminal emulator. Automated `ratatui::TestBackend` snapshots and the
fake terminal-control lifecycle passed at 60×18, 80×24, 110×30, and 160×45;
they do not substitute for terminal-product evidence.

| Terminal/environment | Width/Unicode/color/input/restore evidence | Status |
| --- | --- | --- |
| macOS Terminal | Not available in this run | Blocked; not Supported |
| iTerm2 | Not available in this run | Blocked; not Supported |
| WezTerm on Unix | Not available in this run | Blocked; not Supported |
| Alacritty on Unix | Not available in this run | Blocked; not Supported |
| Windows Terminal | No Windows runner | Omitted; not Supported |
| tmux wrapping Unix terminal | tmux not exercised | Blocked; not Supported |
| `TERM=dumb` non-interactive environment | classification only; no TUI claim | Experimental only |

The required evidence run must check width, Unicode fallback, ANSI/truecolor,
paste/input, opt-in mouse, resize, clipboard capability, alternate screen, and
terminal restoration. Missing access is a support-scope reduction or release
blocker, never a guessed pass.
