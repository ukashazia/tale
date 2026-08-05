# Decision 0005 — Semantic theme system

- Status: accepted
- Date: 2026-08-05
- Scope: Specification 13

## Decision

Tale owns one immutable semantic `Theme` in application state. Views request a
`StyleRole`; only `src/ui/theme/` converts roles into Ratatui colors. The three
built-ins are `tailscale-dark`, `tailscale-light`, and `terminal`, projected at
configuration resolution to truecolor, ANSI-256, ANSI-16, or no-color. Theme
replacement is an in-memory value change before the next complete frame.

This is a Tailscale-inspired visual language, not a copy of the private admin
console and not an endorsement by Tailscale Inc.

## Public provenance

The following official public material was retrieved on 2026-08-05:

- Tailscale, [Heart of dark mode: done, and still in progress](https://tailscale.com/blog/heart-of-dark-mode), published 2024-08-29. It documents the public design-system rationale for semantic text classes, focus outlines, two principal backgrounds, disabled treatment, raised-surface borders, and separate light/dark mappings.
- The CSS artifact linked by that page at retrieval time,
  `https://tailscale.com/_next/static/css/4e4372c1029bdebc.css`, SHA-256
  `a117cfe1679971bd8c1781a1a3e7d3aa03fe341554956efad15a47a2db298acd`.
  This artifact exposes the public warm gray, blue, green, orange, red, and
  purple scale values used as numeric references. It is a pinned research
  input only; Tale never downloads or parses it during build, test, or runtime.

Directly observed public facts include `gray-1000` `#181717`, `gray-800`
`#232222`, `gray-700` `#2E2D2D`, `gray-50` `#F9F7F6`, `gray-400` `#AFACAB`,
`gray-500` `#706E6D`, `blue-100` `#ADC7FC`, `blue-200` `#85AAF5`,
`blue-600` `#3F5DB3`, `blue-700` `#324994`, `green-100` `#85D996`,
`green-400` `#09825D`, purple references `#E3C3FA`/`#8052A1`, orange
references `#EFC078`/`#BB5504`, and dark red `#FFB1AB`.

The Tale role assignments, `#585757` dark normal border, and `#B22D30` light
danger mapping are Tale-specific choices from the Specification 13 proposal.
They are not represented as copied Tailscale tokens. The complete distinction
is recorded in `docs/design/theme-token-ledger.md`.

## Contrast and projection

WCAG 2.1 relative luminance is calculated from linearized sRGB. Primary and
muted text pairs actually used by Tale pass 4.5:1; state/focus boundaries pass
3:1. Disabled text is intentionally lower contrast and is always paired with a
disabled word/symbol and crossed-out or dim-plus-label signaling. The ledger
records measured ratios and the one prohibited pairing: light muted text is not
used on the light inset surface (`4.28:1`).

ANSI-256 indices were precomputed with CIELAB perceptual distance against the
xterm palette, then manually reviewed for semantic collisions. A neighboring
index or modifier is used where hierarchy would otherwise collapse. ANSI-16
uses named families plus modifiers and labels; it makes no RGB claim about the
terminal emulator. No-color sets both foreground and background to Reset and
retains meaning using bold, underline, italic, reverse, crossed-out text,
borders, stable symbols, and explicit labels.

## Composition

`StyleComposition` applies meanings in this fixed low-to-high order: base,
source, operational state, risk, selection, active focus, and safety
(secret/redaction). Call order cannot alter precedence. Lower-priority meanings
must survive as their explicit source/state/risk label or symbol when a higher
layer replaces foreground or background.

## Rejected alternatives

- Terminal appearance probing is not claimed: no portable, documented terminal
  contract reliably reports light/dark background, and probing would introduce
  startup I/O and possible escape-response leakage.
- There are no custom files, aliases, arbitrary RGB values, Base16 import,
  plugins, runtime palette downloads, or fallback theme names.
- Opacity and animation are unsuitable for terminal cells and would weaken
  deterministic buffer evidence.

