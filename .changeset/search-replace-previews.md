---
"@brink-lang/studio": minor
"@brink-lang/editor": patch
---

Search replace previews (card-stack PR D). With the replace row open,
every still-matching card renders a display-only old→new preview — the
previews ARE the confirmation; the arm/confirm step is gone. Per-card
Accept applies one replacement (the card keeps its row with a
"✓ replaced" receipt — frozen snapshot); per-card skip excludes it from
Accept all (undo available); the summary strip counts pending/stale/
skipped/replaced and carries Accept all (N). Excluded matches are
per-match (skipped, edited-stale, or failing the live-text guard),
badged with why — never a global abort. The old results-buffer view is
removed from the studio; `@brink-lang/editor`'s `SearchResultsBuffer`
class is deprecated but stays exported for external embedders. Card
chevrons now reuse the fold gutter's glyph in a proper hit target, and
the reveal arrow matches its slot.
