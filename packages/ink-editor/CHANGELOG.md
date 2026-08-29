# @brink-lang/editor

## 0.17.0

### Minor Changes

- 0d32184: `[project] indent` is now the single source for indentation width, and the
  default when it is unset is **4** (ruled 2026-08-27).

  - `brink-fmt` no longer keeps a default of its own — it defaulted to two
    spaces while the editor indented by four, which is exactly the
    disagreement this setting exists to prevent.
  - `brink fmt` discovers the `brink.toml` for each file it formats.
  - The language server reads the project's width and ignores the client's
    `tabSize`, which would otherwise be a silent second source.
  - The editor's `indentUnit` reads the configured width instead of
    hardcoding four spaces; the indent guides follow it automatically.

  New: `EditorSessionHandle.getConfiguredIndent()`, and `DEFAULT_INDENT` from
  `@brink-lang/editor`.

  Also: the status bar no longer says "— file not analyzed" for a draft
  (#3145), matching the out-of-scope banner it accompanies.

### Patch Changes

- 7603a3e: The editor's prose no longer slides sideways when a file opens. The
  structure-rails gutter was sized by its content, and that content only
  exists once the HIR projection arrives a few hundred milliseconds later, so
  the column grew from nothing and the compensating content padding — which is
  the text's offset — was rewritten by the same delta. The column is now a
  fixed one-lane width that does not depend on the open file's nesting depth
  or on when the projection lands, so there is no growth to compensate for.
  Deeper stacks paint their extra lanes over the neighbouring play gutter,
  which is empty except on the hovered line; the bars live in an
  absolutely-positioned layer and still render every lane at full size. Also
  reclaims 10px of permanently blank gutter on every file.
- b0f5ccf: New `cm.dispatch` perf span times the whole synchronous CodeMirror transaction cycle (state update + extensions + DOM sync) on the main editor view, with the transaction count as meta — added to decompose per-keystroke handler time that no existing `cm.*` span accounted for.
- 8953403: Detached gutters (#3119): in wrapping views the editor's gutters leave CodeMirror's scroller flex/sticky flow, with the horizontal space they vacate paid back as content padding. CodeMirror makes the gutter container a sticky flex child stretched to the full document height, which costs WebKit roughly 5x on every editor layout — a cost paid synchronously on each keystroke and once per frame while scrolling (Chromium is unaffected). Measured on a real ~1,100-line project under WebKit: forced layout 36-40ms → 17ms, felt keystroke latency 48ms → 24ms, long frames 55ms → 35ms. Self-gating: a non-wrapping view keeps CodeMirror's stock layout, since sticky gutters exist to survive horizontal scrolling.
- 19d913a: Diagnostic tooltips get a fixed anatomy and a width cap.

  Both producers — the compiler and the prose checker — now render through one
  shape: a severity/kind label, the message, the fix buttons on their own row,
  and the diagnostic's code as a source tag.

  - **Width is capped**, at the same 460px the hover card has always used, now
    shared through one token so the two floating explainers cannot drift apart.
    The lint tooltip previously had no cap at all, so a long message ran to a
    200-character measure and pushed the fixes out of reach.
  - **Fixes sit on their own row** with 26px targets, hover, active and
    focus-visible states. Inline, a long message pushed them toward the far
    edge, so reaching one meant crossing the whole message without leaving the
    tooltip.
  - **The label carries severity as a word as well as a colour** — the rail
    alone fails a colourblind reader and fails a screenshot pasted into an
    issue, which is how most of these get reported. Prose lints label with the
    checker's rule name (`spelling`), which says more than `info` would.
  - **The diagnostic code is shown.** It was computed and then dropped, so
    there was no way to look a diagnostic up from the tooltip.
  - `info` severity was never themed, so every prose lint inherited the error
    rail and announced a spelling suggestion in the colour reserved for "this
    will not compile".
  - Hover-card rows wrap rather than widening the card, so an `effects` row
    listing several variables no longer fights the cap.

- 8ca39af: Fix gutter clicks below the fold after the detached-gutters change (#3119): the container was pinned `bottom: 0`, capping its box at one viewport height while CodeMirror keeps positioning markers from the document top — so every gutter marker below the fold fell outside its own container's box and silently refused clicks (the ▶ play affordance, fold arrows, host gutter markers) while still painting normally. The box now grows to contain its markers.
- fae5eb5: References in the hover card are now navigable. The cells an `effects` row
  names, and the file in _Defined in_, are links to their declarations —
  clicking one reveals it, the same route goto-definition already used.

  The card named things without letting you reach them, which made it a
  readout rather than a way to move.

  - `HoverInfo` gains `links`, and content refers to them as `[text](#N)`. An
    index rather than a path inside the link target, deliberately: a path in
    markdown has to survive `)` and `:` inside it, and that escaping is a
    silent-corruption bug waiting on the first bracket in a filename.
  - Atoms with nowhere to go stay plain text — `calls` atoms are raw external
    names with no symbol to point at, and the compiler-owned `rng` cell has no
    declaration. A link that navigates nowhere is worse than plain text.
  - An embedder that passes no navigate hook gets plain text too, the same
    rule "Add to dictionary" follows.
  - Effect atoms are now individually code-styled rather than the whole row
    being one code span, and clause labels and status words (`pure, silent,
total`) read as prose.

- 67dd310: Indent guides line up with the column they mark, and break between rows.

  The guides were painted half a character right of their column — literal in
  the upstream package, which appends `.5` to every gradient stop — so a caret
  at that indent sat left of its own guide and read as needing one more space.
  The shift is `0.5ch`, font-relative, because the editor font size is
  user-settable.

  Each row's guide is now slightly shorter than its row, leaving the small
  vertical break between rows that Inky shows.

  Two smaller fixes: Single File view remembers whether you hid the player
  (it reopened on every reload and every switch back from Code view), and the
  "not included in the project" banner can be dismissed — per file, for the
  session, since what it states can stop being true.

- cfa5738: A character cue now teaches the prose dictionary the spelling the prose
  actually uses, and the cue line itself is no longer spell-checked.

  An ink cue is written in caps (`@GRISWOLD:<>`) while the prose that mentions
  the same character is not (`Griswold`), and dictionary matching is literal —
  so seeding the cue's own spelling left every prose mention of the character
  underlined.

  Two halves, and neither works alone:

  - Cue names are seeded in title case rather than as written. Seeding _both_
    spellings does not work: with `["GRISWOLD", "Griswold"]` in the dictionary
    the all-caps use is still reported, because Harper's proper-noun metadata
    drives a capitalization rule that fires regardless.
  - Character-cue lines are excluded from prose ranges. A cue is the speaker's
    name, not prose — the same category as the knot and stitch names prose
    checking already excluded — but an ink cue line is an ordinary content span
    to the HIR projection, so it was being checked. With title-case seeding it
    would now be reported.

  `griswold` in prose is still flagged, which is the point: it is a real
  misspelling of a proper noun. Parentheticals and dialogue lines are still
  checked — those are written prose.

  `@brink-lang/editor` exports `withoutCueLines`, the second half, for hosts
  composing prose ranges themselves.

- 95d3f82: Fix the editor text sliding sideways on the first click after load. The
  detached-gutters layout marked its view with a class added directly to the
  editor element, which CodeMirror owns and rewrites wholesale whenever it
  rebuilds that element's attributes — as gaining focus does. The marker was
  erased on the first click, the gutters fell back from absolute to their
  inline sticky positioning and rejoined the layout flow, and because the
  compensating content padding stayed applied the text jumped right by the
  full gutter width. The marker is now published through CodeMirror's own
  `editorAttributes` facet, so it is reapplied every time the attributes are
  rebuilt.
- 2c2903a: Remember the editor across a reload: open tabs and their order, pin/preview
  state, the active tab per group, the split structure and its sizes, and each
  open document's cursor and scroll. State is scoped per project — the host
  names the scope (`mountStudio`'s `sessionScope`; the desktop passes the
  project root) — so two projects keep their own layouts instead of
  overwriting one another, with a least-recently-used cap on how many are
  remembered. A project with nothing remembered still opens as the default
  two-up, and tabs naming files that no longer exist are dropped on restore.
- 7a6560a: Performance instrumentation ships in all builds (prod-perf ruling 2026-08-25): the probe, browser observers, `__brinkPerf` harvesting global, and the Performance tool window are no longer dev-only — `mountStudio` enables them by default and `perf: false` (or the playground's `?perf=0`) strips the whole surface. The session worker now runs its own probe and wasm counters, reported through new host-level queries (`hostPerfReport` / `hostPerfReset` / `hostPerfSetEnabled` — answered by the hosting realm, never the session facade), and the HUD grows worker-plane and wasm-counter sections plus a combined JSON export; since W5 the analysis cost lives in the worker, so a main-thread-only panel could not see it. The probe's User Timing mirror now periodically clears its own entries (only its own — an embedding page's timeline is untouched), bounding an always-on session's growth. Perf payloads remain structurally content-free: static span/counter names and numbers only, nothing from the author's project.
- 029dae2: Prose checking: spelling and light grammar over a manuscript's prose.

  The engine is Harper, in its own lazily-loaded wasm module — 6.5 MB gzipped,
  larger than the entire compiler, so it is never in the main bundle and an
  embedder that registers no checker pays nothing.

  What makes it usable on fiction rather than hostile to it: the checker only
  ever sees `content` spans with interpolations subtracted (never diverts,
  tags, or logic), and its dictionary is seeded from the project's own names —
  including the character cues, so writing the manuscript teaches it. Without
  that, every invented name reports as a misspelling.

  `@brink-lang/web` gains `getProseDictionary`, `getConfiguredProseDialect`
  and `getConfiguredProseEnable`. `@brink-lang/editor` gains the `ProseChecker`
  seam and a shared diagnostic-source registry, so the compile and the prose
  check no longer overwrite each other's squiggles. `@brink-lang/studio` gains
  the Prose settings section and registers the checker.

- c3ebae8: The author's prose dictionary now lives in `brink.toml`, under `[prose]
dictionary`, and is visible and editable in Project → Prose.

  It previously went to a `.brink-dictionary` sidecar with no UI anywhere, so
  "Add to dictionary" wrote a file nothing displayed — the word stayed
  underlined until the next compile and there was no way to see the list or
  undo an entry. The settings panel now shows the words, adds and removes
  them, and the editor action writes to the same place.

  Matching is literal: `Griswold` and `GRISWOLD` are two separate entries.

  Package-level notes:

  - `@brink-lang/web` gains `EditorSession.getConfiguredProseDictionary()`,
    reading `[prose] dictionary` from the applied config. Like the other
    `configured*` readers it is wholesale-replaced on every apply, so a word
    removed from the file stops being a known word.
  - `@brink-lang/editor` gains a `onAddToDictionary` document-session option
    and no longer owns dictionary storage: the list is the embedder's
    `brink.toml`, so the editor package no longer writes it. The
    `PROSE_DICTIONARY_FILE` export is removed. An embedder that does not pass
    `onAddToDictionary` no longer sees the "Add to dictionary" action at all,
    rather than seeing one that silently does nothing.

- ab5efa5: Spelling and grammar findings now appear in the Problems panel, behind a
  filter toggle that is **off by default**.

  This completes behaviour that was specified when prose checking was first
  scoped — results "render as squiggles and are listable, but the Problems
  panel filters them out by default; the author opts in to seeing them in the
  list". Only the squiggles half had shipped, so a typo was visible in the
  buffer and findable nowhere else.

  - A fourth filter bucket, `prose`, sits beside error/warning/info. It is a
    SOURCE rather than a severity, which is what lets it default off while
    every severity defaults on — folding spelling into `info` would bury the
    E189 TODO notes an author actually reads.
  - Prose findings are stored separately from compile diagnostics and joined
    for display. The two producers have different lifetimes — a compile
    replaces its whole set at once, prose lints arrive per view on their own
    debounce — so one list would mean each erasing the other's rows.
  - A prose row's context menu offers **Prose settings…** rather than
    "Configure <code>…", which would have opened the Diagnostics section and
    offered nothing about it.

  An existing author's stored preferences have no `prose` key, and it reads as
  off: the severity rule ("only an explicit false hides it") is deliberately
  inverted for this bucket, so upgrading never switches spelling rows on.

  `@brink-lang/editor` gains an `onProseLints` document-session callback
  reporting findings per file, fired from the same guarded point as the
  squiggles so a host list can never hold rows the editor has cleared.

- b0f5ccf: Rails-gutter WebKit layout fix: the percent-height inline-flex rail marker made every forced layout cost ~1 ms per visible marker in WebKit (~110 ms per keystroke-burst refresh on a real project — the dominant slice of desktop typing latency; Chromium was unaffected). Markers now use an in-flow fixed-width spacer plus an absolutely-positioned bar layer — same visuals, measured 120 ms → 36 ms full-layout and ~3x lower felt keystroke latency under WebKit. Also: `cm.dispatch`/`cm.dispatch.state`/`cm.dispatch.view` perf spans on the main editor view, `__brinkPerf.report(worstCount)`, and the playground's `?fixtureUrl=` loader for measuring real-project shapes without baking content into the repo.
- Updated dependencies [40e941a]
- Updated dependencies [0b07df5]
- Updated dependencies [b43ebbc]
- Updated dependencies [e4a20b3]
- Updated dependencies [132a3a4]
- Updated dependencies [76bbdeb]
- Updated dependencies [5079c84]
- Updated dependencies [b0f5ccf]
- Updated dependencies [953daff]
- Updated dependencies [0fed188]
- Updated dependencies [cf2d5a4]
- Updated dependencies [237fd39]
- Updated dependencies [42efdf1]
- Updated dependencies [87521b2]
- Updated dependencies [fae5eb5]
- Updated dependencies [0d32184]
- Updated dependencies [cfa5738]
- Updated dependencies [a260c8c]
- Updated dependencies [736061f]
- Updated dependencies [029dae2]
- Updated dependencies [c3ebae8]
- Updated dependencies [b6d2af7]
- Updated dependencies [ef99ec9]
- Updated dependencies [641e278]
  - @brink-lang/web@0.17.0

## 0.16.0

### Minor Changes

- 46f8257: Cmd-clicking a symbol's _definition_ now runs Find References instead of
  a no-op self-navigation — you're already at the definition. Use sites
  keep navigating to the definition; when references are unavailable or
  empty the click falls back to selecting the declaration.
- bc3b08a: File-anchored project open model (epic #3021, ruled 2026-08-23): new
  `entryIsExplicit` option on `ProjectSessionOptions` and
  `MountStudioOptions`. When set, a discovered `brink.toml`'s
  `[project] entry` never supersedes the host-given `entryFile` — the
  #2331 precedence ("`[project] entry` beats `mountStudio`'s `entryFile`")
  stands for host-supplied defaults, but a human's explicit file open is
  not a default. Config discovery itself still runs (lints, conventions,
  warnings all apply). Default `false`, the pre-existing behavior.
- f1b7c76: Literal-whitespace editor presentation (ruled 2026-08-23): the editor no
  longer imposes layout of its own. Removed: standalone-divert
  right-align, the weave-depth artificial indent and its superscript
  depth-sigil collapse (nested `* *` sigil runs now render as typed), the
  screenplay character/parenthetical/dialogue indents and dialogue column
  width, CHARACTER uppercase, and the 8.5in page cap/margins. Colors and
  highlighting are unchanged, and the classification taxonomy (element
  classes, `data-depth`, `brink-divert-standalone`) remains the host
  contract — an embedder that wants a styled layout adds its own CSS over
  those hooks. New: whitespace/tab indent guides
  (@replit/codemirror-indentation-markers), themed from the `--bs-*`
  tokens, spaced at the editor's 4-column indent unit; default on,
  `indentGuides: false` to opt out. New: hanging indent for soft-wrapped
  lines — continuation rows align even with the first row's text start
  (not flush-left, not Inky's extra padding), carried by a `--line-indent`
  custom property per line.
- c5193ad: Performance probe + dev-only HUD (measure-first ruling, 2026-08-24).
  `@brink-lang/editor` gains a perf module — `setPerfEnabled`/`perfSpan`/
  `perfTime`/`perfReport` over a preallocated ring buffer, every span also
  emitted as a `performance.measure` so DevTools recordings show named bars —
  plus browser observers (long tasks, event-timing input latency, long
  frames), a CM6 viewport/scroll probe (`cm.viewportLag`), a wasm-boundary
  Proxy timing every session call (`wasm.<method>`), and spans at the hot
  extension sites (element-type, highlight, HIR overlay + rails gutter,
  inlay hints, folding, screenplay passes, argument widgets, hanging indent,
  inline markup, the debounced compile cycle, project initialize). The studio
  wires the dev edge (`import.meta.env.DEV`): store-write sweep timing
  (`store.set.<field>`), compile fan-out spans, startup marks, a React
  commit profiler, and a "Performance" tool window (aggregates, worst
  events, marks, Copy JSON). Everything is inert single branches when
  disabled — production builds neither collect nor register the HUD.
- ba67f95: Search result cards (card-stack PR C). The Search panel's results render
  as one card per match, in both text-search and references mode: a header
  row (file:line, containing knot/stitch, `edited` badge, reveal ↗) over
  the match's own small editable buffer — the match line plus a tunable
  context window (default 1 above / 2 below), fully syntax-highlighted via
  a per-file semantic-token cache. Cards collapse to a header preview
  (per-card chevron, plus the binder-style expand/collapse-all buttons in
  the summary row alongside the context knob and the snapshot ↻). The
  list is virtualized: off-screen cards render as static HTML instead of
  live editors. Card edits write through to the source and never remove
  rows — the frozen snapshot flags them instead.

  Also fixes cmd/ctrl-click goto/references from a real pointer: the
  handler now binds mousedown (CodeMirror's own cmd-mousedown multi-cursor
  preventDefault suppressed the browser click event a click-bound handler
  was waiting for). And cmd/ctrl-clicking the file path in an INCLUDE
  statement now opens that file.

- e4de0cc: Frozen search snapshot model (search-result cards, PR B). The Search
  panel's result set is now a snapshot: edits never remove or re-filter
  rows. Match spans are edit-mapped through document changes (driven by
  the compile seam), flagging rows `edited`/`stale` instead of dropping
  them; only a new search or the explicit refresh replaces the set. The
  store gains the context-lines setting (default 1 above / 2 below), the
  per-card collapse map, and `refreshSearchSnapshot()` — query snapshots
  re-run their frozen query, references snapshots re-resolve from the
  edit-mapped declaration anchor. The editor's Find References surfaces
  (`onShowReferences`) now pass the symbol's declaration location as an
  anchor when goto-definition resolves one.

### Patch Changes

- 4302f46: Adaptive deferral for advisory paint (#3064 C2): in documents of 1,000+ lines, the HIR overlay and inlay hints map their decorations through each edit (positions stay exact) and rebuild content once the document has been quiet for ~120 ms — a typing burst pays one rebuild at its end instead of one per keystroke. Documents under the threshold rebuild synchronously exactly as before, so small-file behavior is byte-identical.
- eba0faa: Bounded-edit ingress (#3064 C1): `applyEditsDocument(doc, edits)` applies a CM6 change list Rust-side — the full document no longer crosses the wasm boundary on every keystroke, and the write is source-only: the fused eager whole-project analysis that `updateDocument` forced per keystroke (and that nothing on the keystroke path consumed — diagnostics are debounced-compile-driven) is no longer computed until something actually pulls it. The editor's element-type field uses the delta path automatically for single-range edits on file handles, falling back to the full push for multi-cursor batches, fragment views, and older wasm builds/mocks. `updateDocument` is unchanged for compatibility.
- 9bf177e: Context-menu matrix, identity rows: right-clicking any identity-bearing token — divert targets, VAR/CONST/temp/param references, list items, labels, EXTERNAL calls, including refs inside `{interpolations}` — adds a Navigate/Rename group above the text group: Go to Definition (the ⌘-click path), Find References (the ⇧⌥F highlight), and Rename '<name>'… opening the inline-rename UI with its breakage report. The identity test is "goto-definition resolves here", so exactly the tokens with definitions get the group; the actions reuse the same callbacks as their keyboard/mouse counterparts (`navigateToLocation` and `showReferencesAt` extracted as shared entry points). The inline-rename input now hugs its content — the `size` attribute tracks the value, replacing the browser's ~20-char default that rendered as a line-breaking slab — and grows live as you type. The rename UI itself is redesigned as a floating badge below the token (the Zed/JetBrains shape): the symbol stays in the document with a highlight mark while the input floats in a `showTooltip` — which also fixes Escape appearing to delete the token (the old design REPLACED the token with the widget, so its text was hostage to widget lifecycle; now the rename is an inserted editor row and the document is untouched by construction). The row is a block widget beneath the target's line — no gutter number, lines pushed down — with the input rendered as bare inline text: the token's own `tok-*` highlight classes copied onto it, exact column alignment via a hidden spacer carrying the line's real prefix text, no chrome and no focus ring. Escape cancels, and moving the editor cursor off the target's line also dismisses without committing. Structural rows land too: INCLUDE lines offer Open <file>, foldable lines offer Fold/Unfold (through the registered fold service), TODO lines offer Show in TODOs Panel, and Rename is gated per-token by the same prepareRename query F2 uses — externals (whose names are the host-binding contract) get Navigate items with no dead Rename item.
- 8bdb676: The editor context menu is now always ours (docs/editor-context-menu-spec.md, phase 1): right-click anywhere in the editor suppresses the native menu — knot/stitch headers open the shared symbol menu (including **function** headers, whose clicks previously vanished: `headerName` treated the `function` keyword as part of the path), and everything else opens a text menu (Cut / Copy / Paste / Select All with shortcuts, Cut/Copy disabled without a selection) whose actions are bound to the raising view. New `onTextContextMenu` option threads through `brinkStudio`/`DocumentSessions`; the studio renders it via a new `EditorTextMenuHost` sharing the symbol menu's chrome and dismiss contract (`useContextMenuDismiss`, extracted).
- fe696cf: Editor gutter polish: the fold gutter renders one fixed-slot SVG chevron for both states (collapsed = the same glyph rotated by CSS, so the marker never shifts; open-fold chevrons appear only while the pointer is over the gutter, collapsed markers stay visible and accented) via `brinkBasicSetup`, a drop-in copy of `basicSetup` with the brink fold gutter. The play-from-here ▶ is a centered SVG triangle with a hover pill instead of a font glyph. Structure rails show a real floating tooltip on hover — the container's name (bare knot/stitch name, choice/gather text) plus kind and line range (e.g. "Knot · lines 25–31"). Fold and play markers top-align with the first visual row of wrapped lines, matching the line numbers. The shell's corner menu button is an SVG aligned to the strip's icon axis.
- 1609068: Keystroke micro-work toward the 8 ms frame budget (#3064): a config epoch invalidates delta-slice caches on dialect/host-manifest swaps (fixing a stale-classification bug under unchanged segment keys); one manifest fetch per document version; element-type derives per-line infos per segment under the delta protocol's version keys; the keystroke path serves the edited knot's semantic tokens from a classifier-only slice (no analysis pull — the symbol index and resolution passes leave the synchronous path entirely) with resolution-refined colors landing on the deferred refresh; occurrence highlights defer during large-document typing bursts (selection moves stay instant). Per-keystroke instrumented work on a 6k-line document drops to ~6–7 ms, with most keystrokes completing below the Event Timing API's 16 ms reporting floor.
- ee6f0e4: The one-shot analysis family rides the worker road (#3110, closing the last main-thread analysis paths outside documented fallbacks): goto-definition and find-references become async sources (the cmd-click gesture is claimed immediately and lands on resolution — with CM's multi-cursor emulated when nothing resolves), the inline-rename family resolves through the client (`startInlineRename` pre-resolves the target via a resolver facet and dispatches on landing; the live breakage badge lands through `InlineNameInput`'s existing pending machinery; the context menu's identity/rename gating resolves before the menu opens), symbol-tab ranges resolve hint-first with an async worker verify that restores a degraded fragment at its fresh offsets, and search-card highlighting fetches asynchronously (cards render unhighlighted and colorize on landing). The main-thread analysis boundary guard's allowlist shrinks to the choke-point fallbacks only.
- 80dd24f: Outbound delta protocol (#3064 option A): per-keystroke wasm→JS payloads for line contexts and semantic tokens drop from whole-document JSON (~1.4 MB combined on a 6k-line file) to a small segment manifest plus the edited knot's slice. New wasm surface: `getSegmentManifestDoc` (per-segment version keys — salsa identity `index:generation`, stable across shift edits, changed exactly when a segment's content changes, ABA-safe by generation) plus `getSegmentLineContextsDoc`/`getSegmentSemanticTokensDoc` slice fetches. `DocHandle.lineContexts()`/`semanticTokens()` assemble transparently from a version-keyed slice cache — same return types, no consumer changes — and fall back to the whole-document queries for fragment views, native files, older wasm builds, and mocks. Delta-reconstructed results are parity-gated against the assembled queries across the full corpus.
- bd2e490: Two-range model for container spans (#3054): `HirSpan` gains `content_end_line` — the TIGHT end (last line of actual content, trailing whitespace and the next declaration's doc block excluded) alongside the structural `end_line` that runs to the next sibling. Rails and their tooltips use the tight range, so a two-line function no longer paints (or reports) itself through the next function's docs; choice rails get eight golden-step color buckets so siblings are distinct; conditional-branch tooltips show the condition.
- 1b653ff: Performance (#3067, no behavior change): the HIR rails gutter's
  span-by-handle map is built once per projection on the overlay state
  instead of once per visible line inside `lineMarker` — scrolling a large
  file paid O(spans × visible lines) for it (19.7 ms per gutter rebuild
  batch, ~1.5 s per full scroll pass on the perf fixture).
- 79277a6: Find References works — and presents through the Search panel (the spec's open question, now ruled): the menu item and ⇧⌥F route results into the search results surface, grouped by file with line previews, cross-file included, click-to-reveal and inline-editable like text-search results. A references-mode chip names the symbol and count; typing a query returns the panel to text search. (The old in-view 3s highlight painted raw cross-file offsets into the current document — broken by design; it remains only as a fallback for hosts that wire no references surface.)
- f917e08: Hover cards are suppressed while an inline rename row is open, and any
  open card is dismissed when the rename starts. The reworked rename UI
  places the "⚠ breaks N" badge beneath the token — exactly where a hover
  tooltip lands when it flips below (viewport top) — so the card sat on
  top of the badge and intercepted its clicks (caught by the symbol-rename
  e2e; a real-pointer hazard too, since moving toward the badge keeps the
  card alive by hovering it).
- 3a44fe6: Search replace previews (card-stack PR D). With the replace row open,
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
- 2100f62: Session protocol substrate (editor worker architecture W1, `docs/editor-worker-spec.md`): new `SessionClient` async facade, `SessionTransport` interface, `AdmissionScheduler` (mutations before queries, interactive before background, coalesce-key supersession, staleness drops), and `LocalTransport` — an in-process transport that enforces the protocol's JSON-safety contract on every envelope. No existing behavior changes; consumers migrate onto the client in later waves. Wire shapes are mirrored from the Rust source of truth (`brink-web`'s `protocol` module) with cross-language golden pins.
- 37f54ea: Tab indents and Shift-Tab dedents (by the 4-space indent unit), like every other editor. The built-in Tab/Shift-Tab line-conversion cycle (choice→body→gather→choice, character→parenthetical→dialogue, the double-blank `@:<>` template) is stripped for now (ruled 2026-08-24) — previously the keys were swallowed even where no conversion applied. Dialect-DECLARED transition rows keep first claim on Tab (#395 consumer contract; the default at-cue preset declares none). Enter/Shift-Enter transitions are untouched.
- edd0db5: TODO author notes are now visibly highlighted in the editor (#3050). Lines opening with the `TODO` keyword (colon optional, matching the parser's `AUTHOR_WARNING` rule) classify as the new `todo` element kind and carry the `brink-todo` line class; the opening keyword gets a `brink-todo-keyword` mark. The studio styles the class as a full-width amber band with a left bar, forcing syntax-token colors to the amber inside the note so the line reads as one called-out unit (`--bs-todo`/`--bs-todo-rgb` override per theme, falling back to the warning family). The `E189` squiggle is suppressed in the editor — the band is that diagnostic's in-editor presentation — and Info/Hint diagnostics now map to CodeMirror's `info` severity instead of rendering as warning squiggles.
- 07c482b: Diagnostics compile rides the async session facade (editor worker architecture W2a, `docs/editor-worker-spec.md`): `ProjectSession` owns a `SessionClient` (over the in-process `LocalTransport` for now) and gains `compileProjectAsync()` — same generation cache as the sync road, plus in-flight dedup so concurrent views share one compile. The diagnostics extension accepts a sync-or-async `compile` and lands async results under staleness guards (doc moved, view detached, plugin destroyed → the landing is discarded; a newer compile follows). `ProjectSession.destroy()` rejects in-flight client queries before freeing the wasm handle. Embedding hosts passing a synchronous `compile` are unaffected.
- 21500ae: Deferred-refresh consumers ride the async session facade (editor worker architecture W2b, `docs/editor-worker-spec.md`): the quiet-fire for refined highlight tokens, the HIR overlay/occurrences, inlay hints, argument widgets, and fold ranges now runs an async warm-up (`prepare*` options) through the `SessionClient` — background priority, per-surface coalesce keys — and dispatches the refresh effect only when it settles, under landing guards (doc moved or view destroyed → skipped; rejected warm-up → the field's synchronous fallback still refreshes, never stranding a view). Fields themselves are unchanged; small documents keep their synchronous rebuilds. Hosts not passing the new `prepare*` options get the previous behavior exactly.
- b0e3a91: Interactive queries ride the async session facade (editor worker architecture W2c, `docs/editor-worker-spec.md`): completion, hover, signature help, and code actions accept sync-or-async sources, and the studio wiring routes them through the `SessionClient` at interactive priority (after queued mutations, before background pulls; never coalesced or dropped). Completion and hover lean on CM6's native promise handling; signature help lands under sequence + doc-held-still guards (an out-of-order or stale landing is discarded); the code-actions menu opens on landing only if the document and cursor held still. Hosts passing synchronous sources are unaffected.
- b0e3a91: Structural computes ride the async session facade (editor worker architecture W2e, `docs/editor-worker-spec.md`): `ProjectSession` gains `structuralQuery` — an interactive-priority client query for the compute-only structural ops (`renameFile`, `renameDir`, `moveStitch`, `promoteStitch`, `demoteKnot`, `renameSymbol`, `renameSymbolAt`; they return new sources + a breakage report and mutate nothing, so query semantics fit exactly) — and the studio's gated-op runner awaits it, mapping a destroy-during-queue cancellation to the same swallowed result as a destroy-during-defer. The `ProjectSession` file-lifecycle mutations deliberately stay synchronous until the transport flips (sync reads couple to them; recorded in the spec). The paint-path enrolment guard now matches both the facade call shape and the raw wasm call shape, so a raw gated call reappearing anywhere still fails it.
- c0d9a61: `ClassifierSession` — the capability-stripped main-thread session (editor worker architecture W3, `docs/editor-worker-spec.md` §4). The wasm module exports a new single-document session whose surface is exactly the keystroke path's needs — delta/full-text ingress, segment manifest, per-segment line contexts and classifier tokens, dialect config — with no project method exported and write paths that never trigger an analysis pull (parity with the full session's slices is pinned Rust-side). `@brink-lang/web` wraps it as `ClassifierSessionHandle` (feature-detected: `available` is false on older builds and mocks). In the editor, full-file document handles attach a `ClassifierMirror`: the keystroke path's line contexts and fast tokens serve from the classifier's own analysis-free instance (with its own version-keyed slice cache), and the fast-token road blends positionally — cached refined slices keep their colors while uncached (edited) segments serve from the classifier. Mocks and older wasm keep the previous session-road behavior exactly.
- 46a74b3: The session worker (editor worker architecture W4, `docs/editor-worker-spec.md` §8): `WorkerTransport` + a session worker entry running `SessionHostCore` — the exact host semantics `LocalTransport` runs, extracted and shared so the two transports cannot drift — with a boot handshake and crash fallback. `ProjectSession` gains `projectQuery`: the project-level pulls (compile, outline, story graph, closure) run in the worker's own wasm session, kept current by an ordered file/config mutation stream flushed before every worker query; `triggerCompile` (the last synchronous compile caller) rides the async facade. Opt-in via `MountStudioOptions.workerSession` (the playground's `?worker=1`); fully feature-detected — environments without workers, boot failures, and crashes all keep the in-process road. In worker mode the main thread records zero compile time: the whole-project compile (up to ~1.8 s cold on studio-scale projects) leaves the UI thread entirely.
- a844808: The doc-scoped road rides the worker (editor worker architecture W5b, decision log 2026-08-25): the W4 query-time flush becomes a **continuous replica** — every session mutation (file writes, config ops, doc lifecycle, per-doc edits as protocol edit/push messages) forwards to the worker session the moment it happens through the mirror choke point, spawned eagerly at construction so the replica tracks from t0. Doc ids stay aligned by determinism (both sessions mint ids monotonically from the same replayed open/close sequence), guarded by a runtime tripwire: a forwarded open whose replica id mismatches drops the worker and every road falls back in-process. Interactive queries (completion, hover, signature help, code actions), deferred-refresh warm-ups, and structural computes now route to the replica via `ProjectSession.docClient()`. The main session stays fully written — its sync reads remain valid until the W5c delete.
- 69586b3: Worker architecture W5c close-out (`docs/editor-worker-spec.md` §14): the deferred-refresh rebuilds read **worker-fed stashes** instead of pulling analysis on the main thread — `DocHandle` gains per-surface stashes (projection, hints, widgets, folds; dirty-bit guarded so a stash is never served across an edit it predates) and a refined-token worker plane (`refreshRefined`: replica manifest + changed slices only, assembled synchronously at rebuild time); the compile-delivery overlay refresh fetches its projection first and dispatches on landing. Desktop export awaits the async compile landing (fixing the W4-era regression where it read story bytes synchronously after `compile.run`). A new lexical boundary guard pins every surviving main-thread analysis call to a documented allowlist — the one-shot family (goto/rename/symbols/search-cards) stays main-side at incremental cost, tracked by #3110. The synchronous session survives as content store + the in-process fallback road (decision log 2026-08-25).
- 1202806: Web dependency sweep (rides the desktop-perf measure-first work): vite
  6.4 → 8.2 and @vitejs/plugin-react 4.7 → 6.1 across the workspace's dev
  servers/builds, Playwright 1.58 → 1.62, vitest/@types/node current, and
  current minors for the runtime dependencies the published bundles carry —
  the CodeMirror 6 packages (state 6.7, view 6.43, language/lint/search/
  commands/autocomplete), zustand 5.0.15, @floating-ui/react-dom,
  @xyflow/react, @dagrejs/dagre, @fontsource/jetbrains-mono,
  react-resizable-panels, and the react 19.2.x patch line. No API changes;
  the perf scenario suite was re-recorded on the new toolchain and compared
  against the pre-sweep baseline (docs/desktop-perf-baseline.md).
  Deliberately NOT taken: TypeScript 7 and @changesets/cli 3 (majors held
  for their own decisions).
- Updated dependencies [eba0faa]
- Updated dependencies [8bd2fcb]
- Updated dependencies [c0f357b]
- Updated dependencies [d8cfbcd]
- Updated dependencies [1609068]
- Updated dependencies [29541b3]
- Updated dependencies [78e4dd0]
- Updated dependencies [80dd24f]
- Updated dependencies [85a1700]
- Updated dependencies [bd2e490]
- Updated dependencies [9985bcf]
- Updated dependencies [3c8d180]
- Updated dependencies [d043c59]
- Updated dependencies [62fdee9]
- Updated dependencies [c0d9a61]
- Updated dependencies [bfdde5e]
  - @brink-lang/web@0.16.0

## 0.15.0

### Minor Changes

- 8f30776: `[project] entry` now has a real schema slot and wins over a host's `entryFile` argument (issue
  #2331, ruled 2026-08-07 "`[project] entry` beats `mountStudio`'s `entryFile`").

  - `brink_project_config::ProjectConfig` gains `entry: Option<String>`, validated the same way as
    `conventions` (any non-empty string; existence/resolution is left to the consuming mount, kept
    dependency-free per #1234).
  - `EditorSession` (`@brink-lang/web`) tracks the discovered file's `[project] entry` and exposes it
    via the new `configured_entry()`/`EditorSessionHandle.getConfiguredEntry()` — `null` when no
    `brink.toml` was found, or one was found that doesn't set `entry`.
  - `ProjectSession` (`@brink-lang/editor`) now owns entry-file precedence: after
    `discoverProjectConfig` runs, a discovered `entry` that resolves to a real file in the session
    supersedes the constructor's `entryFile` argument (for both `compileProject()` and
    `getEntryFile()`); an `entry` that does NOT resolve to a real file falls back to the current
    `entryFile` and is reported through the existing `onProjectConfigWarnings` channel — no new
    warning channel invented. The `entryFile` constructor option is now only the configless fallback
    (and the seed path `brink.toml` discovery walks up from).
  - `mountStudio` (`@brink-lang/studio`) opens the initial tab from `project.getEntryFile()` (read
    after `initialize()`, so any config supersession has already happened) instead of its raw
    `entryFile` option.
  - `packages/brink-desktop`'s `resolveEntryFile` regex peek at `[project] entry` is deleted (not
    merely unused) — it shrinks to the plain configless-fallback chain, since `ProjectSession` now
    supersedes its guess whenever `brink.toml` sets a valid `entry`. Not independently versioned
    (`@brink/desktop` is private).

  The embedded playground's `?fixture=native` project (`packages/brink-studio/src/main.tsx`'s
  `NATIVE_FIXTURE`) already sets `entry = "story.brink"`, agreeing with the `entryFile` argument
  `main.tsx` passes for that fixture — this change is a no-op there by construction, not by luck; a
  test asserts the agreement holds (`config wins` test in `project-config-application.test.ts`
  additionally exercises a real mismatch to prove supersession, not just agreement).

- 9fc8665: Binder "Library" section for mounted `std/` files (issue #2343, part 2 of #2306's ruling "Mounted stdlib
  presents as a read-only library node" — part 3, session-level read-only enforcement, shipped separately
  in #2342). `list_files`/`project_outline`/`story_graph` (`@brink-lang/web`) switch from **excluding**
  mounted stdlib files entirely (#2231's phantom-row fix) to **listing them flagged** (`mounted: boolean` on
  `ProjectFile`/`FileOutline`/`StoryGraphNode`) — dropping the exclusion without adding a consumer that
  renders the flag would reintroduce the exact phantom-row bug #2231/#2303 fixed, so this ships both
  together. `EditorSession::remove_file` (`@brink-lang/web`) and `rename_file` now refuse a mounted path
  (the delete/rename route gap #2343's review found: previously unreachable only because `list_files`
  excluded the mount from the Binder) — `remove_file` gains a `boolean` return (previously `void`).

  `@brink-lang/studio`'s Binder renders a visually distinct, collapsed-by-default "Library" section below
  the project's own file tree: browsable (expand/collapse a folder tree, click/double-click to open a file
  read-only) but with no drag, rename, delete, or new-file affordances. `@brink/studio-store`'s search slice
  (internal) excludes mounted files from `runSearch`'s candidate list — "Excluded from save-all and
  search/replace" per the ruling — and `ProjectSession.markAllSaved` (`@brink-lang/editor`) does the same for
  `file.saveAll`. The binder slice's `applyMoveResult`/`undo` now surface an `applyEdit` refusal (a structural
  move or undo landing on a mounted path) as a "skipped N read-only file(s)" warning instead of a silent
  no-op behind a success toast.

  A mounted file's CM6 view (`@brink-lang/editor`'s `DocumentSessions`) is now genuinely non-editable —
  `EditorState.readOnly` + `EditorView.editable.of(false)`, the same pattern `conflict-view.ts` uses for its
  "ON DISK" pane — rather than relying solely on the wasm-layer write refusal to make a keystroke silently
  revert. `ProjectSession` gains a public `isReadOnly(path)` query for this. Navigation (goto-def/hover) into
  a mounted file lands in the same read-only view via the existing open-file path — no special-casing needed.

- ce263f8: External deletion of an open file: keep the view, mark orphaned; ⌘S
  recreates (issue #2371, 2026-08-07 decision). `mountStudio`'s
  `onExternalFileChange` used to skip deletions entirely; it now calls
  `DocumentSessions.markOrphaned`, which never touches the kept editor buffer
  (no refresh, no auto-close) and recreates the file in the wasm session from
  that buffer so IDE queries and a later save keep working. `FileChangeHub`
  gains an `orphaned` path set (`isOrphaned`/`orphanedPaths`, mirroring the
  existing `conflicted` tracking) — set by `noteOrphanRecreated` once a kept
  buffer is confirmed to survive a deletion (`applyExternal(path, null)`
  alone does not flag it, so a headless deletion with no open view never gets
  permanently badged), cleared by a canonical save (`markSaved`, or a
  write-through `flush()`) or by the path reappearing on disk. New
  `ProjectSession.recreateOrphaned` (the
  provider is deliberately not notified until a real save, so recreation
  stays gated on ⌘S even for a provider whose `onFileChanged` is itself the
  persistence step) and `isOrphaned`/`orphanedPaths` pass-throughs. New
  `StudioApi.getOrphanedFiles()`, mirroring `getDirtyFiles()`, for a host to
  render an orphaned-tab badge.
- 733e3ec: `FileProvider.renameFile` now receives the moved file's rewritten source
  (issue #2425).

  The rename op folds the moved file's own outbound `INCLUDE` rewrites into
  `new_source`, but the atomic-rename branch of `ProjectSession.renameFile`
  passed only the two paths on — so a host whose `renameFile` moves bytes
  (a real filesystem rename) kept the pre-rewrite text in storage, while the
  `createFile` + `deleteFile` fallback branch had always written the rewritten
  source. The optional third parameter, `newContent`, closes that gap:

  ```ts
  renameFile?(oldPath: string, newPath: string, newContent?: string): Promise<void>;
  ```

  It is optional and additive — an existing implementation declaring only
  `(oldPath, newPath)` still satisfies the interface and behaves exactly as
  before. `InMemoryFileProvider` now stores `newContent` when supplied.

  `@brink-lang/studio` re-exports `FileProvider` through `mountStudio`'s
  `MountStudioOptions.provider` (`packages/brink-studio/src/mount.tsx`), so an
  embedder that supplies its own provider and implements `renameFile` is
  affected by the new third argument: it can keep taking two parameters and
  see no change, or add the third to persist the rewritten source the way
  `InMemoryFileProvider` and `TauriFileProvider` now do.

- 255cf53: `ProjectSession` gains `readProviderFile(path)`, a thin pass-through to the
  provider's existing `FileProvider.readFile`, bypassing session state. It
  lets a caller confirm what a host write actually persisted rather than
  assuming a pre-save snapshot still matches (issue #2435) — used by
  `@brink-lang/studio`'s `file.save`/`file.saveAll` guard.
- e69d48c: **Breaking:** `ProjectSession.renameFile` now resolves `Promise<RenameFileResult>`
  instead of `Promise<string[]>` — a consumer doing `(await project.renameFile(a,
b)).length` or iterating the resolved value directly will break at runtime.
  `packages/ink-editor/src/index.ts` also gains two new exported types,
  `RenameFileResult` and `RenameDirResult`.

  Surface the rename/move breakage gate at the Binder's rename call sites (issue #2918).

  `ProjectSession.renameFile`/`renameDir` (`@brink-lang/editor`) run the same
  safe-by-default breakage gate every other structural op does (#316): the
  wasm `rename_file`/`rename_dir` ops already compute `safe` and
  `introduced_diagnostics` correctly. But both methods used to resolve with
  only the bare data a caller needed to apply the move (a referrer path list,
  or `{ moved, referrers }`) — discarding the breakage-gate verdict entirely.
  A move that broke a reference (a divert pointing at the renamed file, for
  example) applied exactly like a clean one, with nothing anywhere telling the
  user.

  `renameFile` now resolves with `{ referrers, safe, introducedDiagnostics }`;
  `renameDir` with `{ moved, referrers, safe, introducedDiagnostics }`. The
  Binder's `applyRename`/`applyDirRename` (`studio-store`'s binder slice,
  bundled into `@brink-lang/studio`) thread the verdict through to the same
  `_notify` channel PR #2916 used for a refused move: a `safe: false` result
  now raises a `warning`-severity "breaks N reference(s)" notification instead
  of the unconditional `info` toast every rename got before. This is the
  notification FLOOR, not a preflight gate — the move still applies (the undo
  entry still gets pushed) exactly as it did before; the user is now told
  about the breakage rather than discovering it later. The fuller "will break
  N references" preflight/confirm pattern (#324) exists for the editor's
  inline symbol rename, on a dedicated widget the Binder's type-a-new-name
  tree rename has no analog of — building one is out of this fix's scope; see
  issue #2918 for the follow-up.

- 658e7a6: Exported `scheduleIdleWork`/`cancelIdleWork` (the off-paint-path scheduling helper #722 added
  for the inline rename widget) from the package's public entry point, so other rename/analysis
  surfaces can take the same discipline instead of re-implementing it (issue #696).
- 18da64e: Overlay persistence for embedding hosts (the celeris file model, 2026-08-07
  decision): `FileChangeHub`/`ProjectSession`/`mountStudio` gain an
  `egressPersists: false` contract under which `onFilesChanged` delivery feeds
  a backup ring rather than counting as persistence — dirty then means
  "diverges from the last canonical save" and only the save commands clear it
  (an undo back to the saved text drops to clean). New `OverlayPersistence`
  coordinator in `@brink-lang/editor`: routes egress batches to a
  host-provided `BackupSink` (ring bounds are sink-owned), owns canonical
  `save`/`saveAll` (write + re-baseline, rejected writes stay dirty for
  retry), and an autosave scheduler where an autosave tick IS `saveAll` —
  one save path, one artifact class. The default (`egressPersists` absent)
  is byte-identical to the previous write-through behavior.

### Patch Changes

- e3ae45a: Argument Form: an unregistered semantic type's field label now shows the
  same honesty marker hover/signature help use, instead of a bare, confident
  type name (issue #1053, extending #1027).

  `FormField` gains an optional `typeDisplay` — when the brink-ide-supplied
  `CallWidgetSite` carries it, the Form's label renders it in place of the raw
  `typeName` (e.g. `id: var_id ⚠ unregistered semantic type — E040`); a
  registered type's label is unchanged. A producer that hasn't upgraded (no
  `typeDisplay`) still gets the previous bare-name label — this is additive,
  not a breaking change to `FormField`.

- 24fa48f: Issue #2134 review finding: add the `cue` completion kind (issue #2134's
  new `CompletionContext::CueName` items) to `completionType`'s `KIND_MAP`,
  mapping to `"constant"` (matching the LSP side's
  `CompletionItemKind::CONSTANT`). Without this entry a cue completion row
  silently fell back to `"text"`, mis-rendering the row's icon and disabling
  auto-open-on-completion (#229) the same way a missing `value` entry did
  before #174 added it.
- 62dba1d: Session-level read-only enforcement for a mounted stdlib file (issue #2306, ruled 2026-08-06 "Mounted
  stdlib presents as a read-only library node", part 3 of the ruling — built first per its own sequencing
  note). #2231/PR #2303 mounted the stdlib into `EditorSession` and hid mounted files from
  `list_files`/`project_outline`/`story_graph`, but a by-id route that resolves a file outside those three
  listings — a doc handle opened via goto-def navigation into an inherited symbol, or a bulk TS-level caller
  like project-wide search/replace — could still write through to the mounted copy and hand the edit to the
  host to persist, silently forking the stdlib into the project.

  `EditorSession` (`@brink-lang/web`) gains `is_read_only(path)`, and `update_document` /
  `auto_import_apply_include_doc` now refuse (returning the existing "did not apply" sentinel for each —
  `"null"` and `{ ok: false, error }` respectively) when the handle's file currently resolves to a mounted
  id — `open_document`/`open_fragment` still succeed on a mounted path, so it stays browsable/openable, only
  writing through the handle is rejected. `update_file` is deliberately left unguarded: it is the host's
  whole-file "this is the content now" API, and a real project file placed at a mounted key must keep
  winning by construction-time ordering (the existing shadowing contract). `update_source` — the singleton-
  session sibling, including its fragment-splice branch — is **also** left unguarded in this PR: it has no
  in-repo caller today, but as published `@brink-lang/web` surface an external embedder driving the
  singleton API can still reach the same silent-fork hole this PR otherwise closes. That gap is not fixed
  here; tracked as a known follow-up rather than guessed at.

  `EditorSessionHandle.isReadOnly` (`@brink-lang/web`) exposes the new query. `ProjectSession.applyEdit`
  (`@brink-lang/editor`) — the shared seam every bulk-edit caller (search/replace, results-buffer edits,
  binder undo) already routes through per issue #137 — now checks it before writing and returns `boolean`
  (previously `void`) so a caller can react to a refusal instead of assuming success.
  `ProjectSession.initialize()`/`addFile()`/the external-change handler are unaffected: they call
  `session.updateFile` directly, exactly like a legitimate shadow write.

  `@brink/studio-store`'s search slice (internal, not independently versioned) surfaces a refusal from the
  three `applyEdit` callers (`replaceSearchMatch`, `replaceAllSearchMatches`, `applySearchRowEdit`) as a
  "read-only" notification instead of silently continuing.

- d9a83d3: `brink.toml` is no longer inert (issue #2324). `EditorSessionHandle.applyProjectConfig`/`discoverProjectConfig`
  (#1005, #1414) were exposed and unit-tested but nothing outside test files ever called either, so every
  `[project]`/`[lints]` key in a mounted project's `brink.toml` was silently ignored end to end.

  `ProjectSession` (`@brink-lang/editor`) now calls `discoverProjectConfig` — chosen over `applyProjectConfig`
  because it walks the session's own already-loaded documents, so no host-specific directory-walk/read code is
  needed — once during `initialize()` (before the first analysis) and again whenever a `brink.toml` anywhere in
  the session is created, edited, renamed into/out of, or externally rewritten. A new optional
  `ProjectSessionOptions.onProjectConfigWarnings` callback forwards the unrecognized-key/lint-code warnings from
  each call.

  `mountStudio` (`@brink-lang/studio`) wires that callback into the Output tool window, so a typo'd or
  unrecognized `brink.toml` key is now visible instead of silently dropped. `[project] entry` is one such key:
  `brink_project_config::ProjectConfig` has no field for it at all (verified against
  `crates/internal/brink-project-config/src/lib.rs`), so it always reports as an unrecognized key — `mountStudio`'s
  explicit `entryFile` argument remains the only thing that decides the compiled entry file; there was nothing at
  the wasm-session layer for it to conflict with.

  **Review-finding fix:** `discoverProjectConfig` throws on malformed TOML or a recognized key with an
  invalid value, and that throw was unhandled — a typo'd `brink.toml` aborted `mountStudio` entirely (no
  editor to fix the file in), or, once mounted, threw out of every subsequent keystroke's debounced
  `notifyFileChanged`/`applyEdit` call. `ProjectSession.applyProjectConfig` now catches the throw at its
  single call site and reports it through a new optional `ProjectSessionOptions.onProjectConfigError`
  callback instead of rethrowing; `mountStudio` wires it into the same Output channel as the warnings.

- b50c5a1: The in-editor name prompt (`InlineNameInput` — the shared widget behind F2 inline rename and
  extract-to-knot/function, issue #2535) no longer selects text you have already typed. It focuses
  its input from a `setTimeout(…, 0)` scheduled while the widget is still detached, and the
  `select()` that rode along was unguarded: typing during that window left your text selected, so
  the next keystroke replaced it and the rename committed the wrong name — silently, since the
  rename itself still succeeded. `select()` now runs only while the field still holds the value the
  widget seeded it with, matching the guard `SymbolRenamePrompt` took in #2523. The deferral itself
  is unchanged and still required: `render()` is called from CodeMirror's `WidgetType.toDOM()`,
  which returns the element before the view inserts it, and focusing a detached element does
  nothing.
- 25534e6: Two internal correctness fixes with no observable behavior change, both found by PR #2548's review (#2557, #2558):

  - `InlineNameInput.dispose()` (the shared widget behind F2 inline rename and extract-to-knot/function)
    now clears its two remaining deferred `setTimeout(…, 0)` handles — the post-mount focus timer and the
    breakage-report force-button focus timer — alongside the debounce timer and idle handle it already
    cleared, matching the class doc's "tears them all down" claim. Applied the same pattern to the two
    sibling sites with an identical unguarded post-teardown focus timer: `ExtractPrompt` (`extract-actions.ts`)
    and `InlineRename` (`rename.ts`). All three were latent, not live — the owning DOM is already detached
    by the time any of these timers fire, and `focus()` on a detached node is a no-op — but each timer is
    now cancelled on teardown so a future change to the callback can't turn the latent leak into a live one.
  - `RenameQueryCache`'s cache-key separator is no longer a literal NUL byte. The old
    `` `${path}\x00${offset}\x00${newName}` `` made `rename.ts` register as a binary file to `grep`/`rg`
    without `-a`/`--text`, silently hiding the file's own lines (including this method's) from any
    repo-wide sweep. The key is now `JSON.stringify([path, offset, newName])` — provably collision-free
    (JSON.stringify of a fixed 3-element array is injective) and, unlike `\x00`, keeps the file plain
    greppable UTF-8 text.

- fbdb3fb: Wire the Binder's folder rename to the atomic `rename_dir` op (issue #2587).

  The Binder's folder-rename action (`renameFolder`, `packages/studio-store/src/slices/binder.ts`,
  bundled into `@brink-lang/studio`) looped a per-file `renameFile` call over
  every file under the folder — the exact pattern `rename_dir` (#314) was built
  to replace, because a per-file loop computes each file's cross-file INCLUDE
  edits independently, against whatever has already moved, rather than against
  one pre-move snapshot. Concretely: a folder move that only changes the
  directory prefix (every moved file keeps its own basename) left an outside
  referrer's `INCLUDE` pointing at the old, now-nonexistent path, because a
  same-basename rename never triggers the per-file op's basename-keyed
  cross-file rewrite.

  `ProjectSession` (`@brink-lang/editor`) gains `renameDir`, the directory
  analog of `renameFile`: it calls the atomic wasm `rename_dir` op (unused by
  any TS caller since #314 landed), applies every moved file's content plus
  the outside referrers' rewrites from that one snapshot, and writes each
  moved file through the provider (a provider write is inherently per-file —
  the atomicity guarantee lives in the edit computation, not in these writes).
  Deferred off the paint path via the same `deferGatedCall` yield `renameFile`
  uses (#2776), since `rename_dir` runs the identical breakage gate.

  `renameFolder` now calls `project.renameDir` instead of looping
  `applyRename`. All-or-nothing failure semantics (a deliberate change from
  the old loop's silently-skip-a-collision-and-move-the-rest behavior): a
  partial directory move can only be computed by falling back to per-file
  INCLUDE rewriting for the files that "succeed," which is exactly the
  inconsistency #314 exists to prevent, so a collision now refuses the whole
  move with one error notification and nothing moves. Undo gets a new
  `rename-dir` entry kind that re-applies `renameDir` with the prefixes
  swapped, so undoing a folder move gets the same single-snapshot consistency
  guarantee the forward move does, instead of falling back to a per-file undo
  loop.

- 2a9abb7: `ProjectSession.renameFile` no longer runs its gated wasm call (`rename_file`, which runs the
  same full-project breakage gate as the knot/stitch structural ops) synchronously on the paint
  path (issue #2776, generalizing #2767/#722's remedy). The wasm call is now deferred to the next
  idle slot via `scheduleIdleWork`, so under CPU contention a file/folder rename or move (the
  Binder's inline rename, drag-move, and multi-select move all go through this method) no longer
  blocks the main thread inline in the same frame as the triggering event. Callers that render a
  busy indicator while awaiting `renameFile` (the studio's `applyRename` commits `structuralOpPending`
  synchronously before the call) now get a real paint of it before the heavy work begins; callers
  that don't render one see no behavior change beyond the deferral itself.
- cd03b9e: Fixes stuck/unescapable menus and popovers (#279).

  The global Escape safety net added alongside the capture-phase dismiss fixes
  (`dismiss-registry.ts`) previously attached its own listener on `document` in
  the capture phase — the same phase every individual surface uses for its own
  dismiss listener. Because the net installs once, on the very first surface
  that ever registers, it ended up running _before_ a surface's own listener on
  every subsequent open: on the code-actions menu (Ctrl-./Cmd-.) this stripped
  focus return and let Escape leak to CodeMirror's keymap; on the argument-
  widget popover/modal chrome it defeated their own `preventDefault()`/
  `stopPropagation()` outright. The net now attaches on `window` in the bubble
  phase, so it only ever runs after every capture-phase listener already had a
  chance to handle the event — restoring each surface's own dismiss behavior
  while keeping the net's resilience against an orphaned listener intact.

  Also: `InlineNameInput` (the shared F2-rename / extract-to-knot inline
  prompt) is now wired into the same safety net — its own Escape handling was
  scoped to the `<input>` element, so Escape did nothing while the breakage
  report's force-override button (a sibling subtree) held focus; and the
  inline element-type picker's (`keybindings.ts`, Alt+Enter) outside-dismiss
  listener moved from a bubble-phase `mousedown` to a capture-phase
  `pointerdown`, matching the dismiss contract everywhere else.

- 0f1a4ff: Two structural gaps in the paint-path-defer family (issue #2794, found by
  #2788's adversarial re-review — "the enrolment family's gap, not this PR's").

  `ProjectSession` (`@brink-lang/editor`): a gated call deferred via
  `scheduleIdleWork` (today, `renameFile`) could outlive `destroy()` — an
  unmount landing inside the deferral's idle window let the scheduled callback
  fire anyway and call into a wasm handle `destroy()` had already freed. This
  was contained (the throw surfaced as an ordinary error notification through
  `applyRename`'s existing `catch`), not unreachable, but containment is not a
  fix. `deferGatedCall` (replacing a bare `scheduleIdleWork` await) now
  tracks its idle handle and rejects the caller's `await` — instead of
  resolving into a freed session — if `destroy()` runs first; `destroy()`
  cancels every still-pending handle and rejects its caller before freeing the
  wasm handle. One guard, meant to cover every gated call this class defers,
  present or future — including `runGatedStructuralOp`'s symbol-menu ops
  (`moveStitch`/`promoteStitch`/`demoteKnot`, in `@brink/studio-ui`), which a
  follow-up review found still deferred through their own independent
  `scheduleIdleWork` yield outside this guard; `deferGatedCall` is public for
  exactly this reuse.

  `structuralOpPending` (`@brink/studio-store`, bundled into
  `@brink-lang/studio`): two independent fire-and-forget writers
  (`runGatedStructuralOp` for symbol-menu ops, `applyRename` for Binder
  rename/move) both cleared this status-bar pending indicator unconditionally
  in a `finally`. An overlapping Binder drag-move and symbol-menu op could
  erase each other's still-live indicator, whichever settled last winning
  regardless of which op was actually still running. `SymbolMenuSlice` gains
  `clearStructuralOpPending(description)` — a compare-and-clear that only nulls
  the field when the live value still equals the description the clearing
  call itself set — and both writers now clear through it instead of an
  unconditional clear. `setStructuralOpPending` is narrowed to take only
  `string` (no caller ever passed `null`), so a future regression back to the
  unconditional shape fails typecheck instead of relying on review attention.

- Updated dependencies [3b94ac6]
- Updated dependencies [462f61b]
- Updated dependencies [87fe945]
- Updated dependencies [f7e54e3]
- Updated dependencies [e3ae45a]
- Updated dependencies [f36faf9]
- Updated dependencies [f71aa3d]
- Updated dependencies [ae7b829]
- Updated dependencies [5a95959]
- Updated dependencies [39f3801]
- Updated dependencies [bb503cc]
- Updated dependencies [aeebad7]
- Updated dependencies [4fd4658]
- Updated dependencies [640d1d1]
- Updated dependencies [3ddd90e]
- Updated dependencies [f87adc2]
- Updated dependencies [199c822]
- Updated dependencies [8add320]
- Updated dependencies [1d5c985]
- Updated dependencies [1ef7797]
- Updated dependencies [319f9dc]
- Updated dependencies [257e7a9]
- Updated dependencies [c852cbe]
- Updated dependencies [cf076d5]
- Updated dependencies [2f0b5cf]
- Updated dependencies [4bae57f]
- Updated dependencies [9b1d832]
- Updated dependencies [ec58199]
- Updated dependencies [9586408]
- Updated dependencies [b5fcf8e]
- Updated dependencies [c074d71]
- Updated dependencies [7239301]
- Updated dependencies [74b8586]
- Updated dependencies [ff8794e]
- Updated dependencies [5a7c18e]
- Updated dependencies [2ccae0b]
- Updated dependencies [269fc6f]
- Updated dependencies [cb56346]
- Updated dependencies [2df4377]
- Updated dependencies [3b18503]
- Updated dependencies [0dcdd10]
- Updated dependencies [51d243b]
- Updated dependencies [137c169]
- Updated dependencies [e839fa9]
- Updated dependencies [529bc3f]
- Updated dependencies [72b978c]
- Updated dependencies [741ac65]
- Updated dependencies [5680e1e]
- Updated dependencies [b6fdef9]
- Updated dependencies [916837b]
- Updated dependencies [d27382f]
- Updated dependencies [cd70ad8]
- Updated dependencies [8531452]
- Updated dependencies [cbc6683]
- Updated dependencies [d7994d5]
- Updated dependencies [867e75c]
- Updated dependencies [faf45f2]
- Updated dependencies [b8e3246]
- Updated dependencies [80ede86]
- Updated dependencies [db2a6fa]
- Updated dependencies [f285bec]
- Updated dependencies [7545fdf]
- Updated dependencies [6262d13]
- Updated dependencies [ef2973c]
- Updated dependencies [fd10f7a]
- Updated dependencies [52fb2d3]
- Updated dependencies [b895c4f]
- Updated dependencies [4de4d3f]
- Updated dependencies [ad09a98]
- Updated dependencies [98a1ae6]
- Updated dependencies [9dec659]
- Updated dependencies [d22cef5]
- Updated dependencies [11cdf95]
- Updated dependencies [38db35c]
- Updated dependencies [186546c]
- Updated dependencies [2ae8fc9]
- Updated dependencies [63bc2a3]
- Updated dependencies [276bf6c]
- Updated dependencies [cc52b83]
- Updated dependencies [96173a5]
- Updated dependencies [39124bb]
- Updated dependencies [acc6b0b]
- Updated dependencies [73b12c8]
- Updated dependencies [e5d78d1]
- Updated dependencies [7182df5]
- Updated dependencies [a5d1b37]
- Updated dependencies [67bf45d]
- Updated dependencies [f58b1f6]
- Updated dependencies [ad9d451]
- Updated dependencies [aef14d6]
- Updated dependencies [5ee89a8]
- Updated dependencies [b615f7d]
- Updated dependencies [cc34968]
- Updated dependencies [34f740a]
- Updated dependencies [c41b0c7]
- Updated dependencies [874c40b]
- Updated dependencies [0c9db81]
- Updated dependencies [65f96b0]
- Updated dependencies [e4fb577]
- Updated dependencies [7e8d3a2]
- Updated dependencies [b308544]
- Updated dependencies [fbd074e]
- Updated dependencies [e4fc530]
- Updated dependencies [666edaf]
- Updated dependencies [0de4a8f]
- Updated dependencies [a9cdbf8]
- Updated dependencies [1e91561]
- Updated dependencies [bdeecb2]
- Updated dependencies [cb874b5]
- Updated dependencies [f766b2a]
- Updated dependencies [af56482]
- Updated dependencies [4917db1]
- Updated dependencies [78cfd24]
- Updated dependencies [b1122e3]
- Updated dependencies [6cd41cc]
- Updated dependencies [18dffa4]
- Updated dependencies [025c865]
- Updated dependencies [689f1f7]
- Updated dependencies [d7fb30e]
- Updated dependencies [55976d2]
- Updated dependencies [029512d]
- Updated dependencies [405be81]
- Updated dependencies [9e89eb2]
- Updated dependencies [12b5302]
- Updated dependencies [0b94925]
- Updated dependencies [96998ef]
- Updated dependencies [25e3742]
- Updated dependencies [533daf9]
- Updated dependencies [62e63ba]
- Updated dependencies [3436d7f]
- Updated dependencies [96eb265]
- Updated dependencies [70a1385]
- Updated dependencies [7915095]
- Updated dependencies [f73db83]
- Updated dependencies [c2d0c9f]
- Updated dependencies [f59a88c]
- Updated dependencies [16a548e]
- Updated dependencies [bee5bdb]
- Updated dependencies [220957f]
- Updated dependencies [3316a25]
- Updated dependencies [80735d8]
- Updated dependencies [6453c13]
- Updated dependencies [470cef5]
- Updated dependencies [0d28d28]
- Updated dependencies [ea92b07]
- Updated dependencies [ae3eece]
- Updated dependencies [a6863e3]
- Updated dependencies [1104a9f]
- Updated dependencies [9243ec0]
- Updated dependencies [f07284d]
- Updated dependencies [a6d86e5]
- Updated dependencies [3dd7936]
- Updated dependencies [f81379d]
- Updated dependencies [19e6cbb]
- Updated dependencies [fa52c61]
- Updated dependencies [21a40e8]
- Updated dependencies [8f0f38b]
- Updated dependencies [22bac8a]
- Updated dependencies [329560b]
- Updated dependencies [b42e3e5]
- Updated dependencies [c1ed5cd]
- Updated dependencies [540d094]
- Updated dependencies [90e0989]
- Updated dependencies [217ba82]
- Updated dependencies [4c6c8a5]
- Updated dependencies [20ab18e]
- Updated dependencies [1adefcc]
- Updated dependencies [814276c]
- Updated dependencies [e976041]
- Updated dependencies [c1be12d]
- Updated dependencies [260a94a]
- Updated dependencies [2a4b311]
- Updated dependencies [422d968]
- Updated dependencies [881726e]
- Updated dependencies [9c211d5]
- Updated dependencies [a4f14ba]
- Updated dependencies [92eb241]
- Updated dependencies [a7556a5]
- Updated dependencies [ef4d386]
- Updated dependencies [e44f1fa]
- Updated dependencies [b2b1ad5]
- Updated dependencies [f5395de]
- Updated dependencies [c3ac050]
- Updated dependencies [0d17b32]
- Updated dependencies [60b83cd]
- Updated dependencies [736e8d4]
- Updated dependencies [4dcafc9]
- Updated dependencies [06cacc4]
- Updated dependencies [50c1107]
- Updated dependencies [52e6809]
- Updated dependencies [22540ca]
- Updated dependencies [d64cefc]
- Updated dependencies [a5e5896]
- Updated dependencies [115bb40]
- Updated dependencies [f958d24]
- Updated dependencies [8632205]
- Updated dependencies [231bb5f]
- Updated dependencies [9fac670]
- Updated dependencies [f628345]
- Updated dependencies [4a1dee1]
- Updated dependencies [4bfcdab]
- Updated dependencies [78b4c2d]
- Updated dependencies [309c00c]
- Updated dependencies [19e18be]
- Updated dependencies [aa26464]
- Updated dependencies [31155ad]
- Updated dependencies [a64d78e]
- Updated dependencies [9943755]
- Updated dependencies [c91926b]
- Updated dependencies [f6838e2]
- Updated dependencies [d120ecb]
- Updated dependencies [5fabf50]
- Updated dependencies [8e6427a]
- Updated dependencies [9c8d51a]
- Updated dependencies [e5b980d]
- Updated dependencies [cf57b22]
- Updated dependencies [546ded5]
- Updated dependencies [3bbd8d9]
- Updated dependencies [56ce7bf]
- Updated dependencies [4a664ec]
- Updated dependencies [c025a9f]
- Updated dependencies [85cb6e5]
- Updated dependencies [9397a1a]
- Updated dependencies [3be1e5f]
- Updated dependencies [d43ec7e]
- Updated dependencies [967bd1b]
- Updated dependencies [b353095]
- Updated dependencies [a7e313d]
- Updated dependencies [d72cad2]
- Updated dependencies [62dba1d]
- Updated dependencies [370715c]
- Updated dependencies [8d92c9c]
- Updated dependencies [1156ff3]
- Updated dependencies [c3c6eab]
- Updated dependencies [633fb8f]
- Updated dependencies [885ca6f]
- Updated dependencies [8f30776]
- Updated dependencies [76cc702]
- Updated dependencies [d8ddd78]
- Updated dependencies [246b800]
- Updated dependencies [9fc8665]
- Updated dependencies [8e6a225]
- Updated dependencies [d4eab47]
- Updated dependencies [79fdaf4]
- Updated dependencies [d18f149]
- Updated dependencies [d44e75f]
- Updated dependencies [07740e1]
- Updated dependencies [1939b97]
- Updated dependencies [77cd00a]
- Updated dependencies [8628395]
- Updated dependencies [7c8480a]
- Updated dependencies [88c6754]
- Updated dependencies [8db452d]
- Updated dependencies [2c7a43d]
- Updated dependencies [59528ec]
- Updated dependencies [db3f8e4]
- Updated dependencies [bd95b30]
- Updated dependencies [dadf0ce]
- Updated dependencies [98d2ad2]
- Updated dependencies [36d6630]
- Updated dependencies [3893794]
- Updated dependencies [dc35b98]
- Updated dependencies [ff1e121]
- Updated dependencies [e2e5ec4]
- Updated dependencies [6fae1a6]
- Updated dependencies [8c52feb]
- Updated dependencies [aadc9b5]
- Updated dependencies [55cc2b1]
- Updated dependencies [46eb61b]
  - @brink-lang/web@0.15.0

## 0.14.0

### Patch Changes

- Updated dependencies [9481137]
- Updated dependencies [a6e8a6a]
  - @brink-lang/web@0.14.0

## 0.13.0

### Minor Changes

- abe28a2: Host-registered argument handlers for primitive base types (#990). A host can
  now register a widget against a **base type** (`bool` | `int` | `float` |
  `string`), not just a `host.<vendor>.<name>` semantic id — e.g.
  `setHostWidgets([{ type: "bool", … }])` gives every `bool`-typed argument the
  host's own toggle, in the host's own design system, with zero brink-shipped
  opinion on primitives. This runs through the same `matchHostWidget` fallback
  that already resolved semantic types; it is now an intentional, documented,
  tested part of the contract.

  New API surface:

  - `getHostWidget` and `matchHostWidget` are now exported, so a host (or a
    consumer's tests) can introspect widget registration/resolution directly.
  - `ArgumentWidget.editor.surface` gains `"inline"` alongside `"popover"` /
    `"modal"` — mounts the widget's control directly in the argument Form's row
    (where a text field would sit) instead of `buildHostField`'s summary-chip +
    expandable editor. The right shape for a primitive control (a bool toggle,
    a number stepper) that IS the field, not something you open an editor on.
    Only the Form honors it; the in-editor Edit/Fill affordances have no row to
    mount into and keep the popover chrome for the same widget.

  **Behavior change:** the argument Form's per-field control precedence is now
  **color → hostWidget → values → text** (previously values was checked before
  hostWidget). A field that declares both a host widget and `values` now gets
  the widget — before this fix a host widget could never win over a plain
  values dropdown on the same type, which also blocked rich pickers (e.g. an
  icon-grid item browser) for semantic types that also carry `setHostValues`
  labels. A field with `values` and no `hostWidget` is unaffected.

### Patch Changes

- 909bb26: `dialect.ts`'s `ResolvedDialect.compile` no longer unconditionally constructs
  its per-kind `RegExp`s with the `d` (`hasIndices`) flag (#1013). That flag
  needs V8 9.0 / Chromium 90+ — NW.js-hosted embedders on older Chromium (e.g.
  RPG Maker MZ's bundled NW.js is Chromium 88, with no newer official runtime)
  threw `SyntaxError: Invalid flags supplied to RegExp constructor 'd'` at
  construction, black-screening the embedder at boot before a single line was
  ever classified.

  Support is now feature-detected once at module scope. Modern engines keep the
  `d`-flag path unchanged (indices read straight off the match). Older engines
  fall back to a capture-group walk that reconstructs the same per-named-group
  `[start, end)` spans by locating each group's captured text within its
  nearest enclosing named group's span — correctly handling nested groups (e.g.
  `parenthetical`'s `content` wrapping `content_inner`) — with no loss of
  `DialectMatch` fidelity (`kind`/`attrs`/`hiddenSpans`/`contentSpan` are
  byte-identical to the `d`-flag path on every input the at-cue conformance
  corpus and generalization suite cover).

- Updated dependencies [17ad933]
- Updated dependencies [f53c6c7]
- Updated dependencies [7e8aa7f]
- Updated dependencies [b9a86e2]
  - @brink-lang/web@0.13.0

## 0.12.0

### Patch Changes

- Updated dependencies [6cb663a]
  - @brink-lang/web@0.12.0

## 0.11.1

### Patch Changes

- Updated dependencies [c246a4a]
- Updated dependencies [ae66340]
- Updated dependencies [7baa01f]
- Updated dependencies [aa43bb6]
- Updated dependencies [edf92bc]
- Updated dependencies [d350551]
- Updated dependencies [3c1e1e1]
- Updated dependencies [c03a73a]
- Updated dependencies [83717d3]
- Updated dependencies [302c6a2]
- Updated dependencies [4a08940]
- Updated dependencies [b86fee8]
- Updated dependencies [1e1be68]
- Updated dependencies [c36b8c4]
- Updated dependencies [71dd2fc]
- Updated dependencies [213a7f5]
- Updated dependencies [730c947]
- Updated dependencies [a0d9ee2]
- Updated dependencies [7ac0a5d]
- Updated dependencies [1198586]
- Updated dependencies [058f410]
- Updated dependencies [7500e27]
- Updated dependencies [bcb5cd3]
- Updated dependencies [c62687c]
- Updated dependencies [8870113]
- Updated dependencies [e16e8f8]
- Updated dependencies [820f6c5]
- Updated dependencies [45eb96b]
- Updated dependencies [e8cb050]
- Updated dependencies
- Updated dependencies [fe0c16d]
- Updated dependencies [6266cbf]
- Updated dependencies [9e9f07a]
- Updated dependencies [878be79]
- Updated dependencies [c66409b]
- Updated dependencies [86c4bee]
- Updated dependencies [fdf94f6]
- Updated dependencies [9d559a3]
- Updated dependencies [cc1d11e]
- Updated dependencies [62cb759]
- Updated dependencies [a350dcf]
- Updated dependencies [3ad1bc5]
- Updated dependencies [2b7dd5a]
  - @brink-lang/web@0.11.1

## 0.9.3

### Patch Changes

- 96a4513: Rename-collision analysis (#722) no longer runs synchronously on the paint
  path. Root-caused from PR #714's RCA of the #696 e2e flake: the inline
  rename widget's breakage/collision query (`renameSymbolAt`, a wasm call)
  used to run inline in the debounce/Enter handler and could block paint for
  several seconds under load.

  The debounce settle (and an Enter with no cached result) now flips the "⚠
  breaks N" badge into a disabled, `aria-busy` **pending** state (`⋯`,
  `.brink-inline-rename-badge--pending`) synchronously — so a paint can land
  before the heavy call runs — then defers the actual query to the next idle
  slot (`requestIdleCallback`, falling back to a macrotask where unavailable)
  via a small `scheduleIdleWork`/`cancelIdleWork` helper. Apply/force stays
  disabled until the deferred query resolves; Enter never forces a synchronous
  call. `query`'s signature is unchanged (still plain and synchronous) — only
  its _scheduling_ moved, so existing synchronous callers/tests keep working
  unmodified. Same shared `InlineNameInput` also covers extract-to-knot/
  function (#315 H), so both prompts get the same off-paint-path behavior.

  No web-worker architecture (out of scope for #722) — a query that is itself
  slow still blocks once it starts; this mitigates by not starting it inside
  the same frame as the triggering keystroke.

- Updated dependencies [8a3635d]
- Updated dependencies [34951ec]
- Updated dependencies [81ddfa7]
- Updated dependencies [9c58d6e]
- Updated dependencies [f68c094]
- Updated dependencies [b9ad39f]
- Updated dependencies [b7b7eb0]
- Updated dependencies [d29671d]
- Updated dependencies [ca45425]
- Updated dependencies [abc369a]
- Updated dependencies [30e09f9]
- Updated dependencies [2541c08]
- Updated dependencies [5b07740]
- Updated dependencies [d02c4e2]
- Updated dependencies [20d2bfa]
- Updated dependencies [d38fa08]
- Updated dependencies [9bef954]
- Updated dependencies [1e71455]
- Updated dependencies [c9475df]
  - @brink-lang/web@0.11.0

## 0.9.2

### Patch Changes

- 601e999: HIR overlay now also refreshes when a view MOUNTS after a compile was
  already delivered (#518, follow-up to #494/#502). The 0.9.1 fix refreshed
  mounted views on `deliverCompile`, but for a slot without a view the refresh
  was dropped, not queued — so in the mount-after-initial-compile order (an
  external embedder's passive-load sequence: `ProjectSession.initialize()` →
  `triggerCompile()` → the framework commits the editor mount afterwards) the
  overlay showed whatever its `StateField` last held and nothing ever
  repainted it: a passive load never compiles again, and a remount reuses the
  cached `EditorState`, so the field's `create()` never re-runs and a value
  cached blank at unmount persisted until the first keystroke.

  `DocumentSessions.mountView` now self-serves the missed refresh: when a
  compile has already been delivered (`lastCompileDelivered`), it dispatches
  `refreshHirOverlayEffect` to the freshly mounted view — after the slot's
  wasm handle is (re)opened, so the projection read is live at mount time.
  The overlay's refresh trigger set is now
  {compile-deliver} ∪ {view-mount-after-a-deliver}, covering both mount
  orders and cached-state remounts.

  Also documents (hir-overlay.ts, editor-consumer-guide) that a host-dispatched
  `refreshHirOverlayEffect` is matched by object identity, so it must come from
  the same module instance of `@brink-lang/editor` that built the view's
  extensions — a bundler-duplicated copy produces an effect the field silently
  ignores, which can make host-side refresh workarounds appear to "read empty".

- 8333685: Fixed narrative-run folding in screenplay mode (#417). When a narrative run
  IS a choice's body (a character cue + dialogue directly under `* [Talk]`),
  the fold now anchors on the choice line itself and hides the whole body
  beneath it, instead of anchoring one line down on the cue. The collapsed
  pill no longer duplicates the anchor line's visible text ahead of the chip —
  the fold now hides the whole anchor line and the pill IS the line, matching
  the existing decl-fold placeholder shape. The pill's snippet also strips the
  dialect's cue sigils and shows the first CONTENT line (or the cue's bare
  name when the run has no content line), rather than raw text like
  `@Jackie:<>`.
- Updated dependencies [e2acdbb]
- Updated dependencies [6ed8a8d]
- Updated dependencies [eb06ccc]
- Updated dependencies [1154eb4]
- Updated dependencies [0f6ae50]
- Updated dependencies [e96d2a1]
- Updated dependencies [bd69ac6]
- Updated dependencies [f40c345]
- Updated dependencies [f25362a]
- Updated dependencies [ebce613]
- Updated dependencies [3c5808f]
- Updated dependencies [eaff136]
- Updated dependencies [ba69a35]
- Updated dependencies [124bb9e]
- Updated dependencies [75b8a3b]
- Updated dependencies [350b663]
- Updated dependencies [9e1257d]
- Updated dependencies [9213d77]
- Updated dependencies [f835cfd]
- Updated dependencies [b8392a2]
- Updated dependencies [6089ed6]
- Updated dependencies [1c389ec]
- Updated dependencies [81f0055]
- Updated dependencies [6e007d3]
- Updated dependencies [0308aec]
  - @brink-lang/web@0.10.1

## 0.9.1

### Patch Changes

- 66457c1: Fixed a silent dialect-classification drop in `computeLineInfos` (#426):
  when a mounted view's wasm document handle was present but not yet synced
  (or a host's `line_contexts_doc` returned `[]`, as some test/mock sessions
  do), every line was classified via the bare regex fallback and the TS
  dialect interpreter (`applyDialectFallback`) was never run — character cues,
  parentheticals, and chained dialogue lines silently rendered as plain
  narrative with no diagnostic. The same TS dialect fallback the no-handle
  path already ran is now also run over the regex-classified tail whenever the
  handle yields fewer line contexts than the document has lines, so dialect
  classification survives that path.
- 8f20d1e: HIR overlay now refreshes when the initial compile completes (#494). The
  overlay's projection StateField seeded at view creation — before the first
  async compile/analysis finished — and only recomputed on doc-changing
  transactions, so a passively loaded file rendered no `brink-hir-*` marks or
  rails until the first edit. `DocumentSessions` now dispatches a redecorate
  effect to every mounted view whenever a compile result is delivered, and the
  new `refreshHirOverlay(view)` / `refreshHirOverlayEffect` exports let hosts
  with custom wiring re-read the projection from their own compile-complete
  signal (mirrors `refreshGutterMarkers`).

## 0.9.0

### Minor Changes

- 73e2746: Line-classification fixes (#478) — deliberate behavior changes to the
  `line_contexts` contract and `LineInfo`:

  - A choice line with an inline divert (`* [Go] -> hub`) now classifies as
    `choice` (was `divert`), so Tab/Enter smart-editing transitions work on
    it again.
  - Every gather-label line — continuation labels, `LabeledBlock` labels,
    top-level labeled gathers — uniformly classifies as `gather` with
    `gather_continuation` weave at its sigil depth. Previously a labeled
    block with an inline divert showed `divert` while the visually identical
    continuation form showed `gather`.
  - Choices inside conditional/sequence branches report their sigil depth
    (was 0), so depth-dependent transitions and gutter depth markers work
    inside arms.
  - Blank lines inside a choice body inherit the body weave (element stays
    `blank`); the editor maps them to `ChoiceBody` so Tab works anywhere in
    the body — replacing the old single-shape TS post-pass, and covering
    deeper blank runs it missed.

- 36bf266: Machinery/narrative fold runs are now opt-in (#479). `foldingRanges` /
  `folding_ranges_doc` return structural folds only unless the host enables
  run computation via the new session-level `setFoldRunsEnabled(true)`
  (mirrors `setDialect`; also on `DocumentHandle`), and the editor's default
  active fold kinds are `structural` only — hosts implementing prose/logic
  view modes activate `machinery`/`narrative` with `setActiveFoldKinds` and
  collapse with `foldAllOfKind`. Runs are additionally bounded by weave
  containers (choice branches / gather continuations), so a run fold never
  crosses weave structure; conditional scaffold + arms still fold as one
  pure-routing region, and inline `{a|b}` alternatives don't fragment
  narrative runs.
- 4b5b3ab: HIR structural overlay in the editor (#454 phases 3–5): a queryable projection
  StateField renders `brink-hir-*` inline marks with `data-*` identity, per-line
  rail attributes plus a concentric rails gutter (knot/stitch/choice/gather/
  branch), and identity-keyed occurrence highlighting. New `getHirProjection`
  option on `BrinkStudioOptions`; `hirSpansAt`/`hirIdentityAt` query helpers.
  Default skin styles only unresolved refs, occurrences, and rails — hir marks
  stay visually inert for host theming.
- 1bca37c: LineInfo on one shared projection (#480). The HIR projection is now
  computed once per edit and cached on the session — `getLineContextsDoc`,
  `getFoldingRangesDoc`, and `getHirSpansDoc` all share it instead of each
  re-projecting. `LineContext` gains two additive fields the editor now
  consumes instead of deriving: `option_path` (option identity from real HIR
  nesting — the TS weave re-walk only serves the pre-wasm regex fallback)
  and `standalone` (structural divert-vs-tunnel/thread fact — no more text
  sniffing in the editor or fold-run natures). Span kinds `tunnel_stmt` and
  `thread_stmt` split out of `divert_stmt`, which now means a simple
  `-> target` statement only.
  Also fixes `has_tags`: it is now true for **any** line carrying an
  author-written tag — tagged choice lines (`* Choice # tag`), tags inside
  inline conditional/sequence branches, and standalone `#` lines — where the
  legacy walk under-reported (decision 2026-07-10; verified against the C#
  reference, whose runtime surfaces choice-line tags).

### Patch Changes

- Updated dependencies [73e2746]
- Updated dependencies [36bf266]
- Updated dependencies [973858f]
- Updated dependencies [54c37df]
- Updated dependencies [1bca37c]
- Updated dependencies [6289b0e]
  - @brink-lang/web@0.10.0

## 0.8.1

### Patch Changes

- 33f49a7: Fix #403: the narrative fold pill's cast summary now routes through `detectCast` (the #366/#399 public dialect extractor) instead of reading the `speaker` attr raw off `LineInfo.dialect.attrs`. A custom dialect whose chain carries a differently-named attr (e.g. `narrator` instead of `speaker`) now surfaces correctly in the pill; the default at-cue dialect's pill output is unchanged (still shows the carried `speaker` value).
- 81578e6: Fix #405: `preparePlaceholder` now breaks an exact-span tie between a structural fold and a machinery/narrative fold deliberately (structural wins), instead of relying on the accidental push order of `getFoldingRanges()`. No visible behavior change for the ordering hosts already ship (structural is pushed first in `folding_ranges_impl`), but the precedence is now pinned and covered by a test that constructs the tie with the ranges pushed in the opposite order.
- d9e6ad0: Fix #406: reconcile the at-cue preset's `parenthetical` shape's `content_group` (which spans the parens-inclusive `"(text)"`, needed so `content_span`/markup geometry keeps the parens as visible content) with a convert/strip round-trip's need for the bare text between the parens. Before this fix, a dialect declaring a `convert` transition row targeting `parenthetical` from a bare-content source kind rendered the target template (`"${content}<>"`) with the bare extracted text, dropping the opening paren entirely (`"radio<>"` instead of `"(radio)<>"`) — latent since the at-cue preset itself ships `transitions: []`, but it would have bitten the first dialect declaring such a row. Adds `PatternShape.template_group` (optional, additive — defaults to `content_group`, byte-identical for every dialect that doesn't set it): the group whose captured value fills `template`'s placeholder for convert/strip round-trips. No visible change for the default dialect's classification geometry or `data-*` attributes.
- d346261: Fix a false-positive in the conditional-scaffold classification pass added
  by #413: ordinary narrative containing inline logic that happens to
  start or end with a brace (a standalone inline conditional used as
  narrative content, e.g. `{visited: You were here before.}`, or narrative
  ending in a value interpolation, e.g. `You have {gold}`) was incorrectly
  swept into `Logic` classification (`brink-logic`) merely because the line
  started with `{` or ended with `}`. Only a conditional/sequence block's
  own genuine opening/closing brace (bare `{`/`}`, or `{` followed by a
  switch expression ending in `:`) is scaffold now — inline logic embedded
  in narrative keeps its narrative/dialogue classification.
- f35b20c: Fix a headless-contract leak (#414, follow-up to #363): the line-decoration
  pass stamped two inline `style` attributes regardless of `theme: false` —
  `padding-left` for weave-depth indent on choices/gathers, and
  `text-align: right` on standalone diverts — which beat host stylesheets and
  left headless embedders unable to restyle them.

  Both are now taxonomy instead:

  - Weave depth rides as `data-depth="N"` (choices/gathers at depth > 1).
  - Standalone diverts carry the `brink-divert-standalone` class.

  `brinkTheme` ships the previous look (indent scaled by depth, right-aligned
  standalone diverts) via CSS attribute/class selectors, so `brink-studio`
  renders unchanged. Headless hosts (`theme: false`) restyle
  `[data-depth="N"]` / `.brink-divert-standalone` directly — the
  line-decoration pass never emits a `style` attribute.

- d346261: Fix two screenplay-mode classification gaps (#413): a `~`-sigil logic line
  immediately after a chained dialogue line was swallowed into the
  cue→dialogue chain (rendered `brink-dialogue` instead of `brink-logic`),
  and lines in/around conditional blocks (`{`, `- cond:`, `- else:`, `}`, and
  cue/dialogue lines inside conditional arms) got no classification at all.

  Sigil classification now always wins over chain continuation. Conditional
  scaffold lines classify as logic; cue/dialogue lines written inside a
  conditional or sequence arm classify normally (Character/Parenthetical/
  Dialogue) and participate in the dialogue chain, matching top-level
  narrative. Choice-body narrative is unaffected — it still classifies but
  never chains, per the existing spec-mandated split.

  Emitted classes for lines that already classified correctly are
  byte-identical; only the previously-broken lines gain classes.

- Updated dependencies [5075db7]
- Updated dependencies [cbc27aa]
  - @brink-lang/web@0.9.0

## 0.8.0

### Minor Changes

- 3cf1062: Fold kinds (#365): `FoldRange` now carries a `kind: "structural" | "machinery" | "narrative"`.

  - **`structural`** — everything the folding pass emitted before #365 (knot/stitch declarations, doc comments, conditionals, sequences, choice sets, the INCLUDE-block fold). User-invoked in every mode; never auto-collapsed.
  - **`machinery`** — a maximal run of `>= 2` consecutive machinery-natured lines (logic `~`, VAR/CONST/LIST decls, standalone diverts, conditional/sequence scaffold lines). Run-based over the per-line classification (base, or a registered dialect's declared `nature`, #368) — never HIR-block-based, so a narrative-bearing conditional's scaffold lines don't drag its prose branches into a machinery fold.
  - **`narrative`** — the symmetric run of `>= 2` consecutive narrative-natured lines (plain prose, or dialect kinds like `character`/`parenthetical`/`dialogue`).

  Editor-side (`@brink-lang/editor`):

  - `foldingExtension` takes a live-reconfigurable **active-kinds set** (default: all three); `setActiveFoldKinds(view, kinds)` reconfigures a mounted view.
  - New exported commands `foldAllOfKind(kind)` / `unfoldAllOfKind(kind)` — bulk fold/unfold every current range of one kind. Mode auto-collapse is always **host-invoked** (call these on your own mode-entry hook); the extension itself never forces a collapse.
  - Machinery/narrative folds render a JetBrains-style summary pill instead of the generic `…` placeholder: `brink-fold-pill` + `brink-fold-pill-machinery`/`brink-fold-pill-narrative` + `brink-fold-pill-icon`/`brink-fold-pill-summary`/`brink-fold-pill-count` child spans — class-addressable, zero inline styles. The machinery pill summarizes salient calls/assignments/divert targets (capped at 2, "+N more"); the narrative pill shows a first-line snippet, cast (via the registered dialect's carried `speaker` attribute — not a re-hardcoded `characterName()`), and line count.
  - The existing declaration fold placeholder (`.brink-fold-decl`) now carries `data-decl-kind="knot" | "stitch" | "function"` plus a `.brink-fold-decl-icon` slot span.

  `brink_ir::ElementNature` (narrative/machinery/structural) and `ResolvedDialect::nature_of` are new in the Rust dialect schema, consumed by `brink-ide::folding::machinery_and_narrative_folds` — never re-hardcoding a kind list in Rust or TS.

- 58d93ee: Compiler lines table + public `DialectParser` (#366): a host can now work out the cast (and similar per-line analyses) from the compiler's own line table instead of duplicating the `@Name:<>` convention.

  - **`@brink-lang/web`**: `StoryRunnerHandle.linesTable()` returns the compiled program's line table — one entry per scope (root/knot/stitch), project-wide (`INCLUDE`s already resolved by the compile), each line carrying its text (plain or a slot/select template) and, when known, its source span (`file` + byte range). Reuses the exact shape the `export-xliff` CLI path already produces (`brink_intl::export_lines`) rather than inventing a second representation. Static for the loaded program — no running `Story` required.
  - **`@brink-lang/editor`**: `DialectParser` (pure TS, no CM6/wasm dependency) — `parseSource(text)` classifies plain `.ink`-style source line-by-line against a `DialogueDialect` (mirrors `element-type.ts`'s classify + chain passes); `parseEmitted(text)` walks _runtime-emitted_ text (the post-glue output of `continue_line()`) into composite segments per the pinned iteration protocol: a cue + parenthetical + trailing text emitting as ONE line is the normal case, and a non-reserved-prefix shape (e.g. a parenthetical) never opens a composite line — it only peels as a continuation after a reserved-prefix (cue) segment.
  - **`detectCast(lines, dialect)`** ships as the #366 answer to cast detection: it walks `parseSource` output and collects the distinct values of whichever attr a dialect's chain rules `carry` forward (dialect-agnostic — not hardcoded to `speaker`). `characterName()` is NOT exported publicly (stays `screenplay.ts`-internal, per the dialect-spec ruling).

  First consumer: celeris cast detection feeding its speaker-color settings surface. The same lines-table exposure serves future analyses (per-speaker word counts, the #362 line-fit metrics epic).

- 6785663: Dialogue dialect editor integration (#368): the screenplay behavior (`@Name:<>` character cues, `(beat)<>` parentheticals, the dialogue chain) is now driven by a `DialogueDialect` — a versioned, pure-JSON schema — instead of hardcoded regexes.

  - **`brinkStudio({ dialect })`** (default `AT_CUE_DIALECT`, byte-identical to the old hardcoded behavior). `dialect: null` tears down the screenplay layer — classification, decorations, dialect transition rows, dialect keybinding behaviors — for true headless composition (pair with `theme: false`, #363); the structural weave keymap (Choice/Gather/Narrative Tab/Enter transitions) stays active, per the spec's structural-rows-stay-interpreter-owned rule. A custom `DialogueDialect` object drives classification/decorations/transitions/conversions with zero editor code changes.
  - **`@brink-lang/web`**: `EditorSessionHandle` gains `setDialect(dialect)` / `clearDialect()` (wrapping the wasm `set_dialect`/`clear_dialect` seam from #386), and the `DialogueDialect` schema types + the `LineContext.dialect` facet are published from the type surface.
  - **`setDialect(view, dialect)`** live-reconfigures an already-mounted editor: swaps the screenplay compartment, forces reclassification, and re-runs the wasm `set_dialect`/`clear_dialect` when a document handle is present.
  - **`extendDialect(base, overrides)`** adds a kind (or overrides chain/transitions/templates) without forking a preset.
  - Classification is authoritative in Rust (`brink_ir::dialect` + `line_contexts_with_dialect`, landed in #386) when a wasm document handle is present. Without one, the editor falls back to a thin TS interpreter over the identical JSON (`ResolvedDialect`), pinned against the same conformance corpus (`tests/dialect_fixtures/at_cue.json`) as the Rust side so both paths agree on every case.
  - Screenplay geometry (`screenplay.ts`'s hidden decorations, atomic ranges, edit guard, cursor clamps) is now derived from the resolved dialect's hidden-group match indices — computed once at classification time and cached, never re-matched in per-keystroke hot paths. The `CHAR_SUFFIX_LEN`/`GLUE_LEN` constants and the public `characterName()` export are gone; the geometry is dialect-derived and internal.
  - The Tab/Enter/Shift-Tab transition table and name-surgery keybindings now consult a dialect's declared overlay rows before the built-in structural weave table (inert for the default preset, which ships no overlay rows).

  ### BREAKING CHANGE: `ElementType` enum → open string union (0.x hard cut, ruled 2026-07-05)

  `ElementType` used to be a numeric TS `enum`. It is now a `const` object of kebab-case kind strings mirroring the existing `brink-<kind>` CSS class scheme — `ElementType.Character`-style call sites migrate mechanically (the values still compare correctly), but the type is now `string`, and two PascalCase leaks are now kebab-case:

  - `@brink-lang/studio`'s published `StudioApi`: `StudioPublicState.element.type` was `"KnotHeader"`, `"NarrativeText"`, `"Choice"`, … — now `"knot-header"`, `"narrative"`, `"choice"`, ….
  - `@brink/studio-store`'s duplicate `ElementType` enum is deleted; it now re-exports the real one from `@brink-lang/editor` (still available as `ElementTypeEnum`).

  Full PascalCase→kebab mapping table in `docs/editor-consumer-guide.md`. No compat shim — both packages are pre-1.0.

- 047db46: Publish the tier-1 boundary helpers (#369): re-export `CompileResult`/`Diagnostic` from `@brink-lang/web` for module identity, and export the canonical `sortDiagnostics` (positional: file → offset → errors-first; presentation order is a host choice layered on top) and `lineColAt` (offset → 1-based line:col).
- f06d4ff: Generalize the last two at-cue-hardcoded sites so custom `DialogueDialect`s are functionally complete (#395, a follow-up from #368):

  - **Dialect `convert` and `strip` transition rows** now extract a line's content via the resolved dialect's OWN declared shapes (`ResolvedDialect.convertibleShapes()`), not the hardcoded `@name:<>`/`(text)<>` regexes. A custom dialect's non-at-cue wrapping kinds (e.g. `<<name>>`) now convert and strip correctly via a `transitions` row's `convert`/`strip` actions.
  - **`contentRegions`** (the inline-markup content-region scoping core) now accepts an optional third `geometry` argument (a line's cached `LineInfo.dialect`); when given, a Character/Parenthetical-shaped line's content bounds derive from `geometry.contentSpan` instead of the fixed at-cue affix-length constants, so a dialect overriding those kinds with different affix widths scopes markup correctly.
  - `extractLineContent` gains an optional `shapes: ConvertibleShape[]` parameter (tried before the built-in at-cue shapes); `ConvertibleShape` is now exported.

  Both changes are additive — omitting the new optional parameters reproduces the exact pre-#395 behavior for the default (at-cue) preset.

- ed2446b: Headless-ready editor (#363): the `brinkTheme` skin is now opt-out — pass `theme: false` to `brinkStudio(...)` (or `DocumentSessions`'s new options bag) for a fully headless editor, or pass your own CM `Extension` to substitute it; the default is unchanged and brink-studio now opts into `brinkTheme` explicitly. All presentational inline styles on editor-owned popups and widgets (code-actions menu, inline element picker, widget popover, argument-form anchor, inlay hints, color swatch + picker) moved onto classes; dynamic values (popup coordinates, swatch colors) ride on CSS custom properties consumed by a new always-on, zero-specificity (`:where(...)`) structural stylesheet (`ensureStructuralStyles`, exported), so hosts can restyle the full class taxonomy directly. The taxonomy — element line classes (an open `brink-<kind>` scheme), structural decoration classes, floating-surface classes, and custom properties — is now documented as a semi-stable contract in docs/editor-consumer-guide.md.
- bfd7e50: Host gutter-marker contribution API (#343): `BrinkStudioOptions.getGutterMarkers(source, fromLine, toLine) => HostGutterMarker[]` and `onGutterMarkerClick` render host-supplied per-line markers (breakpoints, annotations, run/flag icons) in a dedicated gutter slotted after the built-in play-from-here gutter. Purely additive — absent callback changes nothing. Deterministic ordering (by line, host array order within a line), per-marker + shared click dispatch, recompute on doc changes, and an exported `refreshGutterMarkers(view)` / `refreshGutterMarkersEffect` for external marker-set changes. Also exported standalone as `hostGutterExtension`.
- 4ad5d81: Extensible inline-markup rules (#367): `inlineMarkup(rules)` lets hosts register inline-markup shapes (single `pattern`, or `open`/`close` pair with an optional `contentClass`) that decorate as `brink-markup-<name>` marks with `data-*` attributes from named capture groups. Matching is content-region scoped — rules run only within the narrative content text of classified lines and never over ink syntax (glue `<>`, threads `<-`, divert arrows, choice brackets, sigil prefixes, hidden screenplay sigils). Ships zero rules by default; the RMMZ-style angle-tag rule is exported as the optional `rmmzAngleTagRule` preset. Styling is entirely host-side (classes only, per #363).
- fd69c0a: Emit option identity on choice/body lines (#364): every `Choice` line and its `ChoiceBody` lines now carry `data-option-path` (the full lineage of zero-based option indices through the weave, e.g. `"0.2.1"` — nested weaves first-class) and `data-option` (the convenience innermost index) as CM6 line attributes alongside the existing element class. Gather lines close their level's groups, so the next option at that depth starts a new group at index 0; knot/stitch headers reset the weave. Hosts can render per-branch rails (e.g. colored `border-left` on body lines) from these attributes without re-deriving the weave. Also exports the pure `assignOptionPaths` post-pass and adds `optionPath` to `LineInfo`.
- dfdc1e2: DocumentSessions: per-view cursor+scroll save/restore seam (#347). New `viewState(docKey, groupId?)` reads `{ anchor, head, scrollTop }` from the live view when mounted or from the cached slot (EditorState selection + unmount scroll snapshot) for backgrounded tabs, so hosts can persist every open tab, not just the focused one. New `restoreViewState(docKey, state)` re-applies a saved snapshot on the next mount (or immediately when mounted) via the pending-reveal mechanism — full selection + pixel scroll, no focus steal. Scroll now also survives in-session background/remount cycles.

### Patch Changes

- fabd5a5: Chromium 88 (RMMZ/NW.js) compatibility: remove every `color-mix()` from the editor and studio themes — Chromium 88 has no `color-mix()` (Chrome 111+), so those declarations were dropped wholesale, most visibly leaving text selection with no fill.

  - Behind-text highlight layers (`.cm-selectionBackground`) now use a solid `var(--bs-accent)` fill plus layer `opacity`, which composites identically and works on any host that defines the base tokens.
  - The active line uses a new optional theme token `--bs-active-line-bg`, falling back to the opaque `var(--bs-surface-bg)` for hosts that define only base tokens.
  - All other alpha-tinted highlights (search/selection matches, bracket matching, binder/search/graph chrome) are written as `rgb(var(--bs-X-rgb) / N%)` over new per-theme sRGB triplet tokens (`--bs-accent-rgb`, `--bs-error-rgb`, …) defined by the built-in Mocha/Latte themes.
  - Opaque two-color mixes (story-graph node borders/fills, conflict banner) are precomputed per theme as `--bs-graph-*` / `--bs-conflict-banner-bg` tokens.

  Visual output on modern Chromium is unchanged; hosts embedding `@brink-lang/editor` with a custom token set get correct selection/active-line out of the box and can define the new tokens for the tinted variants.

- Updated dependencies [3cf1062]
- Updated dependencies [58d93ee]
- Updated dependencies [6785663]
- Updated dependencies [f72f181]
- Updated dependencies [9d1dd69]
- Updated dependencies [1f91422]
- Updated dependencies [a11b115]
  - @brink-lang/web@0.8.0

## 0.7.0

### Minor Changes

- 8be15da: Editor enhancement sweep (#311): inline rename with a live "⚠ breaks N" badge and inline breakage report; external-conflict 2-way merge view (with the dirty-buffer no-clobber data-loss fix); editor-owned editable search results buffer; INCLUDE-block fold; auto-import "from <file>" completion affordance; in-view find panel; code-actions apply with extract-to-knot/function. First published release of `@brink-lang/editor` (promoted from `@brink/ink-editor`). See `docs/editor-consumer-guide.md`.

### Patch Changes

- Updated dependencies [8be15da]
  - @brink-lang/web@0.7.0
