# Writing-first editor color design

**Status: RULED & IMPLEMENTED** (2026-08-25, decision log "Manuscript
colorway + Inky themes ship as selectable themes"). The design pass ran
live over four mockup rounds; the outcome supersedes the options below:
**option B** — the colorway ships as the selectable "Manuscript" theme
(existing themes untouched), alongside faithful "Inky" / "Inky Dark"
ports. The final system is three temperatures: prose as the page's one
true foreground; hot-red narrative structure markers (`* + [ ] -`) and
halt words; all other machinery in one tight cool band ordered by
conceptual distance. Classifier support landed as the `marker` /
`divert` / `halt` token types. §"Proposed principles" and the options
below are kept as the record of the deliberation.

## Motivation

Field feedback from a working author using the desktop studio on a real
project, paraphrased impersonally:

1. Distinct coloring for machinery is *right* — the author already reads
   ink tooling's colors semantically (a "pauses the output" color vs a
   "keeps going" color) and wants the programmy parts to read as
   programmy.
2. A writing-focused presentation is the goal and current per-element
   work (screenplay layout, prose dialect) serves it well.
3. But the studio's palette "runs together": many similar-toned hues of
   similar weight, hard to parse at a glance.
4. Net feel is a programming IDE (the VS Code / Zed family), not a
   writing app.

So the critique is not "too much color" or "too little" — it is that the
color SYSTEM is structured like a code editor's: one hue per grammar
category, all at equal salience, with prose as just another default.

## Diagnosis of the current system

- Eleven `--bs-syn-*` token colors, assigned by grammar category
  (keyword, operator, enum, parameter, label, …), inherited from the
  LSP-ish semantic-token worldview. In mocha nearly all are
  mid-saturation pastels of similar lightness — hue variety with no
  tonal hierarchy, which is exactly "runs together".
- Prose carries the default text color and default weight — visually a
  peer of the machinery rather than the page's primary content. (Until
  the 2026-08-25 tweak, variables were literally the prose color.)
- Salience is flat: story flow, data plumbing, and meta annotations all
  compete equally, everywhere, always.

## Proposed principles

- **P1 — Prose is the page.** Prose gets the highest-contrast, most
  neutral treatment, and NOTHING else may use the prose color. (The
  binding fix instantiated this rule; adopt it as an invariant.)
- **P2 — Machinery colors by role, not grammar.** Collapse the eleven
  grammar hues into a small number of semantic families:
  - **Flow** — what moves the story: diverts/tunnels/threads, choice
    and gather markers, knot/stitch headers, glue, `DONE`/`END`. This is
    where the author's existing "pauses vs keeps going" reading lives —
    color by *runtime effect* (halts vs continues vs branches), not by
    token spelling.
  - **Data** — bindings and logic: `VAR`/`CONST`/`~` lines,
    expressions, interpolations and their delimiters, list/struct
    machinery. One family, one temperature.
  - **Meta** — comments, tags, TODO. Muted, receded.
- **P3 — Tonal hierarchy over hue variety.** Fewer hues, differentiated
  by saturation and weight. Machinery sits slightly recessed relative to
  prose (muted, not dim); within a family, variation comes from
  weight/shade, not new hues.
- **P4 — Context-sensitive salience (optional, stretch).** Inline
  machinery embedded in a prose line (an interpolation, an inline
  conditional) takes the quiet treatment; dedicated logic regions
  (multiline `{}` blocks, `~` lines, headers) may carry full code
  styling. Costs classifier/structural awareness; worth ruling on
  separately.

## Options for the ruling

- **A. Re-map the existing themes in place.** Mocha/latte keep their
  identity; the `--bs-syn-*` layer is regrouped into the P2 families.
  Cheapest; every existing user sees the change immediately.
- **B. A new "Manuscript" theme, made the studio default.** Implements
  P1–P3 from scratch (possibly not Catppuccin-constrained); current
  themes stay selectable for anyone who wants the IDE feel. Safer,
  allows side-by-side comparison, but splits maintenance.
- **C. Dialect-owned prose + single recessed machinery layer.** The
  element/screenplay presets own all prose presentation; the entire
  syn-token layer collapses to one recessed "machinery" treatment plus
  flow accents only. The most radical writing-first reading of the
  feedback.

## Strawman mapping (illustration only, option-A shaped, mocha)

| Family | Members | Treatment |
|---|---|---|
| Prose | dialogue/action text, cue names via dialect styling | `--ctp-text`, full weight — reserved |
| Flow: halts | `END`, `DONE`, and diverts targeting them | red family |
| Flow: continues | diverts/tunnels/threads/glue | green family |
| Flow: branches | choice/gather markers, choice text sigils | yellow family |
| Structure | knot/stitch headers (already bold) | mauve, weight-differentiated |
| Data | keywords, operators, bindings, numbers, strings-in-logic, delimiters | one warm recessed family (maroon/flamingo range), shade-varied |
| Meta | comments, tags, TODO | overlay/muted |

Open implementation note: "halts vs continues" needs the classifier to
know a divert's TARGET (`-> END` vs `-> knot`) — today's token layer
does not carry that; it would ride the resolution index the classifiers
already consult, or the HIR projection's `target_id`.

## Decisions needed

1. Adopt P1–P3? (P4 separately.)
2. Option A, B, or C?
3. Does the halts/continues/branches flow split match the intended
   authoring mental model, or is a single flow family enough?
4. Scope: editor only, or should the shell chrome (binder symbols,
   status bar, panels) follow the same families?
