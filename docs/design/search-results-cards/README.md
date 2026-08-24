# Search result cards — design canvas

Approved 2026-08-24 (decision-log: "Search results: stable snapshot,
per-match editable cards with context"). Spec: `docs/search-results-cards-spec.md`.
Live annotatable canvas: published as the "Search Result Cards" artifact.

- `Main.dc.html` — text-search mode: per-match editable cards, context knob
  (1↑ 2↓), collapse + all-buttons, ↻ refresh, `edited` badge (frozen snapshot).
- `References.dc.html` — pinned `decl` card, kind-of-use badges, knot headers.
- `Replace.dc.html` — inline previews, per-card Accept/skip, Accept all,
  replaced/skipped/edited exclusion states.
