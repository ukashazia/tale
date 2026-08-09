# Decision 0005 — Semantic theme system

- Status: accepted
- Date: 2026-08-05
- Scope: semantic theme system

## Decision

Tale owns one immutable semantic `Theme` in application state. Views request a
`StyleRole`; only `src/ui/theme/` converts roles into Ratatui colors. The three
built-ins are `tailscale-dark`, `tailscale-light`, and `terminal`, projected at
configuration resolution to truecolor, ANSI-256, ANSI-16, or no-color. Theme
replacement is an in-memory value change before the next complete frame.

This is a Tailscale-inspired visual language, not a copy of the private admin
console and not an endorsement by Tailscale Inc.

## Public provenance

### Brand Toolkit, October 2025 — authoritative for numeric values

`Tailscale_BrandToolkit_10-2025.pdf`, distributed in Tailscale's public media
kit, publishes the canonical palette: seven grays (`Gray0` `#FAF9F8` through
`Gray900` `#1F1E1E`) and seven steps each of blue, green, red, orange, and
purple. Page 6 directs that the 400-range values be used as the primary accent
colors.

This supersedes the CSS artifact below as the numeric reference. The two
disagree: the compiled site CSS exposes a finer scale whose values — `#181717`,
`#232222`, `#ADC7FC`, `#85D996`, `#EFC078`, `#FFB1AB`, `#E3C3FA` — have no
equivalent in the toolkit. The toolkit is dated, distributable, and defines the
brand; the CSS is an implementation detail of one property that can drift.
Tokens now take toolkit values wherever the toolkit publishes a step.

Tale departs from the 400-range instruction for most accent roles, because that
tier is sized for large display type and fills. No 400 value clears 4.5:1 on
both canvases as one-cell foreground text, though all of them clear the 3:1
large-text gate. `Blue400` `#5A82DE` is kept as the selection fill, where it is
a background carrying `Gray900` ink at 4.50:1. The measured basis for the
departure is tabulated in the token ledger.

The toolkit's typography (Inter, MD IO), logo geometry, shape language, and
layout grids do not apply: the typeface is the user's terminal font, and Tale
ships its own ASCII wordmark rather than any Tailscale mark.

### Site CSS, retrieved 2026-08-05 — retained for semantic rationale

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

These CSS values are no longer the numeric reference; they are retained here as
the retrieval record and because the blog post remains the public rationale for
the semantic structure. Where a CSS value and a toolkit value describe the same
token, the toolkit value is used.

The Tale role assignments remain Tale-specific choices and are not represented
as copied Tailscale tokens. Two surface values,
`#2A2929` and `#353434`, are interpolated: the toolkit publishes no step between
`Gray900` and `Gray600`, and Tale needs two elevation levels across that gap.
`selection-ink` is a Tale role name carrying the toolkit's `Gray900`. The
complete distinction is recorded in `docs/design/theme-token-ledger.md`.

## Contrast and projection

WCAG 2.1 relative luminance is calculated from linearized sRGB. Primary and
muted text pairs actually used by Tale pass 4.5:1; normal, state, and focus
boundaries pass 3:1. Disabled text is intentionally lower contrast and is always
paired with a disabled word/symbol and crossed-out or dim-plus-label signaling.
The ledger records measured ratios and the one prohibited pairing: light muted
text is not used on the light inset surface (`4.28:1`).

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
