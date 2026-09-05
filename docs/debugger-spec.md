# Debugger spec — v1 `DebugInfo` encoding + the ruled contract

Status: **RULED contract recorded, v1 wire encoding DESIGNED** (this document,
issue #3179, "D1" of epic #452). Nothing here is scheduled to build yet on
its own — D2–D9 (see [epic #452](https://github.com/Syynth/brink/issues/452))
implement against it. See `docs/decision-log.md` ("Debugger epic (#452): v1
`DebugInfo` contract + D1 design round", 2026-08-28) for the ruling record.

This document has two kinds of content, marked throughout:

- **RULED (maintainer, 2026-08-27/28)** — already decided on issue #3179;
  recorded here, not re-litigated.
- **DECIDED HERE** — the v1 wire-encoding design this ticket exists to
  produce, working inside the ruled constraints above. Reasoning is given so
  a later reviewer can tell a design choice from a maintainer ruling.

Two sub-questions below are explicitly **NOT decided here** because they
need a maintainer call or are already scoped to a later ticket — see
§8 ("Open questions this document does not resolve").

## 0. Scope and program-wide constraint

Scope is **full GDB-style debugging** — breakpoints, step in/over/out, call
stack, variable inspection — with **brink-desktop as the first consumer**
(RULED, maintainer 2026-08-27).

**Both source surfaces must be debuggable** (RULED, maintainer 2026-08-28):
`.ink` (the compatibility surface, `brink-syntax`) and `.brink` (the native
surface, `brink-syntax-native`) both need working breakpoints, stepping,
call-stack, and variable inspection. This was already the working
assumption for D5 (#3183, delivered — its LIR-provenance tests round-trip
both frontends) but had not been written down as a constraint on D1/D6–D9
until now. It is stated once, here, rather than repeated per ticket:

- Mostly free: `.ink` and `.brink` converge at HIR (`CLAUDE.md` "What we're
  building"), so everything below the convergence point — LIR, codegen, the
  `DebugInfo` section, runtime position exposure, breakpoints, stepping — is
  shared by construction. A knot compiled from either surface produces the
  same bytecode shape and the same kind of debug entries.
- It is **not** free at the file-table/`KindToken` layer (§2.3) — that is
  where per-surface information has to be recorded explicitly, or resolution
  silently breaks. See §2.3 for why and how.
- It forces D8 (#3186) to define frame semantics that cover **both**
  vocabularies — ink's tunnels/threads and the condition-park (`until` on
  the native code ground, `~ await`/`~ while await` on the ink surface —
  both spellings lower to the same `AwaitStmt` HIR node,
  `docs/flow-suspension-spec.md` §3) — not just one (§4).
- It forces D9 (#3187) to build the breakpoint gutter against `.ink` files
  too, not only the native studio fixture.
- **Verified by D9 (#3187), not just assumed**: the studio editor DOES give
  `.ink` files the same HIR-overlay `def_id`-carrying treatment as `.brink`
  — `brink-db`'s `projection_query` dispatches on `file_language` and both
  surfaces produce container spans with a `def_id`, proven end to end
  through `EditorSession::hir_spans_doc` for both surfaces in the same test
  (`crates/brink-web/src/editor/spans.rs::ink_files_get_def_id_carrying_hir_spans_like_native_files_do`).
  This document's own research pass had scoped to Rust crates and `docs/`
  and left this unread, hence the earlier "unverified, flagged for D9"
  wording — see §8 item 2 for the record of the check.

## 1. The four carrier/policy rulings (record only)

These were ruled on issue #3179 (maintainer, 2026-08-27) and are recorded
here verbatim in substance, not re-argued.

### 1.1 Carrier: in-file section, not a sidecar file

Debug info is a strippable `SectionKind::DebugInfo` inside `StoryData`, tag
**`0x11`**. Verified on `origin/main`: `SectionKind` tops out at
`FrameShapes = 0x10` (`crates/internal/brink-format/src/inkb/mod.rs:308`);
`0x11` is unclaimed (a test at `inkb/mod.rs` pins it as currently
*rejected* — `from_u8_rejects_unclaimed_section_tag` — and D6 must flip
that pin, not delete it). `VAL_RANGE = 0x11` at `inkb/mod.rs:150` is a
value-tag constant in the *value-tag* byte namespace, a different space
from `SectionKind` — not a conflict.

Confirms Q-R1 (2026-07-19, `docs/sourcemap-epic-evaluation.md`). Rationale:
the 2026-07-27 format durability doctrine made unknown sections
skippable-by-length, so an in-file section costs every other reader
nothing; one artifact removes the cross-file staleness problem a sidecar
would have; and `StoryData.source_checksum` (`brink-format/src/story.rs:92`,
CRC-32 from the `.inkb` header) already exists as the identity gate for "is
this debug info still valid for the bytecode I'm running" — no new
staleness mechanism needed.

### 1.2 Ship policy: dev/studio compiles only

Studio compiles and an explicit `brink compile` debug flag emit the
section; release export omits it.

**The CLI flag spelling is `--debug-info`** (settled 2026-08-28 with #3248,
having been left open here as "a D6/D9 implementation detail"). It is a
mount-time `OptionOverrides { debug_info }` override on `brink compile`
only, with no `brink.toml` key — matching the ruling below that
debuggability is a per-invocation choice, never a project property. Two
consequences that are part of the spelling, not incidental to it:

* **`brink debug` implies it and does not accept it.** The subcommand
  recompiles a `.ink`/`.brink` entry with `debug_info: true` unconditionally
  (`brink-cli`'s `load_program_with_debug_info`), because a debugger without
  the section cannot bind a breakpoint or tell when `step` has crossed a
  line. Making the flag optional there would only offer the user a way to
  ask for a debugger that does not work.
* **A prebuilt `.inkb`/`.inkt` is taken as-is.** Whether it carries the
  section was decided when it was built, and `brink debug` will not silently
  recompile a binary artifact behind the user's back. Debugging one built
  without `--debug-info` therefore degrades honestly rather than failing:
  breakpoints refuse to bind, and stepping reports no source position.

**RULED 2026-08-28 (#3229) — "studio compiles" means a PER-SESSION flag.**
The editor session owns a `emit_debug_info` toggle
(`IdeSession::set_emit_debug_info`, reaching the studio as
`EditorSession::set_debug_info_enabled` / `@brink-lang/web`'s
`setDebugInfoEnabled`). A host turns it **on for the session it is about to
debug and off when that session ends**; it is not a project property and
not on by default.

Two alternatives were considered and rejected. *Always-on for editor
sessions* is simplest but pays the section's size and time cost on every
keystroke of ordinary authoring, and re-baselines the editor acceptance
gate. *A `brink.toml` key* makes debuggability a property of the project
and would leave the debugger off for every project that predates it.

Three consequences the implementation depends on:

1. **It is not an analysis input.** Diagnostics are byte-identical either
   way. `debug_info_policy_query`'s narrow salsa projection gives the flag
   a cheap cutoff, so toggling re-runs `story_data_query` (codegen) and
   leaves every diagnostic query backdated. That is what makes flipping it
   at debug-start affordable rather than a full reanalysis.
2. **The caller must recompile.** The flag governs what the *next* compile
   emits, not the artifact already in hand. Toggling bumps the session
   generation, so the next compile is a real one and not a cache hit.
3. **It must reach the worker replica.** `compileProjectAsync` — the road
   the studio actually compiles on — routes through `projectQuery`, which
   runs on the worker session whenever one is live. The toggle is therefore
   in `project-session.ts`'s `WORKER_CONFIG_METHODS`; without that it would
   set cleanly on the main session and change nothing in the real studio,
   reproducing #3229 one layer up.

Until this landed, every position feature D4/D6/D7/D9 built was inert in
the studio: `EditorSession`'s own compile hardcoded the flag off, and the
live session runs on exactly its bytes. Nothing failed, because every proof
opted in through `OptionOverrides { debug_info: true }` — a path no studio
code takes. The regression tests for this ruling therefore drive
`EditorSession::compile_project` itself and feed its `story_bytes` to the
same `WebSession` `LocalSessionProvider` constructs, asserting both states:
off resolving to nothing is as much the contract as on resolving to source.

The 2026-08-25 perf-ruling went the other way for the perf HUD (that stayed
in release builds); the distinction is payload: the perf panel is
structurally content-free (counters, timings), whereas debug info embeds
source byte-ranges and symbol names lifted straight from the author's
project — shipping it in a release build would leak source text and
identifiers into a distributed artifact for no player-facing benefit.

Consequence for the wire design (§2): because the section is
omitted-when-absent (the same "self-framed in the offset table" pattern as
`Visibility`/`FrameShapes`), **every existing release-shipped `.inkb` stays
byte-identical** — this section changes nothing for a story compiled
without the debug flag. No `VERSION` bump, no oracle exposure (the oracle
never compiles with the debug flag).

### 1.3 Breakpoint anchors: range-keyed in v1

Byte-range anchors ship now; breakpoints re-anchor on recompile and may
drift across edits. The `NodeId` column stays reserved for v2 per Q-R4
(2026-07-19) — this document's entry design (§2.2) leaves that column out
entirely rather than reserving dead bytes for it, because Q-R4's own
ruling is that the *column* is reserved conceptually, and the section is
already section-locally versioned (one prefix byte, §2.1) so a v2 bump can
append it without disturbing v1 readers.

Rationale (already ruled): `NodeId`s do not exist yet, and blocking the
whole debugger on an unbuilt identity system is the worse trade — echoing
the standing warning that path-derived synthetic-container identity "was a
consistent source of pain during compiler development" (2026-07-07,
`docs/sourcemap-epic-evaluation.md`).

### 1.4 VM seam: feature-gated debug hooks

Step/breakpoint control follows the `effect-trace` precedent — paired
`#[cfg(feature = "...")]` / no-op-stub arms that compile out entirely in a
release build — rather than promoting `step_once` into the released public
API.

Verified precedent on `origin/main`: `brink-runtime`'s `effect-trace`
feature (`Cargo.toml:57`) gates real vs. no-op arms at
`crates/brink-runtime/src/vm.rs:66,1549-1620`. `step_once` already exists
today (`crates/brink-runtime/src/story/mod.rs:1423`) but is gated
`#[cfg(feature = "testing")]` and returns a debug string tuple, not a
structured position — it is a test probe, not the seam. D8's new feature
(a distinct flag, not a widening of `testing`) adds the real hooks:
breakpoint-hit check, pause, resume, step-into/over/out — each with a
release no-op stub compiled to nothing, keeping the production hot loop
and step-limit accounting (`CLAUDE.md` "Guard against unbounded growth")
completely untouched when the feature is off.

**Debug budget — separate from the production step limit** (implemented
D8, #3186; orchestrator decision-comment ruling, 2026-08-28, pending
maintainer confirmation — recorded here per the "spec drift" review note
on PR #3218 rather than left undocumented). `Story::debug_run`/
`debug_step`/`debug_run_watching` bypass `FlowInstance::advance_with_limit`
entirely (calling `vm::step` directly, per this section's own precedent
above), so they never read or write `Stats::steps` — the counter
`advance_with_limit`'s own `StepLimitExceeded` check reads. Instead each
call counts VM steps in a loop-local variable against a caller-supplied
`budget_ceiling`, defaulting to `DEFAULT_DEBUG_BUDGET = 200_000`
(`brink-runtime::debug_control`) — generous enough that ordinary
single-stepping never trips it, low enough that a `debug_run` that never
reaches an armed breakpoint, or a `debug_step` step-over/out whose target
frame never returns, reports back promptly instead of hanging a studio UI.
Exceeding it is the new public `RuntimeError::DebugBudgetExceeded {
breakpoint, ceiling }` — never `RuntimeError::StepLimitExceeded`, which would
misattribute the debug-only budget as the production one. See
`crates/brink-runtime/src/debug_control.rs`'s module doc for the full
accounting argument.

## 2. The v1 `DebugInfo` entry encoding (DECIDED HERE)

### 2.1 Granularity — RULED, not re-litigated

**RULED (maintainer, 2026-08-28):** adopt a DWARF-`is_stmt`-style design.

1. Every entry carries a **statement-boundary flag** (`IS_STMT`): "this
   entry is a recommended stop location / the start of a statement."
   Breakpoints and source-level stepping use the lowest-offset entry **with
   `IS_STMT` set** for a given line/statement; attribution (backtraces,
   error locations, coverage, profiling) reads **every** entry, flagged or
   not.
2. **v1 emits statement-level rows only** (every entry flagged) — that is
   all `lir::Stmt`/`Container` provenance can support today (delivered by
   #3183/#3189: `Container.provenance` and `Stmt.provenance` are both bare
   `Provenance`, never `Option`). Expression-level rows arrive later as
   **unflagged** entries (`IS_STMT` unset) — no format version bump, no
   reader change. This document's job is to make that literally true; see
   §2.2 for how the shape supports it and §5 for what those later rows look
   like once `lir::Expr` provenance exists.
3. A **prologue-end marker ships in v1 for real**, not deferred (§2.4).

The maintainer's reasoning: the native surface has expression richness
comparable to C, so stepping through expression evaluation is a real
authoring need, not a compiler-developer convenience — which is why
`lir::Expr` provenance (deferred by #3183) is now on the critical path
(#3183 stays open for it) rather than an optional refinement.

### 2.2 Entry shape and wire encoding

**The natural tuple, as scoped by the issue:** `(bytecode_offset, file_idx,
range_start, range_end, kind_token, flags)`.

**DECIDED: delta/varint for the high-cardinality entry table, fixed-width
`brink-format::codec` helpers for everything else.** The ruling's own
reasoning ("committing to eventual fine-grained rows means the v1 encoding
should assume a large entry count... do not defer compactness to a v2 that
would then need a bump") applies specifically to the *entry* table, whose
row count scales with statement (later, expression) count. It does not
apply to the file table or the locals table (§3), whose row counts scale
with file count and declared-temp count — both small and roughly constant
per container. So:

- **Entries** (per-container, sorted ascending by `bytecode_offset` — see
  below): unsigned LEB128 varint fields, chosen because it is the same
  scheme DWARF itself uses for exactly this kind of table, and because
  nothing in `brink-format::codec` today provides variable-width encoding
  (verified: `codec.rs` has only fixed-width `write_u8/u16/u32/u64/i32`).
  D6 adds `write_varint`/`read_varint` (unsigned LEB128) to `codec.rs`
  alongside the existing fixed-width helpers, scoped to this section only
  — every other section keeps the format's established fixed-width house
  style; this is a deliberate, ruled departure for one section, not a
  format-wide encoding change.
- **File table** (§2.3): reuse the existing `codec::write_str`/`read_str`
  (fixed `u32` length prefix + UTF-8 bytes) and a fixed `u32` file count.
  Row count here scales with distinct files referenced, small and roughly
  constant per artifact — it doesn't benefit from variable-width encoding,
  and reusing the established helpers is simpler than adding a second
  string encoding for it.
- **Locals table** (§3): varint, same as the entry table above — not
  fixed-width. `local_count` scales with authoring style (declared-temp
  count per container) the same way `entry_count` scales with statement
  count, and the locals table reuses the entry table's own
  `(file_idx, range_start, range_len)` varint shape verbatim for its
  optional declaring range (§3) rather than defining a second encoding for
  the same three fields. `codec::write_str`/`read_str` still carries the
  `name` field — varint applies to the count and index/range fields only.

**Entry field encoding** (one entry, in this order):

| Field | Encoding | Notes |
|---|---|---|
| `bytecode_offset` | varint, **delta from the previous entry's offset** in the same container (first entry: delta from 0) | Always ≥ 0 because entries are sorted ascending by offset (below) — deltas are never negative, which is exactly what makes varint pay off here. |
| `file_idx` | varint | Index into this section's own file table (§2.3), **not** the compiler's project-wide `FileId` — see §2.3 for why a fresh, section-local numbering is used. |
| `range_start` | varint | Absolute source byte offset within the file. Not delta-encoded against the previous entry's range: unlike `bytecode_offset`, source ranges are **not** monotonic across entries (a container's statements can reference source text out of textual order — sequences, conditional branches, diverts that jump backward in the file). Delta-from-self within one entry (below) is safe; delta-from-neighbor is not. |
| `range_end` | varint, **delta from `range_start`** (i.e. the range's length) | `range_end >= range_start` always holds (`Provenance`'s own admission contract requires a non-empty, in-bounds range — `brink-ir/src/provenance.rs`), so this delta is always ≥ 0 and typically small (most spans are a few dozen bytes), which is exactly the varint-friendly case. |
| `kind_token` | fixed `u32` | `KindToken::as_u32()` verbatim (class in the high 16 bits, raw in the low 16 — `brink-ir/src/provenance.rs`). Fixed-width, not varint: it's one field per entry, already bit-packed to its minimum useful width by the existing type, and varint-encoding an already-dense bitfield buys nothing. See §2.3 for how `raw` gets resolved despite being frontend-private. |
| `flags` | fixed `u8` | Bit 0 = `IS_STMT`. Bit 1 = `PROLOGUE_END` (§2.4). Bits 2–7 reserved (zero in v1). A `u8` (not varint) because it's a small fixed bitfield, not a magnitude that benefits from variable width. |

**Reserved-bit forward compatibility — DECIDED, departs from house
strictness on purpose.** This format's default posture toward unknown wire
values is strict rejection — `InvalidSectionKind`, `UnknownOpcode`,
`InvalidLinePart` (`docs/format-spec.md`) all reject rather than tolerate.
That posture does not apply here: **a `DebugInfo` reader MUST ignore any
reserved `flags` bit it does not recognize, and MUST NOT reject an entry
solely because a reserved bit is set — the same tolerance covers any
per-entry trailing bytes a later version appends** — exactly the opposite
of the section-tag/opcode rejection rule. This is what §2.1/§5's promise
("expression rows arrive later... no reader change") actually requires:
those later rows are unflagged `IS_STMT` entries today, but the promise
only survives a *future* revision that also wants a new per-entry flag bit
or field if today's reader already tolerates bits and bytes it doesn't
know about — a strict reader would reject the first such artifact,
breaking the additive-evolution property this section exists to have. A
`version` bump (§2.2's opening framing) remains the escape hatch for a
change too large to express this way.

**Sorted-by-offset, per container.** Entries for one container are emitted
in ascending `bytecode_offset` order and MUST cover the container's full
address range (see §2.4 for how offset 0 is covered) with no gaps a reader
would need to guess across. This is what makes "what source is at
bytecode offset X" a binary search: find the greatest entry whose
`bytecode_offset <= X`, i.e. a **floor** lookup — the same shape as DWARF's
own line-table consumption, and the same shape `IS_STMT`/`PROLOGUE_END`
lookups reuse (find the lowest-offset entry with the flag set, scanning
forward from a floor position).

**Reader-side allocation guard.** Per `CLAUDE.md` ("Guard against unbounded
growth") and the existing `safe_capacity` precedent
(`brink-format/src/inkb/mod.rs`, used by every other section reader against
crafted/corrupt count fields): D6's `DebugInfo` reader must run every count
it reads (file count, per-container entry count, locals count) through the
same `safe_capacity` cap before allocating, exactly like every other
section in this format already does. Not optional — a debug section
reader is no less exposed to a truncated/crafted `.inkb` than any other
section reader.

**Per-container table framing.** One table per container, in the same
order and count as the `Containers` section (`0x06`) — i.e. the `DebugInfo`
section's Nth table describes the container at `Containers[N]`, addressed
by the same `container_idx: u32` the runtime's `ContainerPosition` already
uses (`brink-runtime/src/story/call_stack.rs:18`). This is a deliberate
choice: it means a running VM's `(container_idx, offset)` position resolves
to source with a direct array index into this section, no `DefinitionId`
lookup on the read path — consistent with "the runtime never sees
`DefinitionId` on the hot path" (`docs/format-spec.md`). A container's
`DefinitionId` (for name-based tooling, e.g. "set a breakpoint on knot
`tavern.order`") is available by reading `Containers[N].id` in tandem; it
is not duplicated into this section.

```
DebugInfo section (tag 0x11), v1:
  version: u8 = 1
  file_table: FileTable                 (§2.3)
  container_count: u32                  (fixed — one per Containers[] entry, small)
  containers: [ContainerDebugTable; container_count]

ContainerDebugTable:
  entry_count: varint
  entries: [Entry; entry_count]         (sorted ascending by bytecode_offset)
  locals: LocalsTable                   (§3)

Entry:
  bytecode_offset_delta: varint
  file_idx: varint
  range_start: varint
  range_len: varint                     (range_end = range_start + range_len)
  kind_token: u32
  flags: u8
```

**`.inkt` dump parity.** Per house pattern — `FrameShapes` shipped as
".inkb tag `0x10` + `.inkt` `(frame_shapes …)`" precisely because "atoms
land with the reader" (`docs/flow-suspension-spec.md` §11.3) — D6 must add
a `.inkt` textual-dump rendering for the `DebugInfo` section (file table,
per-container entry table, and locals table) alongside the binary
reader/writer, not ship a debug section with no inspection path.

### 2.3 The section-local file table

**The file table is section-local, not the compiler's project-wide
`FileId` space.** Only files actually referenced by an emitted entry need
appear, so the table stays small even in a large multi-file project; D6
builds a fresh mapping (`FileId -> file_idx`) when it emits the section,
independent of whatever numbering `brink-db` assigned during compilation.

**Each entry also carries `source_hash` and a line index (#3261, RULED
2026-08-28).** The file table row is
`(surface, path, source_hash: u64, line_starts: varint-delta list)`.

`source_hash` is [`content_hash`] of that file's text **exactly as the
compiler consumed it** — no normalisation of line endings, whitespace or
encoding on either side, since a reader that normalises differently sees a
permanent false mismatch. It exists because both resolvers otherwise answer
questions about source they were never built from: the author types, the
recompile is still debounced, and a gutter click asks about the *current*
buffer against the *previous* program, getting a confidently wrong address
rather than an error. This is a hazard for byte ranges every bit as much as
for line numbers — offsets shift on every inserted character. Per-file
deliberately, so one dirty file degrades debugging in that file alone where
a whole-program checksum degrades everything. It is a change **detector,
not a proof**: `content_hash` is FNV-1a, not collision resistant.

`line_starts` records the byte offset of every line start, ascending,
beginning with `0`, delta-encoded on the wire (line lengths are small, so
each costs about one varint byte). A trailing newline does **not** open a
final empty line: `"a\n"` is one line, matching how every editor numbers
it. Carrying it means a reader answers `file:line` **without being handed
source text at all** — the shape a remote frontend needs (DAP's
`setBreakpoints` is file + line, and an adapter may hold no source), and
the reason line↔byte conversion has one implementation rather than one per
consumer. Line indices are **0-based** here; a UI showing 1-based numbers
converts at its own edge.

Both degrade rather than fail. A compile that supplies no source text
(`EmitOptions::debug_sources = None`) emits `source_hash: 0` and an empty
index: positions still resolve, only staleness detection and `file:line`
lookup are unavailable. `source_hash: 0` reads as "cannot tell", which is
deliberately distinct from "stale" — collapsing them would make every
hash-less artifact look permanently dirty. The reserved synthetic sentinel
at index 0 names no real file and carries both empty.

**Paths are project-root-relative, not process-cwd-relative or absolute.**
Verified on `origin/main`: a *registered* file key is spelled relative to
the process's cwd at compile time (`brink-db/src/modules.rs:132`,
"LOAD-BEARING" doc on `root_relative_key`) — that's an artifact of how the
CLI happened to invoke the driver, not a stable identity. `brink-db`
already normalizes this for portability via `root_relative_key`
(`brink-db/src/db.rs`), stripping the project root so the result is stable
across whatever directory the compiler was invoked from. Debug info
persisted inside a compiled artifact has the same portability need — a
`brink.toml` project root is a stable identity; a compiling process's cwd
is not — so the file table stores the same root-relative form, computed the
same way, rather than either the raw registered (cwd-relative) key or an
absolute filesystem path that would be meaningless on a different machine.

**Surface-per-file is REQUIRED (RULED, maintainer 2026-08-28).** Verified
on `origin/main`: `KindToken` (`brink-ir/src/provenance.rs`) is
deliberately split, with its own doc stating the contract —

> `class`: "Frontend-agnostic node class — the only pipeline-interpretable
> half." `raw`: "Frontend-private raw syntax kind (ink: `SyntaxKind as
> u16`)."

There are exactly two `ProvenanceResolver` impls (`InkProvenanceResolver`
at `hir/ink_provenance.rs`, `NativeProvenanceResolver` at
`hir/lower_native/provenance.rs`), each with independent `u16` numbering
for `raw`. A `KindToken` stored without knowing which surface parsed the
file it came from is uninterpretable on read: handed to the wrong
resolver, `raw` mostly returns `None` — a legitimate answer under the
resolver contract (`ProvenanceResolver::resolve`'s own doc: "foreign,
synthetic, or stale" all answer `None`), so misrouting fails **silently**,
and where the two frontends' numbering happens to collide it resolves to
the **wrong node kind** instead of failing at all — worse than a crash.

**DECIDED: v1 carries the full `KindToken` (class + raw) unconditionally,
with surface recorded once per file in the file table** — not per entry,
and not dropped down to `class`-only. This was explicitly framed as an
open choice ("the file table carries the surface per file, or the entry
carries it, or v1 stores only the class half"); the file table is the
right place because surface is a property of *where the code came from*,
constant for every entry pointing at that file, so recording it once per
file (rather than once per entry, which is what the entry-level option
would cost) is strictly cheaper and exactly matches the granularity of the
fact being recorded. Carrying the full token is "free" (issue's own
framing) once the file table exists to disambiguate `raw` — a reader picks
`InkProvenanceResolver` or `NativeProvenanceResolver` by reading
`file_table[entry.file_idx].surface` before interpreting `entry.kind_token`
— so there is no reason to ship the strictly-less-useful `class`-only
option. `class` alone still works with no surface lookup at all for
consumers that only need coarse classification (e.g. "is this a choice
line" for a breakpoint icon) and never call `resolve()`.

**The reserved synthetic sentinel (index 0).** See §2.5 for why file_idx 0
is always reserved, regardless of whether any entry uses it.

```
FileTable:
  file_count: u32                       (fixed — small, one per referenced file)
  files: [FileTableEntry; file_count]
                                         (index 0 is ALWAYS the reserved
                                          synthetic entry — see §2.5;
                                          real files start at index 1)

FileTableEntry:
  surface: u8                           (0 = Synthetic, 1 = Ink, 2 = Native)
  path: string                          (codec::write_str/read_str; empty for
                                          the index-0 synthetic entry;
                                          project-root-relative otherwise)
```

### 2.4 The prologue-end marker (RULED, ships in v1)

**RULED (maintainer, 2026-08-28):** a breakpoint set on a knot must land
past any per-container setup bytes, not at container byte 0 — that setup
structure exists in emitted containers today, so without a marker the
studio has to reconstruct the right offset across the wasm boundary and
will get it wrong. This ships for real in v1, not deferred like `NodeId`,
because — unlike the `NodeId` case — the information needed to place it
(the container's own `Provenance`, already delivered by #3183) exists
today.

**What the setup bytes actually are.** Verified against
`crates/internal/brink-codegen-inkb/src/container.rs` and `content.rs`:
`Opcode::EnterContainer` is emitted from `lir::StmtKind::EnterContainer` /
`lir::ContentPart::EnterSequence` in the **caller's** stream, when
transferring control into a child container — it is not part of the
entered container's own bytecode, so a container's own offset 0 never has
an `EnterContainer` instruction to skip past. `ChoiceOutput` is a
`lir::Stmt` variant (`brink-ir/src/lir/types.rs`), emitted through the same
`emit_body`/`emit_stmt` path as any other statement in a choice-target body
container — real leading bytecode of that container, not a preceding
opcode preamble. The real per-container prologue is: (1) the leading
param-binding `DeclareTemp`s a parameterized container's `Param count`
byte promises (`docs/format-spec.md` Containers: "The prologue binds them
with that many leading `DeclareTemp`s"), plus (2) for a choice-target body
container specifically, its leading statement(s) lowered from
`lir::StmtKind::ChoiceOutput`. `PROLOGUE_END` is defined against *that* —
the container's own leading `DeclareTemp`/`ChoiceOutput` bytecode — not
against an `EnterContainer` opcode that never lives inside the container at
all.

**DECIDED: `PROLOGUE_END` is a flag bit on the entry whose
`bytecode_offset` *is* the landing point**, not a separate offset field —
symmetric with how `IS_STMT` is consumed (§2.1): a reader wanting "where do
I put a breakpoint set on this knot" does the same operation as "where's
the next statement stop" — floor to the container's first entry, then scan
forward for the lowest-offset entry with `PROLOGUE_END` set, and use *that
entry's own* `bytecode_offset` directly. No second lookup table, no
pointer field to keep in sync with the entry it describes.

**Coverage guarantee.** Every container gets at least one entry, at
`bytecode_offset` 0, covering the leading `DeclareTemp`/`ChoiceOutput`
prologue bytes described above for attribution purposes (this is what
makes the "no gaps" binary-search invariant in §2.2 hold even for
containers whose first *statement proper* starts partway in) — using the
container's own `Container.provenance` as that entry's `(file_idx, range)`.
Recommended emission for D6: if the container has no prologue bytes (no
declared params, not a choice-target body — execution begins immediately
at the first statement), the offset-0 entry and the first statement's
entry coincide and a single entry carries both `IS_STMT` and
`PROLOGUE_END`. This is D6's emission strategy to get right, not a
wire-format requirement beyond "offset 0 is always covered and the
prologue-end flag is somewhere at or after it" — the format does not
mandate the merge, only that both facts are representable, which the
flag-bit design already guarantees.

### 2.5 The `FileId(u32::MAX)` synthetic sentinel — wire meaning

Surfaced during review of #3189 (D5): `Provenance::synthetic` stamps
`FileId(u32::MAX)` (`brink-ir/src/provenance.rs`), and that sentinel now
reaches LIR on two real nodes — the implicit whole-project root
`Container` and the C#-compat `#root-terminus` gather (both genuine
artifacts of assembly with no single source file to anchor to; see
`NodeClass::Stmt`'s doc, "the synthetic root-terminus statement in
`lower::mod`"). It needed a defined wire meaning; three options were
named: omit the entry entirely, reserve a sentinel file-id slot in the
file table, or something else.

**DECIDED: reserve file_idx 0 in the section-local file table as the
synthetic sentinel** (§2.3's table above) — never assigned to a real file,
regardless of whether any entry actually references it in a given
artifact. `Provenance::synthetic`'s `FileId(u32::MAX)` maps uniformly to
`file_idx = 0` when D6 builds the section-local numbering; no per-artifact
conditional, no special-casing at write time beyond "the fresh numbering
this section always builds starts real files at 1."

**Rejected: omit the entry entirely.** The root container and
`#root-terminus` gather are still real containers with real bytecode and a
real `Container.provenance` (a synthetic `Provenance` still carries a real,
if synthetic-derived, `TextRange` — `Provenance::synthetic`'s own doc:
"Carries a real `range`... but a file/raw pair no frontend claims"). §2.4's
coverage guarantee requires every container to have an entry at offset 0;
omitting the synthetic-provenance containers' entries would poke a hole in
exactly the containers where the coverage invariant matters most for
attribution (a fault or backtrace frame landing in the root container
still needs *some* answer, even a "this has no source" answer, rather than
an unbounded floor-search past the start of the table). Reserving a
sentinel slot keeps the binary-search/floor-lookup contract uniform across
every container with zero reader-side special casing beyond "check if
`file_idx == 0`."

**Reader contract:** an entry with `file_idx == 0` never resolves to a live
document position (`FileTable[0].path` is always the empty string, `surface
== Synthetic`) — a consumer must not attempt source highlighting,
breakpoint placement, or `resolve()` for it. It remains usable for coarse
attribution (e.g. labeling a call-stack frame "root" / "story assembly"
rather than pointing at a source line) and for the coverage/floor-lookup
invariant. This mirrors the existing resolver contract's own posture:
`ProvenanceResolver::resolve` already treats synthetic provenance as a
normal `None` case, not an error.

### 2.6 Continuation containers — wire meaning (D1's call, per FS-3 §11.2's "may")

`docs/flow-suspension-spec.md` §11.1 splits a container at every `await`/
`until` site: everything after the park becomes a synthesized
**continuation container**, marked `CountingFlags::INVISIBLE` (§11.2 —
"hidden from IDE navigation/completion (debug views may show them)"). This
is a real container in the `Containers` section with real bytecode, not a
sidecar or a special case of `Containers` indexing — §2.2's
`container_idx`-lockstep framing ("one table per container, in the same
order and count as the `Containers` section") already covers it with no
extra machinery needed at the wire level, but three things §11.2 leaves as
"may" needed a decision to make the rest of this document's contract hold:

**DECIDED: continuation containers get a `DebugInfo` table like any other
container — no omission, no special casing beyond what follows.**
`INVISIBLE` (§11.2) governs *story-structure* visibility — no visit
counts, not a valid divert target, absent from IDE navigation/completion —
it says nothing about *debug* visibility, and a continuation container has
real bytecode that a parked-then-woken flow genuinely executes: a backtrace
or step landing inside one with no `DebugInfo` table would violate this
document's own "every container resolves" contract (§2.2's coverage
guarantee, §2.5's sentinel reasoning) for exactly the containers where a
debugger user is most likely to be looking — right after a wake. Omitting
it would also break `container_idx` lockstep itself: the `DebugInfo`
section's Nth table must describe `Containers[N]` for *every* N, so a
continuation container occupying a slot in `Containers[]` needs a
`DebugInfo` table in that same slot or the lockstep invariant is false for
any project that has ever compiled an `await`/`until` site.

**What the offset-0 entry anchors to.** A continuation container's
`Container.provenance` is **not synthetic** — unlike the root container/
`#root-terminus` case (§2.5), the source text a continuation container
executes is real: the statements textually following the park point in the
enclosing tunnel. §2.4's coverage guarantee therefore anchors a
continuation container's offset-0 entry the same way as any ordinary
container's — using `Container.provenance` — with one added property worth
recording explicitly: this container's source range starts **after** the
`await`/`until` statement's own range, not at the enclosing tunnel's start,
so a source view following a resumed flow's position naturally lands past
the park statement rather than back at the top of the tunnel. No new field
or entry kind is needed; this falls directly out of using the codegen-
assigned `Container.provenance` that already exists for it.

**What the studio shows while parked (the D8/D9 question §11.2 leaves
open).** While a flow sits parked (`Step::Suspended`, no VM turn active),
there is no *currently executing* position to resolve at all — nothing is
running. `FlowFrame` (`docs/flow-suspension-spec.md` §2) names the
continuation container as where execution **will resume**, not where it
currently is. The studio's call-stack/position display for a parked flow
must therefore present that as a resume point, not a live position: resolve
`(continuation container_idx, offset 0)` through this section exactly like
any other lookup, but label the frame "parked — resumes here" rather than
"currently at," and gray out step controls other than the condition-park-
aware ones defined in §4. This is also the concrete reason a naive
same-frame step model breaks at a wake boundary, which §4 names but does
not derive: pre-park execution lived in one `container_idx` and post-wake
execution lives in a **different** one (the continuation container), so a
debugger frontend must always re-resolve position via `container_idx` on
each `Step`/wake rather than assuming the active frame's container is
stable across a park — exactly the same discipline any call/return already
requires, not a new discipline this document has to invent, just one it is
naming so D8/D9 don't miss it at a park boundary specifically.

## 3. The symbol/scope model for variable inspection

Scoped per the issue's ask: "per-container temp slot to name, and whether
slot to declaring-range is in v1"; the format-payload half of D7 (#3185,
blocked on this document).

**What "scope" means for ink's temps.** A `~ temp` declaration is scoped to
the container it's declared in and the VM temp slots (`DeclareTemp(u16)` /
`GetTemp(u16)` / `SetTemp(u16)`, `brink-format/src/opcode.rs:1173-1175`)
that back it are allocated per active call frame, not lexically nested in
the C sense — brink has no block-scoped shadowing construct distinct from
statement position today. **v1 models one flat `LocalsTable` per
container**: `slot -> name` (+ optional declaring range, decided below).
This does not claim full lexical-scope nesting (if a future codegen change
reuses a slot number for two different declared names within one
container — e.g. across non-overlapping sibling `if`/`else` bodies — v1
represents that as multiple entries for the same slot, in declaration
order; see below for how a reader disambiguates). That refinement (exact
bytecode-validity ranges per binding, distinct from the source
declaring-range below) is intentionally left for a v2 alongside the
reserved `NodeId` column, not designed here, because it isn't known
whether slot reuse across sibling scopes actually happens in codegen today
— an open question for D6/D7 to check against the real register allocator,
not this document to assume either way.

**Shape, modeled on the existing `SlotInfo` idiom.** `brink-format` already
has a small, established idiom for "index -> name" tables:
`SlotInfo { index: u8, name: String }` (`brink-format/src/definition.rs`),
used on `LineEntry` for localization placeholder naming. The `LocalsTable`
entry below follows the same *pattern* (small inline index-to-name pairs,
`codec::write_str` for the name) — but is not the same field and does not
reuse its `u8` width: `SlotInfo`'s `u8` fits its own domain (line-table
placeholder indices), while VM temp slots are addressed by the `u16`
operand every `DeclareTemp`/`GetTemp`/`SetTemp` actually uses
(`brink-format/src/opcode.rs`), so the locals table's `slot` field is `u16`
to match the real instruction operand width it needs to key against.

**DECIDED: slot → declaring-range IS in v1.** This was framed as a real
open choice ("whether slot to declaring-range is in v1") rather than an
assumed yes, so it's decided explicitly, for two reasons:

1. **Cost is low.** Declaring-range entries are one per *declared* local,
   not one per instruction — there are far fewer `~ temp` declarations in a
   typical container than statements, so the marginal size cost is small
   relative to the entry table (§2.2) the ruling already committed to
   growing. It also reuses the `(file_idx, range_start, range_len)` shape
   already defined for entries — no new encoding to design, just an
   optional presence of fields already specified.
2. **It's the natural key for slot-reuse disambiguation**, not just a
   locals-panel "go to declaration" nicety. If a slot number is genuinely
   reused for two different names within one container (open question,
   above), the declaring range is exactly what a reader needs to pick the
   right entry: choose the LocalsTable entry for a given slot whose
   declaring range's start most closely precedes the current bytecode
   offset's mapped source position. This is an approximation (a true
   bytecode-validity range per binding would be exact — deferred to v2 per
   the note above), but it costs nothing beyond what's already decided to
   ship, and is strictly better than no declaring-range at all, which would
   leave slot reuse with no disambiguation signal whatsoever.

Omitting it would mean the locals panel's "jump to declaration" has to
approximate the location by re-scanning the entry table for the nearest
`TempDecl`-classed entry mentioning that name — strictly worse, for no
byte savings, since the range fields already exist as a type.

```
LocalsTable:
  local_count: varint                   (per-container declared-temp count;
                                          can scale with authoring style, so
                                          varint like the entry table)
  locals: [LocalEntry; local_count]

LocalEntry:
  slot: u16                             (matches DeclareTemp/GetTemp/SetTemp)
  name: string                          (codec::write_str/read_str)
  flags: u8                             (section version 2 — bit 0 has_range,
                                          bit 1 synthetic; reserved bits 2–7
                                          REJECTED, unlike the entry table's
                                          tolerated reserved flag bits;
                                          section version 1 wrote a bare
                                          has_range 0/1 here — same bit)
  file_idx: varint                      (present only if has_range)
  range_start: varint                   (present only if has_range)
  range_len: varint                     (present only if has_range)
```

**`synthetic` (issue #3395, RULED 2026-09-02).** A row whose temp the
compiler minted rather than the author: today, the lift-order hoist's
`$lift{n}` temps (`docs/compiler-spec.md`, "Normalization pass" —
`hir::TempDecl::synthetic` → `lir::StmtKind::DeclareTemp::synthetic` →
this bit). The row is real — slot, live value, a declaring range that
anchors to the content line the interpolation was hoisted out of — and
stays on the wire so a consumer that wants every slot can read it; the
runtime's `DebugLocal::synthetic` and `@brink-lang/wasm-types`'
`DebugLocal.synthetic` carry it through, and **the studio's locals views
(Debugger panel, State View) filter these rows out**, so an author only
sees the variables they wrote. A frame whose only locals are synthetic
renders exactly like a frame with none.

## 4. Frame semantics for step in/over/out

Scoped per the issue: define step-over/step-out for each `CallFrameType`
(`brink-runtime/src/story/call_stack.rs:37-50`: `Root`, `Function`,
`Tunnel`, `Thread`, `External`, `FunctionEvalFromGame`), across **both**
vocabularies — ink's tunnels/threads and the condition-park (`until` on the
native code ground, `~ await`/`~ while await` on the ink surface; §0) —
and say explicitly where there is no honest analogue. This section answers with
the vocabulary already established by the runtime's own types
(`CallFrameType`, `FlowFrame`/`Step::Suspended`), not by inventing new
narrative-VM semantics — the maintainer's instruction to stop and surface
applies to questions that need a product/design call, and "what does
GDB-style step-out mean applied to an already-ruled frame type" is a
mechanical application of standard debugger vocabulary, not a new design
decision. Nothing below required a maintainer call; §8 lists what does.

**step-into** (all frame types): run to the next `IS_STMT`-flagged entry,
descending into any newly-entered frame. This is uniform across frame
types — the only interesting question is what step-over and step-out mean.

**step-over and step-out, per `CallFrameType`:**

The `Thread` row below is the one entry that is *not* a `CallFrameType`. `<-`
pushes no frame of its own (issue #3561), so the debug surfaces synthesize it:
the frame a thread entered on — index `base_depth - 1` of a spawned thread — is
reported with `kind: "thread"` rather than by its own frame type, and
`Flow::at_thread_base` is what the step-out refusal below is keyed on. The
frame's reported position is the threaded knot's own container either way, so
this row's ruling reaches exactly the frames it always did.

| `CallFrameType` | step-over | step-out |
|---|---|---|
| `Root` | Run to the next `IS_STMT` entry at the same or shallower depth (i.e. normal statement-level stepping — a call into `Function`/`Tunnel` runs to completion without stopping). | **No honest analogue.** `Root` is the outermost frame — there is no caller to return to. The debugger must disable step-out (gray it out / reject the command) when the current frame is `Root`, exactly as GDB disables `finish` in the outermost frame. |
| `Function` | Run until control returns to this frame (a called `Function`/`Tunnel`/`External` completes) or the next `IS_STMT` entry within this frame, whichever comes first. | Run until this function returns (explicit `~ return` or falling off the end) and control resumes in the caller — the direct analogue of GDB `finish`. |
| `Tunnel` | Same shape as `Function`: run through any nested call without stopping inside it. | Run until the tunnel returns via `->->` (`TunnelReturn`) to the exact calling container — name-stable, so this is well-defined and matches GDB `finish` exactly. |
| `Thread` (not a `CallFrameType` — see below) | Same statement-level meaning as the others *while the thread is running*. | **No honest analogue — RULED explicit in the issue text: "a thread is not a frame you can return from."** A `<- target` thread is a fork, not a call: it pushes no call frame at all, re-pointing the fork's own copy of the innermost frame at the target and recording where the parent's frames end as `Thread::base_depth` (issue #3561, `docs/runtime-spec.md` "Call frame types"). Exhausting content there pops the whole *thread*; the frames below the mark are the parent's and are never unwound into. There is no caller-resumption event to run to. The debugger must not offer "step out" as if it returns anywhere; the closest *nameable* substitute is "run until this thread yields control back to the scheduler" (the thread exhausts, or the story reaches `Step::Choices`/`Step::Done` at a boundary this thread participates in) — but this must be presented in the UI as a distinct operation ("finish thread" / "run to next yield"), not labeled "step out," so the author is never told a return-to-caller happened when it didn't. |
| `External` | N/A as a frame to step over *into* — see step-into below. | Run until the pending `CallExternal` resolves and control resumes in the calling frame. This is well-defined (the frame pops when the orchestration layer resolves the external — `CallFrameType::External`'s own doc), but note under step-into: |
| `FunctionEvalFromGame` | Same as `Function` — the type's own doc says it "behaves like `Function` for output trimming and implicit-return purposes." | Run until the engine→ink evaluation returns to its engine caller (`FlowInstance::begin_function_eval`'s counterpart `resume_function_eval` completing) — the `Function` analogue holds here too. |

**`External` and step-into, specifically:** an `External` frame holds
*popped arguments* and a `DefinitionId` for a host-provided function
(`CallFrameType::External`'s doc: "the orchestration layer resolves it...
before the VM resumes") — there is no ink bytecode inside it to step
through. Step-into on a call about to push an `External` frame must behave
like step-over: it's opaque from the ink debugger's perspective, the same
posture a native debugger takes stepping into a call to a stripped or
no-debug-info library. (Bridging into host Rust code via brink-desktop's
own debugger, if the host binding happens to be debuggable that way, is out
of scope for this document and for the ink-level stepping model entirely.)

**Condition-park suspension — the second explicit "no honest analogue," at
the *statement* level rather than the frame-type level.**

⚠ **Surface vocabulary.** The park has a different spelling per ground, and
both lower to the same `AwaitStmt` HIR node — so the IR/runtime name must
never be read as the native surface keyword:

| ground | spelling |
|---|---|
| native code ground (`.brink`) | `until <pure-bool-expr>;` (one-shot only) |
| ink surface (brink extension) | `~ await <cond>` (one-shot), `~ while await <cond> { … }` (persistent, host-cancellable) |

`await` is **retired on the native surface**
(`crates/internal/brink-syntax-native/src/syntax_kind.rs`;
`docs/decision-log.md`, 2026-07-23, "Code-ground sitting", item 4): it
"plants the wrong future-resolution mental model, whereas brink's
construct is a **condition-park**." Author-facing debugger text must use
the ground's own spelling and must never describe a park as awaiting a
*value* — it parks on a *condition*, re-evaluated by the wake machinery,
per the 2026-07-23 retirement ruling. Whether the ink surface should be
renamed to match is an open design question (#3195); this document
describes what is true today and does not pre-empt it. Below, "await"
names the runtime/HIR mechanism only, never the native surface keyword.

The construct is statement-only and tunnel-only: awaiting composition
happens through `Tunnel` frames, never `Function`
(`docs/flow-suspension-spec.md` §4).

⚠ **Contested claim, do not rely on it here.** `docs/flow-suspension-spec.md`
§3 also says "Mid-expression `await` is permanently out (statement only)",
but the later block/effect-model ruling (`docs/decision-log.md`,
2026-07-20) ruled that "any operand-position suspension (await, choice,
coroutine call — no carve-out) is legal at the surface and ANF-lowered to
a statement boundary." The two disagree and the spec was never updated; a
third gap between `effects-spec.md` §13.1 and `flow-suspension-spec.md`
§3 is already flagged in the log as deferred "for a later pass". Tracked
in #3194. The stepping model below holds either way — ANF lowering means
a park is a statement boundary *by the time it reaches LIR*, which is the
only level this document maps.

A park does not push or pop a `CallFrameType` — it suspends the **entire
flow** via the FlowFrame model (`docs/flow-suspension-spec.md` §2) and
ends the VM turn with `Step::Suspended`, a terminal `Step` variant exactly
like `Done`/`End` (`CLAUDE.md`'s "Runtime public API"). This is a
**condition park**, not a value-delivery wait: resume happens when
`wakeCheck()` re-evaluates a **dirty** parked condition to true
(`docs/flow-suspension-spec.md` §10.2), not when a host delivers a value
at some future moment — the 2026-07-23 retirement ruling's whole point
was that the future-resolution mental model is the wrong one to teach
here, so this document uses condition-park / reactive-wake vocabulary
throughout, not "waiting for" language. Three distinct moments matter for
stepping:

1. **Approaching the park**: executing up to (and including) the `until` /
   `~ await` statement itself is ordinary statement stepping inside the
   enclosing `Tunnel` frame — no different from any other statement.
2. **At the park**: there is no synchronous "next instruction" to step to.
   Resume happens only when `wakeCheck()` re-evaluates the parked
   condition dirty-and-true — dirtiness comes from a write to something in
   the condition's read-set (`docs/flow-suspension-spec.md` §10.2), which
   may happen at some future, VM-external time, potentially long after the
   debugging session that hit the park has moved on to something else. A
   "step" command issued exactly at a park boundary cannot honestly behave
   like a normal statement step (there is nothing to run to,
   deterministically, right now). The honest behavior: a step command that
   reaches the park statement completes normally (it did execute); a
   *further* step command issued while parked must be presented as "flow
   parked — resumes when its condition next re-evaluates true" (condition-
   park terms, not a delivered-value framing — mirroring how
   `Step::Suspended` already reads at the runtime API level), never
   silently hang the debugger UI waiting for a wake that might not come
   during the debugging session at all.
3. **Step-out of a persistent `~ while await` park.** The one-shot case
   above covers `~ await`/`until`, whose exit is the condition becoming
   true. `~ while await cond { … }` is different: it is a *loop* whose body
   re-parks after every iteration, and its exit is **host-driven** — the
   host cancels the standing wake policy (the condition's false arm,
   `docs/flow-suspension-spec.md` §3 "while await desugar"), not any
   condition the debugged flow itself evaluates to true. There is no
   synchronous "run until this loop exits" a step-out can honestly offer,
   for the same reason as point 2: the exit event is not scheduled by the
   VM at all. The debugger must not offer "step out" of a `~ while await`
   loop as if it resolves during the session; the closest *nameable*
   substitute, matching the `Thread` no-analogue's posture above, is "run
   until this park's condition next re-evaluates" (i.e. behave like
   ordinary step-over across one park/wake cycle of the loop body) —
   presented as a distinct operation, not labeled "step out," since a
   genuine step-out (the loop's host-driven cancellation) is not an event
   the debugger can run to.

### 4.1 Parked/awaiting position reporting (#3225 — RULED, maintainer 2026-08-29)

The open question D8 shipped without deciding (what `Story::debug_position()`
and the frame/call-stack reads report while nothing is executing) is now
ruled, one answer per suspension kind:

- **Condition-park (`Step::Suspended`)**: report the **resume point** —
  `(continuation container_idx, offset 0)`, which resolves per §2.6 to the
  source just *past* the park statement — **explicitly tagged as a parked
  resume point**, never presented as a live position. The pre-park position
  was rejected (it names a statement that already executed and will never
  run again — a breakpoint set "here" would never hit on wake, and the
  arrow would point at a line the flow has permanently left); "no
  position" was rejected (it starves every consumer, which would have to
  reach around the API into `FlowFrame` internals to draw the ruled
  "parked — resumes here" treatment).
- **Deferred external (`StepOutcome::AwaitingExternal`)**: report the
  **calling frame's call site**, tagged with a *distinct*
  awaiting-external marker — the stack is intact and resumption returns
  exactly there; the `External` frame itself stays opaque per §4's
  step-into rule. The tag is distinct from parked because resumption is
  host-driven (`resolve_external`), not condition-driven — the two must
  never be conflated in author-facing text (condition-park vocabulary,
  §4).

**The tag is API-level, not UI courtesy.** An untagged position would let
a consumer honestly-mistakenly render "currently at"; the ruling requires
the reported shape itself to make that impossible.

**Implementation notes.** (1) `DebugSnapshot`/`DebugFrame`/`DebugPosition`
lack `#[non_exhaustive]` (#3215), so this additive shape change is
breaking today — land the hygiene fix in the same change. (2) FS-3r
(#980, the runtime park/spill/resume slice) is still open: this section
defines *reporting semantics* only, not the park machinery itself; the
implementation sequences against #980 consciously, not by accident.

## 5. What D6 should emit once `lir::Expr` provenance exists

Per the granularity ruling (§2.1): once #3183 delivers `lir::Expr`
provenance (now critical-path per the 2026-08-28 ruling, not an optional
refinement), D6 (#3184) should emit **additional entries** for
expression-level bytecode offsets — e.g. each operand-evaluation
instruction inside an assignment, divert-target expression, or conditional
test — with `IS_STMT` **unset**, interleaved by `bytecode_offset` alongside
the existing statement-level flagged entries in the same per-container
table.

Concretely, this means: no new `SectionKind`, no version-byte bump on the
`DebugInfo` section (still `version = 1`), and no reader change — a reader
built against this document today already does the right thing with an
artifact that has expression-level rows in it, because:

- Breakpoint placement and source-level stepping already only ever look at
  `IS_STMT`-flagged entries (§2.1, §2.4) — unflagged expression rows are
  invisible to those two operations by construction, exactly as intended.
- Attribution consumers (backtraces, coverage, profiling) already read
  every entry regardless of the flag (§2.1) — they get finer-grained
  answers "for free" the moment D6 starts emitting expression rows, with no
  code change on their side.
- The binary-search/floor-lookup contract (§2.2) does not care whether a
  table has 200 rows or 20,000 — sorted-by-offset is sorted-by-offset.

The only real work left to D6 at that point is walking `lir::Expr`
provenance (once it exists) and appending unflagged entries; this document
is what makes that additive rather than a redesign. If a future need
arises for expression-level rows to distinguish sub-kinds beyond what
`kind_token` (§2.2, already carrying the full `KindToken` including
`Infix`/`FieldAccess`/`Index`/etc. `NodeClass` variants — see
`provenance.rs`) already provides, that is a v2 question, not a v1 gap:
`kind_token` already has enough resolution for every `NodeClass` currently
defined, expression-level classes included.

## 6. Ship-policy interaction summary

Combining §1.2 (dev/studio-only) with the rest of this design: a
release-exported `.inkb` never contains a `DebugInfo` section at all (the
section is omitted, exactly like `Visibility`/`FrameShapes` when empty —
see `docs/format-spec.md`'s section-omission precedent), so nothing in this
document changes bytes in a shipped artifact. A dev/studio compile emits
the section per §2; brink-desktop (the first consumer, §0) reads it
through the wasm bridge (D9, #3187) using the `container_idx`-indexed
layout in §2.2 to resolve a running `FlowInstance`'s
`(container_idx, offset)` position (exposed by D4, #3182) to source.

## 7. Zero behavior change — how this document is verified

This is a documentation-only deliverable: no `SectionKind::DebugInfo` write
path, no reader, no VM hook exists yet — D2 (already merged, #3180/#3188)
retired the dormant `Opcode::SourceLocation`; D3–D9 build against this
document but are not part of this change. Verification is therefore the
full gate at the **unchanged** oracle ratchet
(`RATCHET_EPISODE_COUNT`, `crates/internal/brink-test-harness/tests/oracle_snapshots.rs`)
— any movement, in either direction, is stop-and-report per `CLAUDE.md`.
Not wasm-observable: `@brink-lang/web` exposes no debugger surface yet (D9
is what would add one), so no changeset is required for this PR.

## 8. Open questions this document does not resolve

Per the maintainer's instruction on issue #3179 ("if any sub-question
needs a MAINTAINER call rather than a technical answer, STOP AND SURFACE
IT — do not decide it yourself"), two things are explicitly left open,
neither invented an answer for here:

1. **Does debug stepping share the production step-limit budget?**
   Explicitly named in the epic's own "STATE AS OF 2026-08-27" comment as
   the one open `needs-design` item for **D8** (#3186), not for this
   document — D1's scope is the wire encoding and the ruled contract, not
   the VM's step-accounting policy while a debugger is attached. Recorded
   here so D8 doesn't have to rediscover that it's still open: this
   document does not answer it, and D8 must get a ruling before
   implementing the step-control hooks it needs.
2. **Whether the studio editor gives `.ink` files the same HIR-overlay
   treatment as `.brink`** (§0) — **verified, not just read, by D9
   (#3187)**: `brink-db`'s `projection_query` dispatches on `file_language`
   and both surfaces produce container spans with a `def_id`, proven end
   to end through `EditorSession::hir_spans_doc` for both surfaces in the
   same test
   (`crates/brink-web/src/editor/spans.rs::ink_files_get_def_id_carrying_hir_spans_like_native_files_do`).
   This was a fact to check, not a ruling to seek, so it was never
   escalated to the issue as a blocking question — it is recorded here now
   that D9 has answered it, so the "unverified" framing above and in §0
   is no longer live.

Everything else the issue asked this document to decide (§2 entry
encoding, §2.3 file table + surface tagging, §2.5 sentinel wire meaning,
§3 locals model, §4 frame semantics) is answered above as a technical
design, working inside the rulings already made — none of it required
escalating back to the maintainer.
