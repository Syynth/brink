---
"@brink-lang/studio": patch
---

Three new selectable themes: **Manuscript** — the writing-first colorway (brightest-on-screen prose, hot-red structure markers and halt words, one tight cool machinery band ordered by conceptual distance, yellow tags, cues rendered as plain prose) — plus faithful **Inky** and **Inky Dark** ports of Inky's editor colors. Supporting hooks: `.tok-marker`/`.tok-divert`/`.tok-halt` rules with fallbacks that keep existing themes byte-identical, and theme-tunable cue styling (`--bs-cue`, `--bs-cue-weight`).
