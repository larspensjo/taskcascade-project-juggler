# TaskCascade Favicon — Design

**Date:** 2026-07-10
**Status:** Approved

## Problem

The TaskCascade browser tab shows Chrome's gray placeholder icon because
`frontend/index.html` declares no favicon. The sister project
TickerTapeTallyBoard solves this with an inline SVG data URI in its
`<link rel="icon">` tag — no icon file needed.

## Decision

Add a single `<link rel="icon" href="data:image/svg+xml,...">` element to the
`<head>` of `frontend/index.html`, following the same pattern as
TickerTapeTallyBoard.

**Icon motif (user-selected):** three blue horizontal bars stepping
down-and-right — a literal "cascade" of tasks. Colors come from
`docs/VisualDesign.DarkTheme.md`:

- Background: rounded square (`rx=8`), canvas color `#0a0b0d`
- Bars: accent `#0052ff`, rounded ends (`rx=2`)

32×32 viewBox; geometry chosen so the three bars remain distinguishable at
16×16 tab size.

## Alternatives considered

- **Separate `favicon.svg` in `frontend/public/`** — adds a file and an HTTP
  request for no benefit at this scale.
- **ICO/PNG set** — legacy-browser compatibility we don't need.
- **Motifs:** cascade with a green (`#16c784`) bottom bar, or stacked cards —
  user chose the plain blue cascade.

## Scope

One-line change to `frontend/index.html`. No build, backend, or test impact.
