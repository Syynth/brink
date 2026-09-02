/**
 * @brink/wasm-types — Pure TypeScript interfaces for the brink wasm module.
 *
 * Zero runtime code — only type definitions. Every other package imports
 * types from here to avoid coupling to the wasm bindings.
 */

// ── Compilation ─────────────────────────────────────────────────

export interface Diagnostic {
  start: number;
  end: number;
  message: string;
  severity: "Error" | "Warning" | "Info" | "Hint";
  /**
   * Structured diagnostic code, e.g. `"E065"` (issue #1004). Lets consumers
   * filter or group diagnostics programmatically instead of string-matching
   * `message`. Optional for backward compatibility with older mocks/fixtures;
   * the wasm compile channel always populates it.
   */
  code?: string;
  /** Path of the file this diagnostic belongs to (may be an INCLUDEd file). */
  file: string;
}

export interface CompileResult {
  ok: boolean;
  story_bytes?: number[];
  warnings?: Diagnostic[];
  error?: string;
}

// ── Semantic tokens ─────────────────────────────────────────────

export interface SemanticToken {
  line: number;
  start_char: number;
  length: number;
  token_type: number;
  token_modifiers: number;
}

// ── HIR structural projection (#454) ────────────────────────────

/** The kind of a projected HIR span. */
export type HirSpanKind =
  | "knot"
  | "stitch"
  | "choice"
  | "gather"
  | "cond_branch"
  | "seq_branch"
  | "label"
  | "param"
  | "var_decl"
  | "const_decl"
  | "list_decl"
  | "list_member"
  | "external"
  | "temp_decl"
  | "divert"
  | "var_ref"
  | "call"
  | "content"
  | "interpolation"
  | "tag"
  | "include"
  | "divert_stmt"
  | "divert_terminal"
  | "logic"
  | "conditional"
  | "sequence";

/**
 * One HIR span projected onto the source: 0-based lines, UTF-16 columns.
 * `def_id`/`target_id` are opaque `DefinitionId` strings (`$tt_hash`) — compare
 * by equality (a reference's `target_id` equals its declaration's `def_id`).
 */
export interface HirSpan {
  start_line: number;
  start_char: number;
  end_line: number;
  end_char: number;
  kind: HirSpanKind;
  /** Block-level container (participates in rails / the per-line stack). */
  container: boolean;
  depth: number;
  def_id?: string;
  target_id?: string;
  /** Stable-within-doc container id; absent on non-containers. */
  handle?: number;
  /**
   * TIGHT end line for containers (two-range model, #3054): last line of
   * actual content — trailing whitespace and the next declaration's doc
   * block excluded. `end_line` remains the structural/ownership end.
   * Absent on non-containers.
   */
  content_end_line?: number;
  /** For Choice containers: `true` = sticky (`+`), `false` = once-only
   *  (`*`). Absent on everything else. */
  sticky?: boolean;
  /** Ink weave depth for Choice/Gather containers — the sigil depth,
   *  with inline choice sets inheriting the surrounding weave's depth.
   *  Distinct from `depth` (all container nesting). */
  weave_depth?: number;
}

/** One entry of a line's container stack. */
export interface HirLineContainer {
  kind: HirSpanKind;
  handle: number;
  depth: number;
}

/**
 * The structural projection of one document: nested spans plus, per line, the
 * stack of containers covering it (outermost→innermost) — the rails view.
 */
export interface HirProjection {
  spans: HirSpan[];
  lines: HirLineContainer[][];
}

// ── Runtime ─────────────────────────────────────────────────────

export type LineType =
  | "text"
  | "done"
  | "choices"
  | "end"
  /**
   * A flow parked at an `await` (`docs/flow-suspension-spec.md` §10.1).
   * Like `"done"`, a park is a turn boundary. Terminals carry no payload
   * of their own (`docs/prose-dialect-spec.md` §7, RULED) — any trailing
   * text already arrived as its own preceding `"text"` line. Drive the
   * flow again via `continueFlow`/`wakeCheck` when the host wants output;
   * a park never auto-continues.
   *
   * **Runtime-unreachable until FS-3r.** No `Line` the runtime produces
   * today carries this type (the E052 fence keeps `await` from lowering).
   * It ships now (FS-3w) so hosts migrate the API shape early.
   */
  | "suspended"
  | "awaiting_external";

export interface Line {
  type: LineType;
  text: string;
  tags: string[];
  /** The run of adjacent content this line belongs to
   * (`brink_runtime::BlockId`). Present only for `"text"`; terminals
   * carry no line payload, so no block id. */
  block_id?: number;
  /** This line's classification (`brink_runtime::Element`, issue #1683).
   * Present only for `"text"`, mirroring `block_id` above. `kind` still
   * always reports the degenerate `"narrative"` case — no `@[element]`
   * handler's own classification reaches `kind` yet. `data` is populated
   * as of issue #2108: an `@[convention(..., attach = StructName)]`
   * handler's returned struct fields merge into `data` on every line in
   * the run that follows it. */
  element?: ElementJs;
  choices?: Choice[];
  /** External name, present only on an `awaiting_external` line. */
  name?: string;
}

/** A line's classification (`brink_runtime::Element`). `kind` is an open
 * vocabulary owned by whichever preset/handler classified the line, not a
 * closed enum; `data` is an open, handler-defined payload. */
export interface ElementJs {
  kind: string;
  data: Record<string, string>;
}

export interface Choice {
  index: number;
  text: string;
  tags: string[];
}

// ── Save / load ─────────────────────────────────────────────────

/** Durable, name-keyed game-state save (globals, visit/turn counts, turn
 * index, RNG). Captures game state only — not execution position. Tolerant of
 * story patches. Treat as an opaque blob unless inspecting in dev. */
export interface SaveState {
  /** Save-FORMAT version (not the story's). */
  version: number;
  /** Global variables by name. Each value is a tagged ink value
   * (e.g. `{ Int: 10 }`, `{ String: "x" }`, `"Null"`). */
  globals: Record<string, unknown>;
  /** Each saved global's compiled `DefinitionId` at save time, keyed by the
   * same name as `globals` (M-3 rehydration miss-path lookup,
   * `docs/modules-spec.md` §5). A `"$tt_hash"` string, same format as
   * {@link VisitEntry.id}. A VAR/CONST/LIST living in a *declared* module
   * hashes its identity as `(module, name)`, so a bare name alone can't
   * reconstruct the id a `#@was` alias-table entry was compiled against —
   * this is the id `Story::load_state` consults when a saved global's name
   * no longer matches any current global slot. Older saves predating this
   * field carry no `global_ids` key at all on the wire
   * (`#[serde(default)]` on the Rust side); the loader defaults it to
   * empty rather than the key being present-but-empty. */
  global_ids: Record<string, string>;
  visits: VisitEntry[];
  turns: VisitEntry[];
  turn_index: number;
  rng_seed: number;
  previous_random: number;
  /** This flow's parked execution position when suspended mid-tunnel/mid-
   * `await` (`docs/flow-suspension-spec.md` §2/§9, FS-1). Absent for an
   * ordinary save at a turn boundary, choice, or `-> END` — and for any save
   * predating this field.
   *
   * **Format-only today.** `Story::save_state`/`load_state` always
   * produce/consume `undefined` as of FS-1; the compiler synthesis that
   * populates a live frame (FS-2) and the runtime spill/restore that
   * produces/consumes one (FS-3) are later slices — see #889. */
  suspended?: SuspendedFlow;
}

/** A visit/turn count for one scope. `id` (a `"$tt_hash"` string) is the load
 * key; `path` is an advisory author path present only for named scopes. */
export interface VisitEntry {
  id: string;
  path?: string;
  count: number;
}

/**
 * A parked flow's durable, recompile-stable execution position — the
 * `FlowFrame` (`docs/flow-suspension-spec.md` §2, RULED). Every field is a
 * name-stable identity (container/`DefinitionId`, never an instruction
 * offset), so it survives a story recompile the same way the rest of
 * {@link SaveState} does.
 *
 * FS-1 is format-only: today this is exercised only by round-trip tests on
 * the Rust side. Section-locally versioned independently of
 * `SaveState.version` (`SuspendedFlow.version`); bumped to `2` for #2108's
 * `next_block_id`/`pending_element`.
 */
export interface SuspendedFlow {
  /** Section-local format version, independent of `SaveState.version`. */
  version: number;
  /** The container the flow is currently parked inside (a `"$tt_hash"`
   * string). */
  current: string;
  /** The tunnel-return chain, outermost first (`"$tt_hash"` strings). */
  return_stack: string[];
  /** Every local crossing the yield, name-keyed — a tagged ink value (see
   * `SaveState.globals`). Treat as opaque unless inspecting in dev. */
  frame: unknown;
  /** The wake policy governing when the parked flow resumes. */
  wake: WakePolicy;
  /** The flow's `Flow::next_block_id` counter at the instant it was parked
   * (#2108, 2026-08-05 ruling: a resumed flow continues its block-id
   * sequence rather than colliding with fresh numbering). A save predating
   * this field carries no `next_block_id` key at all on the wire
   * (`#[serde(default)]` on the Rust side); the loader defaults it to `0`,
   * identical to the pre-ruling behavior. */
  next_block_id: number;
  /** Element-attachment metadata (`@[convention(..., attach = X)]`, #2108)
   * accumulated on the dialogue run open at the instant this flow parked.
   * `#[serde(skip_serializing_if = "BTreeMap::is_empty")]` on the Rust side
   * means the key is entirely ABSENT from the wire (not present as `{}`)
   * whenever no attach run was open, or for a save predating this field —
   * which is every save today, since `Story::save_state` always produces
   * `suspended: None` (no runtime spill/restore yet, FS-3/#889). */
  pending_element?: Record<string, string>;
}

/** A parked flow's wake policy (`docs/flow-suspension-spec.md` §2 point 4;
 * see `docs/effects-spec.md` §13.1 for the wake contract this plugs into). */
export interface WakePolicy {
  /** The `await` site's synthesized resume-container id (a `"$tt_hash"`
   * string). */
  site: string;
  /** The condition's compiler-synthesized pure-fn token id (a `"$tt_hash"`
   * string). Absent when `source` is `"Host"`. */
  condition?: string;
  source: WakeSource;
}

/** The wake policy's host-source discriminant. Only `"Condition"` is
 * compiler-produced today; `"Host"` is reserved for a future host-driven
 * wake source (next-frame, external event) with no compiled ink condition
 * fn — the host owns re-evaluation directly. */
export type WakeSource = "Condition" | "Host";

/** What a load couldn't apply. `unknown_globals` lists saved globals the
 * current story no longer declares; `unresolved_renames` lists saved
 * fn/divert/visit-turn-count keys that still didn't match after consulting
 * the compiled rename-alias table; `anonymous_states_dropped` counts saved
 * visit/turn-count entries for an *anonymous* scope (an unlabeled once-only
 * choice or a sequence — no name an alias table entry could ever be written
 * against) that could not be placed. All empty/zero means a clean load. */
export interface LoadReport {
  unknown_globals: string[];
  unresolved_renames: string[];
  anonymous_states_dropped: number;
}

// ── Story Session (#370/#387) ────────────────────────────────────
//
// `WebSession`'s wire shapes. Distinct from `Line`/`LineType` above (the
// legacy `StoryRunner` union, which smuggles `awaiting_external` into the
// `Line` type tag) — `StepOutcome` here splits that out into its own tagged
// union, per the story-session-spec wire-format fix. Tagged `type` +
// snake_case discriminants throughout, matching `Line`.

/** A tagged ink value, as the journal/save layer serializes it (never the
 * lossy `List`/`Divert` → null mapping the `ExternalValue` boundary uses).
 * e.g. `{ Int: 10 }`, `{ Float: 1.5 }`, `{ Bool: true }`, `{ String: "x" }`,
 * `"Null"`. Treat as opaque unless inspecting in dev. */
export type JournalValue = unknown;

/** One line of session output (mirrors `Line` above; kept as a separate type
 * because `StepOutcome` below never carries the `awaiting_external` variant —
 * that lives on `StepOutcome` itself). */
export interface SessionLine {
  /** `"suspended"` (a flow parked at an `await`) is runtime-unreachable
   * until FS-3r — see {@link LineType}. */
  type: "text" | "done" | "choices" | "end" | "suspended";
  text: string;
  tags: string[];
  /** The run of adjacent content this line belongs to
   * (`brink_runtime::BlockId`). Present only for `"text"`; terminals
   * carry no payload of their own (`docs/prose-dialect-spec.md` §7,
   * RULED), so `text`/`tags` are always empty and `block_id` is absent. */
  block_id?: number;
  /** This line's classification (`brink_runtime::Element`, issue #1683) —
   * present only for `"text"`, mirroring `block_id`. See {@link Line}'s
   * `element` field doc for today's scoping. */
  element?: ElementJs;
  choices?: Choice[];
  /** Transcript provenance (W7/#3300) — present only for `"text"` lines
   *  whose line-table entry carries a source location. */
  source?: SourceLocation;
}

/**
 * Outcome of one `WebSession.advance()` step — the wire-format fix (#387):
 * `awaiting_external` is its own variant, not smuggled into the `Line` union.
 * The two park states stay distinct: this type is always the **deferred**,
 * out-of-band kind (`deferred: true`) that the host resolves via
 * `resolveExternal` — a promise-in-flight park (the session awaiting
 * internally) is never surfaced as `StepOutcome` at all.
 */
export type StepOutcome =
  | { type: "line"; line: SessionLine }
  | { type: "awaiting_external"; deferred: true; name?: string };

/** One input recorded in the session journal (mirrors Rust `EventKind`). */
export type JournalEventKind =
  | { type: "start"; path?: string; args?: JournalValue[] }
  | { type: "choice"; index: number; label?: string }
  | { type: "external"; name: string; args?: JournalValue[]; result: JournalValue }
  | { type: "set_var"; name: string; value: JournalValue }
  | { type: "go_to_path"; path: string; args?: JournalValue[] }
  | { type: "load_state"; state: SaveState }
  | { type: "call"; name: string; args?: JournalValue[] };

/** One journal entry: the event plus reserved (serialized, uninterpreted in
 * v1) anchor/flow dimensions — see `docs/story-session-spec.md`. */
export interface JournalEvent {
  kind: JournalEventKind;
  anchor?: number;
  flow?: string;
}

/** The durable session journal — every input that entered the VM, plus a
 * fast-restore checkpoint. Serialize as-is for a save slot; feed back into
 * `WebSession.restore`. */
export interface SessionJournal {
  version: number;
  program_checksum: number;
  seed?: number;
  events: JournalEvent[];
  truncated: boolean;
  checkpoint?: SaveState;
}

/** Lightweight dirty signal delivered by `StorySessionHandle.onJournalDirty`
 * (#390). Carries just enough for a host to decide whether/when to persist —
 * pull the actual journal via `exportJournal()`. `eventCount` is the journal
 * length at the time the debounced notification fired (a monotonically
 * increasing counter across a session's lifetime, reset only by `restart`). */
export interface JournalDirtySignal {
  eventCount: number;
}

/** A soft, non-fatal replay observation. */
export type ReplayWarning = {
  type: "choice_label_drift";
  at_event: number;
  index: number;
  recorded: string;
  found: string;
};

/** What replay found at a divergence point instead of the recorded event. */
export type DivergenceFound =
  | { type: "choice_index_out_of_range"; index: number; available: number }
  | { type: "not_waiting_for_choice" }
  | { type: "unknown_path"; path: string }
  | { type: "unexpected_event" };

/** Why replay stopped without diverging. */
export type FailReason =
  | { type: "runtime_error"; message: string }
  | { type: "budget" }
  | { type: "awaiting_external"; name: string };

/** Outcome of replaying a journal (prefix) against a program — from
 * `WebSession.restore`/`reload`/`continueReplay`. Typed, never silent. */
export type ReplayOutcome =
  | { type: "replayed"; warnings: ReplayWarning[] }
  | {
      type: "diverged";
      at_event: number;
      expected: JournalEvent;
      found: DivergenceFound;
    }
  | { type: "failed"; at_event: number; reason: FailReason };

/** Resolved membership of a `List`-valued global in a `StateSnapshot`. */
export interface SnapshotList {
  items: string[];
}

/** One summarized call frame in a `StateSnapshot`. */
export interface SnapshotFrame {
  /** root | function | tunnel | thread | external | eval */
  kind: string;
  location?: string;
  temps: number;
}

/** active | waiting_for_choice | done | ended */
export type SnapshotStatus = "active" | "waiting_for_choice" | "done" | "ended";

/** A typed, name-resolved snapshot of a session's game state — distinct from
 * the string-valued `DebugState` (that one is for the studio's read-only
 * State View; this one is a first-class, diffable session artifact). */
export interface StateSnapshot {
  globals: Record<string, JournalValue>;
  lists: Record<string, SnapshotList>;
  turn_index: number;
  visit_counts: Record<string, number>;
  turn_counts: Record<string, number>;
  call_stack: SnapshotFrame[];
  status: SnapshotStatus;
}

/** Membership delta for one list-valued global (part of a `StateDiff`). */
export interface ListDelta {
  added: string[];
  removed: string[];
}

/** A pure diff between two `StateSnapshot`s (`WebSession.diff`/`diffSnapshots`). */
export interface StateDiff {
  added_globals: Record<string, JournalValue>;
  removed_globals: Record<string, JournalValue>;
  /** `[before, after]` per changed global. */
  changed_globals: Record<string, [JournalValue, JournalValue]>;
  list_deltas: Record<string, ListDelta>;
  /** `[before, after]`, present only when `turn_index` changed. */
  turn_index?: [number, number];
  pushed_frames: SnapshotFrame[];
  popped_frames: SnapshotFrame[];
}

// ── IDE types ───────────────────────────────────────────────────

export interface CompletionItem {
  name: string;
  kind: string;
  detail?: string;
  /**
   * Literal to insert when it differs from the displayed `name` — the host
   * value picker (#174) shows `HarborGate` but inserts `5`. Absent ⇒ insert
   * `name` (the default for ordinary symbol completions).
   */
  insert?: string | null;
  /**
   * `true` when the symbol is defined in a file NOT reachable from the current
   * file's INCLUDE graph (#312 F). Such rows get a "from <file>" affordance and,
   * on accept, auto-insert the `INCLUDE` alongside the symbol.
   */
  out_of_scope?: boolean;
  /**
   * Project-relative path of the file declaring this symbol — set only for
   * out-of-scope completions (the auto-import target).
   */
  source_file?: string | null;
}

export interface HoverInfo {
  content: string;
  start?: number;
  end?: number;
  /**
   * Navigation targets for `[text](#N)` links in {@link content}, where `N`
   * indexes this array.
   *
   * An index rather than a path inside the link target: a file path in
   * markdown would have to survive `)` and `:` inside it, and that escaping
   * is a silent-corruption bug waiting on the first bracket in a filename.
   *
   * An entry with an empty `file` is a target the compiler could not
   * resolve to a project file — rendered as plain text, never as a link
   * that goes nowhere. Entries are never dropped, because the indices in
   * the content refer to positions in this array.
   */
  links?: Location[];
}

export interface Location {
  file: string;
  start: number;
  end: number;
}

/** How a reference site uses the symbol — the Search panel's per-card
 *  badges (docs/search-results-cards-spec.md, PR E). */
export type ReferenceUseKind = "decl" | "call" | "divert" | "read" | "write";

/** A reference location plus its use kind (`find_references_with_kinds_at`). */
export interface LocationWithKind extends Location {
  kind: ReferenceUseKind;
}

/**
 * A handler location for the explain-match query (#2113) — a name plus its
 * declaration-site range in the project's conventions module. `start`/`end`
 * are **raw byte offsets**, not UTF-16, and are **file-absolute**, not
 * view-relative — see `explainMatch`/`explainMatchDoc`'s own docstrings on
 * {@link EditorSessionHandle} for why (mirrored from
 * `crates/brink-web/src/editor/explain_match.rs`'s module doc).
 */
export interface ExplainHandler {
  name: string;
  start: number;
  end: number;
}

/**
 * One named capture bound by a matched pattern, as a span into the
 * classified line's own file. Same raw-byte, file-absolute convention as
 * {@link ExplainHandler} — see there.
 */
export interface ExplainCapture {
  name: string;
  text: string;
  start: number;
  end: number;
}

/**
 * A field type's structural shape — mirrors `brink_ir::SchemaTypeShape`
 * verbatim, span-free (issue #2311): a bare nominal name, a generic
 * instantiation, or a function type, recursively.
 */
export type ExplainSchemaTypeShape =
  | { kind: "named"; name: string }
  | { kind: "generic"; name: string; args: ExplainSchemaTypeShape[] }
  | { kind: "fn"; params: ExplainSchemaTypeShape[]; ret: ExplainSchemaTypeShape };

/**
 * One resolved field of an `attach = StructName` schema (issue #2311):
 * the field's declared name and resolved type — schema, never a value any
 * handler computed.
 */
export interface ExplainAttachField {
  name: string;
  ty: ExplainSchemaTypeShape;
}

/**
 * The `attach = StructName` clause's resolution outcome (issue #2311) —
 * mirrors `brink_ir::ConventionAttachSchema`: `resolved` carries the
 * struct's declared name plus every field; `unresolved` carries just the
 * declared name for a clause that named a struct that does not exist
 * anywhere in the conventions module's own file or its import closure.
 */
export type ExplainAttachSchema =
  | { kind: "resolved"; name: string; fields: ExplainAttachField[] }
  | { kind: "unresolved"; name: string };

/**
 * One handler's classification-time match — the winner or a shadowed
 * runner-up. `kind` (issue #2310) — the claimed line's compile-time
 * structural shape — is present only on the `winner` a caller receives via
 * {@link ExplainMatch.winner}; a shadowed entry never carries one, since
 * only the actual winning claim has a compiled record to read it from (see
 * `crates/brink-web/src/editor/explain_match.rs`'s own module doc).
 *
 * All five `ElementKind` variants are declared, and issue #2351 fixed
 * `"cue"` and `"parenthetical"` to be reachable: the live walk now
 * classifies the same claim-candidate sub-node the compiler's own
 * `try_claim` matches against (a `CUE`/`COMPACT_CUE`'s `CUE_NAME`, a
 * `PARENTHETICAL`'s inner `TEXT`), not the whole raw line, so a real
 * `@NAME` cue or `(delivery)` parenthetical now agrees with the compiler.
 * `"bang_dispatch"` still cannot surface through this field (issue #2352):
 * `!name` dispatch handlers are registered on a path (`try_dispatch`) the
 * live walk never consults at all, and `candidate()` explicitly declines a
 * `BANG_DISPATCH` node rather than offering it a sub-node to classify.
 */
export interface ExplainClassifiedMatch {
  handler: ExplainHandler;
  order: number;
  mode: "attach" | "wrap";
  kind?: "content_line" | "scene_heading" | "bang_dispatch" | "cue" | "parenthetical";
  /** What a match on this handler produces — one variant today: `"call"`. */
  disposition: "call";
  /**
   * The handler's declared `attach = StructName` schema, if any (issue
   * #2311) — absent for a handler that only ever emits text.
   */
  attach?: ExplainAttachSchema;
  captures: ExplainCapture[];
}

/** One entry the walk attempted but that did not match. */
export interface ExplainAttempted {
  handler: ExplainHandler;
  order: number;
  mode: "attach" | "wrap";
  /** What a match on this handler would produce — one variant today: `"call"`. */
  disposition: "call";
  /**
   * The handler's declared `attach = StructName` schema, if any (issue
   * #2311) — absent for a handler that only ever emits text.
   */
  attach?: ExplainAttachSchema;
  pattern: string;
}

/**
 * The explain-match query's full per-line answer (issue #2113): is this
 * line matched, by what, what did it bind, and — on a miss — what was
 * attempted, or — on a hit — what else matched but was shadowed. `winner`
 * is present only when `matched` is `true`; `attempted` is populated only
 * when it is `false`.
 */
export interface ExplainMatch {
  matched: boolean;
  winner?: ExplainClassifiedMatch;
  shadowed: ExplainClassifiedMatch[];
  attempted: ExplainAttempted[];
}

export interface FileEdit {
  start: number;
  end: number;
  new_text: string;
}

export interface InlayHint {
  offset: number;
  label: string;
  kind: string;
  padding_right: boolean;
}

/** A `hex_color` argument literal for the built-in color picker (#174). */
export interface ColorHint {
  /** Start offset of the literal (including quotes), UTF-16. */
  start: number;
  /** End offset of the literal (exclusive), UTF-16. */
  end: number;
  /** The bare hex value, quotes stripped (e.g. "#FF0000"). */
  value: string;
}

/** The authoring state of an argument slot (argument-widget spec §4). */
export type SlotState =
  /** A literal — Edit replaces `[start, end)`; `value` is quotes-stripped. */
  | { kind: "filled"; start: number; end: number; value: string }
  /** No argument — Fill inserts at `insert_at` (`, `-prefixed if needed). */
  | { kind: "empty"; insert_at: number; needs_leading_comma: boolean }
  /** A non-literal expression — no inline affordance. */
  | { kind: "expr" };

/** One parameter slot of a call (argument-widget spec §4). */
export interface SlotWidget {
  param_name: string;
  /** The built-in widget kind for this slot's type (`color`, …), if any. */
  widget?: string;
  /** The semantic-type name, if the param is typed. Always the bare written
   *  name — widget-kind matching (`matchHostWidget`'s fallback) uses this
   *  field; render `type_display`, not this, for a user-visible label. */
  type_name?: string;
  /** The honest display string for `type_name` (#1027/#1053): the bare name
   *  when it resolves to a base keyword or a registered semantic type,
   *  `name ⚠ unregistered semantic type — E040` otherwise. Present iff
   *  `type_name` is present. */
  type_display?: string;
  /** Static value-list items (#174) — the Form renders these as a dropdown. */
  values?: ValueItem[];
  state: SlotState;
}

/** The authoring state of an arg-group (uniform across members; spec §2). */
export type GroupState =
  /** All members are literals — Edit replaces each `spans[k]` with a value. */
  | { kind: "filled"; spans: [number, number][]; values: string[] }
  /** All members empty — Fill inserts the members joined by `, ` at `insert_at`. */
  | { kind: "empty"; insert_at: number; needs_leading_comma: boolean };

/** An arg-group widget at a call site (argument-widget spec §2) — one widget
 *  spanning several params, with inter-arg context resolved. */
export interface GroupWidgetSite {
  /** Widget / semantic type (matches a host `ArgumentWidget.type`). */
  type: string;
  /** Editor container — `"popover"` (default) or `"modal"`. */
  surface?: "popover" | "modal";
  /** Param indices the group spans (the studio skips these per-slot). */
  param_indices: number[];
  param_names: string[];
  state: GroupState;
  /** Resolved inter-arg context: key → the sibling arg's literal value (from the
   *  document — what inline editing uses). */
  context: Record<string, string>;
  /** Raw inter-arg context: key → the sibling param index. The Form resolves
   *  context from its own live draft values via this map. */
  context_params?: Record<string, number>;
}

/** A call site with a per-parameter widget slot (argument-widget spec §4). */
export interface CallWidgetSite {
  callee: string;
  /** The call-name span (UTF-16) — anchors the form glyph. */
  name_start: number;
  name_end: number;
  slots: SlotWidget[];
  /** Arg-group widgets (spec §2) — render the group inline, skip its slots.
   *  Only present when the group's members are uniformly filled/empty. */
  groups: GroupWidgetSite[];
  /** Every declared arg-group for the callee, independent of the current args —
   *  the Form renders these (seeding member values from `slots`), so a partial
   *  or over-full call still gets its widgets. */
  declared_groups?: DeclaredGroup[];
}

/** A declared arg-group widget — manifest structure with no arg-state (spec §2).
 *  Used by the Form to render one control per declared group regardless of how
 *  many arguments the call currently has. */
export interface DeclaredGroup {
  type: string;
  surface?: "popover" | "modal";
  param_indices: number[];
  param_names: string[];
  /** key → the sibling param index supplying its inter-arg context. */
  context_params?: Record<string, number>;
}

// ── Host argument widgets (argument-widget-spec §3) ──────────────────
//
// Studio/host API types (not wasm-boundary types) — kept here as the shared
// base both @brink-lang/editor (the registry) and @brink/studio-shell (the
// `StudioExtensions.argumentWidgets` surface) import without a cross-dependency.

/** Context handed to a host widget's renderers (one entry per group member). */
export interface ArgumentWidgetContext {
  /** The widget's semantic type / id. */
  type: string;
  /** The EXTERNAL being called. */
  external: string;
  /** Param name(s) in the group (one for a single-param widget). */
  paramNames: string[];
  /** Current literal value(s), quotes stripped; empty for an unfilled slot. */
  values: string[];
  /** Resolved inter-arg context, e.g. `{ map: "5" }` (Stage 5). */
  context?: Record<string, string>;
}

/** The studio-provided handle a host widget editor resolves/cancels through. */
export interface ArgumentWidgetEditorHost {
  /** New literal value(s) for the group — the studio writes them back. */
  resolve(values: string[]): void;
  cancel(): void;
}

/**
 * A host-provided argument widget. The inline chip is *studio-rendered* from
 * label DATA (`inline` returns text + an optional CSS class); only the editor is
 * host-rendered, into a studio-owned popover/modal via a mount-callback.
 */
export interface ArgumentWidget {
  /** Semantic type / widget id this renders for. Either a host id
   *  (`host.<vendor>.<name>`) or a **base type** (`bool` | `int` | `float` |
   *  `string`) — registering against a base type gives every param of that
   *  primitive type the host's control (e.g. a `bool` toggle), with no brink
   *  built-in opinion (argument-widget-spec §3.1, #990). Matched against a
   *  slot's declared `widget` kind first, falling back to its `type_name`
   *  (base or semantic) — see `matchHostWidget`. */
  type: string;
  /** Optional inline label data — the studio draws the chip from it. */
  inline?(ctx: ArgumentWidgetContext): { text: string; className?: string };
  /** The editor — the only host-rendered surface. Mount the body into
   *  `container`, resolve/cancel through `host`, and return a teardown. */
  editor: {
    /** `"popover"` (default) or `"modal"` for a rich picker anchored/overlaid
     *  on the call site; `"inline"` mounts the control directly in the Form
     *  row where a text field would sit (argument-widget-spec §3.1, #990) —
     *  the right shape for a primitive (a bool toggle, a number stepper).
     *  Only `buildField`'s Form honors `"inline"`; the in-editor call/Edit/
     *  Fill affordances have no row to mount into and fall back to popover. */
    surface?: "popover" | "modal" | "inline";
    render(
      ctx: ArgumentWidgetContext,
      host: ArgumentWidgetEditorHost,
      container: HTMLElement,
    ): () => void;
  };
}

export interface SignatureInfo {
  label: string;
  documentation?: string;
  parameters: { label: string }[];
  active_parameter: number;
}

/** A fold's kind (#365): `"structural"` is everything the editor emitted
 *  before #365 (decls, doc comments, conditionals, sequences, choice sets) —
 *  user-invoked in every mode, never auto-collapsed. `"machinery"` and
 *  `"narrative"` are run-based folds (>=2 consecutive same-nature lines)
 *  computed from the line classification (base, or a registered dialect's
 *  declared `nature`). */
export type FoldKind = "structural" | "machinery" | "narrative";

export interface FoldRange {
  start_line: number;
  end_line: number;
  collapsed_text?: string;
  /** Whole-line declaration fold (docs + header + body): fold from the start
   *  of start_line and render the hidden header as the placeholder. */
  from_line_start?: boolean;
  /** The fold's kind (#365). */
  kind: FoldKind;
}

export interface DocumentSymbol {
  name: string;
  kind: string;
  detail?: string;
  start: number;
  end: number;
  /** Start of the full symbol body (including header through end of content). */
  full_start: number;
  /** End of the full symbol body. */
  full_end: number;
  children: DocumentSymbol[];
}

/**
 * Self-describing, internally-tagged payload identifying which transformation a
 * code action performs. The `action` field is the discriminator; the remaining
 * fields depend on it (e.g. `FormatStitch` carries `knot` and `stitch`). Pass
 * the whole object back to `resolveCodeAction` to apply the action — the caller
 * never reconstructs it from the cursor position.
 */
export interface CodeActionData {
  action: string;
  [key: string]: unknown;
}

export interface CodeAction {
  title: string;
  kind: string;
  /** Opaque, self-describing payload — feed back to `resolveCodeAction`. */
  data: CodeActionData;
}

// ── Document handles (multi-document EditorSession) ─────────────

/**
 * Opaque document-handle id returned by `EditorSession.openDocument` /
 * `openFragment`. At the wasm boundary `0` is the "file not loaded"
 * sentinel and never a valid handle.
 */
export type DocumentId = number;

/**
 * What an `updateDocument` call actually changed in the underlying file,
 * in UTF-16 **file** coordinates. `[start, end)` is the replaced range of
 * the file's previous content. The inserted text is the `source` argument
 * the caller already has — unless `text` is present, in which case a
 * fragment splice appended a `\n` separator and `text` carries the
 * actually-inserted text (`source` + `"\n"`). Sibling editor views of the
 * same file can apply this directly as a CM6 change spec.
 */
export interface DocumentChangeSpec {
  path: string;
  start: number;
  end: number;
  text?: string;
}

// ── Structural move types ───────────────────────────────────────

export interface CrossFileEdit {
  /** Path of the file whose content is replaced. */
  path: string;
  /** The file's full source after applying its reference edits. */
  new_source: string;
}

/** One entry in a structural op's breakage report — a diagnostic the op would
 * introduce. Locations are 1-based, matching the editor's status surfaces. */
export interface RenameDiagnostic {
  severity: "error" | "warning" | "info" | "hint";
  /** Stable diagnostic code, e.g. `E022`. */
  code: string;
  message: string;
  /** Project-relative path of the file the diagnostic lands in. */
  path: string;
  /** 1-based line of the diagnostic's start. */
  line: number;
  /** 1-based column of the diagnostic's start. */
  col: number;
}

/** The unified result of every mutating structural op (#316): rename, move,
 * promote, demote, reorder, file-rename, and delete. `new_source` is the
 * rewritten primary file; `cross_file_edits` carry the referencing files'
 * rewrites. `safe` is true when the op introduces no new diagnostics; otherwise
 * `introduced_diagnostics` holds the breakage report and the edits apply only on
 * an explicit force. Reorders are trivially safe (empty breakage). */
export interface StructuralResult {
  /** True when the op actually happened. `false` means the op was refused
   *  and no write occurred — check this before `safe`: a refusal reports
   *  `safe: true` with no `introduced_diagnostics`, which reads exactly like
   *  a clean success (`docs/studio-shell-spec.md` §7.5). */
  ok: boolean;
  /** The file path this result applies to. */
  path?: string;
  new_source?: string;
  cross_file_edits: CrossFileEdit[];
  /** Diagnostics present after the op but not before. Empty ⇒ `safe`. */
  introduced_diagnostics: RenameDiagnostic[];
  /** True when the op introduces no new diagnostics. */
  safe: boolean;
  error?: string;
}

/** One file relocated by an atomic directory rename/move (#314). The caller
 * writes `new_source` at `new_path` and removes `old_path`. `new_source` already
 * carries the file's own outbound-include rewrites. */
export interface MovedFile {
  /** The file's project-relative path before the move. */
  old_path: string;
  /** The file's project-relative path after the move. */
  new_path: string;
  /** The moved file's full source with its relative includes rewritten. */
  new_source: string;
}

/** The result of an atomic directory rename/move (#314) — the multi-file analog
 * of {@link StructuralResult}. Every affected `INCLUDE` is rewritten against one
 * pre-move snapshot, so moved files' outbound includes, inbound includes from
 * outside the folder, and intra-folder sibling includes stay mutually
 * consistent. `moved_files` are the relocated files; `cross_file_edits` carry
 * the outside referrers' rewrites (full new source, path-keyed). `safe` is true
 * when the move introduces no new diagnostics; otherwise `introduced_diagnostics`
 * holds the breakage report and the edits apply only on an explicit force. */
export interface DirMoveResult {
  ok: boolean;
  /** Every file relocated by the move. */
  moved_files: MovedFile[];
  /** Reference edits in files outside the moved directory (full new source). */
  cross_file_edits: CrossFileEdit[];
  /** Diagnostics present after the move but not before. Empty ⇒ `safe`. */
  introduced_diagnostics: RenameDiagnostic[];
  /** True when the move introduces no new diagnostics. */
  safe: boolean;
  error?: string;
}

// ── Multi-file project types ────────────────────────────────────

/**
 * `mounted` (issue #2306/#2343, "Mounted stdlib presents as a read-only
 * library node"): true when `path` currently resolves to a mounted stdlib
 * copy rather than a file the project scan found or the user created.
 * `list_files`/`project_outline`/`story_graph` used to exclude these paths
 * entirely (#2231); they now list them with this flag instead, so the
 * Binder can render a distinct, collapsed, read-only "Library" section.
 * Also read-only — see `EditorSessionHandle.isReadOnly`. Optional (like the
 * other situational fields on `StoryGraphNode` below) rather than required:
 * the real wasm/mock always sends it, but a hand-written test fixture that
 * doesn't care about the Library section shouldn't have to populate it —
 * every consumer treats an absent flag the same as `false` (`!f.mounted`).
 */
export interface ProjectFile {
  path: string;
  mounted?: boolean;
}

export interface FileOutline {
  path: string;
  symbols: DocumentSymbol[];
  /** See {@link ProjectFile.mounted} — same flag, same issue. */
  mounted?: boolean;
}

// ── Story graph types (studio-shell spec §4.1) ──────────────────

export type StoryGraphNodeKind = "knot" | "stitch" | "end" | "done";

export type StoryGraphEdgeKind = "divert" | "choice" | "tunnel" | "thread";

/**
 * A story-graph node: a knot, a stitch, or an `END`/`DONE` pseudo-node.
 * `file`/`start`/`end` are absent on pseudo-nodes; `start`/`end` are UTF-16
 * offsets of the declaration name within `file`.
 */
export interface StoryGraphNode {
  /** Stable id — the qualified name (`knot`, `knot.stitch`), or `END`/`DONE`. */
  id: string;
  /** The qualified display name (currently identical to `id`). */
  name: string;
  kind: StoryGraphNodeKind;
  file?: string;
  start?: number;
  end?: number;
  /** For stitches: the owning knot's node id (the UI nests them). */
  parent?: string;
  /** See {@link ProjectFile.mounted} — same flag, same issue (#2306/#2343).
   *  Always `false` for the `END`/`DONE` pseudo-nodes (no owning file). */
  mounted?: boolean;
}

/**
 * A source site that produced a story-graph edge: the divert target path's
 * span, or the whole divert statement for `-> DONE`/`-> END`. `start`/`end`
 * are UTF-16 offsets within `file` — the same convention as node spans.
 */
export interface StoryGraphEdgeOccurrence {
  file: string;
  start: number;
  end: number;
}

/** A directed story-graph edge between node ids. Function calls are excluded. */
export interface StoryGraphEdge {
  from: string;
  to: string;
  kind: StoryGraphEdgeKind;
  /**
   * The divert sites that produced this edge, sorted by (file, span). An
   * aggregated edge (e.g. two choices targeting the same knot) keeps one
   * entry per site. Absent when empty — only for HIR-synthesized diverts
   * with no source anchor.
   */
  occurrences?: StoryGraphEdgeOccurrence[];
}

/**
 * The whole-project story graph. Deterministically ordered: nodes sorted by
 * id, edges deduplicated and sorted by (from, to, kind). Recomputed per call,
 * like the project outline.
 */
export interface StoryGraph {
  nodes: StoryGraphNode[];
  edges: StoryGraphEdge[];
}

// ── Line conversion types ───────────────────────────────────────

export type ConvertTarget = "narrative" | "choice" | "sticky_choice" | "gather" | "choice_body";

export interface TextEdit {
  from: number;
  to: number;
  insert: string;
}

/**
 * Result of an auto-import (#312 F): whether `target` was already reachable
 * from the current file's INCLUDE graph and, when not, the `INCLUDE`-insertion
 * `edit` (whole-file UTF-16 coords) to apply to the current file's source.
 * Idempotent — `already_reachable` ⇒ no `edit`.
 */
export interface AutoImportResult {
  ok: boolean;
  already_reachable: boolean;
  edit?: TextEdit | null;
  error?: string | null;
}

// ── Include info types ──────────────────────────────────────────

export interface IncludeInfo {
  path: string;
  resolved: string;
  loaded: boolean;
}

// ── Line context types (from brink-ide) ─────────────────────────

export type LineElement =
  | "knot_header"
  | "stitch_header"
  | "narrative"
  | "choice"
  | "gather"
  | "divert"
  | "logic"
  | "var_decl"
  | "comment"
  | "todo"
  | "include"
  | "external"
  | "tag"
  | "blank";

export interface WeavePosition {
  depth: number;
  element: WeaveElement;
}

export type WeaveElement =
  | "top_level"
  | { choice_line: { sticky: boolean } }
  | "choice_body"
  | "gather_continuation"
  | "conditional_branch"
  | "sequence_branch";

export interface LineContext {
  element: LineElement;
  weave: WeavePosition;
  has_tags: boolean;
  /** For a divert line: standalone (`-> x`, `-> END`) vs a tunnel call or
   *  thread start (#480). Structural fact — never re-sniff the text. */
  standalone: boolean;
  /** Option identity (#480): the full lineage of zero-based option indices
   *  through the weave, present on choice-line / choice-body lines. */
  option_path?: number[];
  block_comment: boolean;
  /** Dialect classification for this line (#368), present only when a
   *  dialect is registered (`EditorSession.set_dialect`) and this line
   *  matched one of its declared kinds (directly or via a chain rule). */
  dialect?: DialectLineInfo | null;
}

/** Dialect-classification result for one line (#368) — computed once at
 *  classification time on the Rust side; the editor never re-derives it. */
export interface DialectLineInfo {
  /** The dialect kind (e.g. `"character"`, `"parenthetical"`, `"dialogue"`). */
  kind: string;
  /** Captured named-group attributes, sorted by name. For chained lines,
   *  carries the `chain.carry` groups forward from the run's originating
   *  match (whole-run `data-speaker`, etc). */
  attrs: [string, string][];
  /** Hidden geometry byte spans (full-line-relative, UTF-8 byte offsets). */
  hidden_spans: [number, number][];
  /** The editable content byte span (full-line-relative). `null`/absent for
   *  chain-only (pattern-less) kinds — content is the whole trimmed line. */
  content_span?: [number, number] | null;
}

// ── Debug snapshot (State View) ──────────────────────────────────

export interface DebugGlobal {
  name: string;
  value: string;
}

export interface DebugFrame {
  /** root | function | tunnel | thread | external | eval */
  kind: string;
  /** Nearest named knot/stitch for this frame, if resolvable. */
  location?: string;
  /** Precise `(container_idx, offset)` this frame will resume at (#3182).
   *  Absent for a frame whose container stack is empty — including every
   *  `external` frame, which carries no bytecode position. Resolve to
   *  source with `StoryRunnerHandle.resolveDebugPosition` (D9, #3187). */
  position?: { container_idx: number; offset: number };
  temps: number;
  /** D7 (`docs/debugger-spec.md` §3, #3185): this frame's named locals —
   *  every declared parameter/`~ temp` slot currently in scope, bound to
   *  its live value. Additive alongside `temps` (D4's bare count, kept
   *  as-is). Absent when the linked program carries no `DebugInfo` at all
   *  (a release-exported story, or one compiled before D6) — a frame with
   *  `DebugInfo` but genuinely zero declared locals reports `[]`, not
   *  absent, so a consumer can tell the two apart. */
  locals?: DebugLocal[];
}

export interface DebugLocal {
  /** The VM temp slot this local occupies — matches
   *  `DeclareTemp`/`GetTemp`/`SetTemp`'s `u16` operand. */
  slot: number;
  name: string;
  value: DebugValue;
}

/** A structured, read-only view of a runtime value for the debugger's
 *  locals panel (`docs/debugger-spec.md` §3, D7/#3185). Deliberately more
 *  structured than `DebugGlobal.value` (a display string, unchanged): a
 *  locals panel needs to tell "a list with these members" from "a string
 *  that reads like a list", expand a struct's fields, etc. Covers every
 *  kind the runtime distinguishes that the issue calls out by name (int,
 *  float, string, list, divert target, struct, handle) plus `bool`/`null`;
 *  every other kind (closures, arrays, maps, weighted tables, fn refs,
 *  pointers, vector/matrix/quaternion, ranges, options, projections) falls
 *  back to `other`'s display string, the same form `DebugGlobal.value`
 *  already uses. */
export type DebugValue =
  | { type: "int"; value: number }
  | { type: "float"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "string"; value: string }
  | { type: "null" }
  | { type: "list"; members: string[] }
  | { type: "divertTarget"; path?: string }
  | { type: "struct"; name?: string; fields: DebugField[] }
  /** `id` is a decimal string, not `number` — a full-range host token id
   *  would silently lose precision above 2^53 as a JS number. */
  | { type: "handle"; kind: string; id: string }
  | { type: "other"; display: string };

export interface DebugField {
  name: string;
  value: DebugValue;
}

export interface DebugVisit {
  path: string;
  count: number;
}

/** Id-keyed visit count (W11/#3304): EVERY container — anonymous
 *  choice/gather bodies included, which `visit_counts` (path-resolved)
 *  drops. `def_id` is the `DefinitionId` display form, string-equal to
 *  the HIR overlay projection's `HirSpan.def_id` for the same container
 *  (#3234's identity join). */
export interface DebugVisitId {
  def_id: string;
  count: number;
}

export interface DebugChoice {
  text: string;
  target?: string;
  /** The choice's own container id (`DefinitionId` display form) —
   *  string-equal to the overlay projection's `def_id` for the choice
   *  span (W11/#3304): the presented-choice ↔ source join. Always sent
   *  by the wasm; optional for older fixtures/hosts. */
  def_id?: string;
  /**
   * The raw `pending_choices` index — the same pre-filter position the
   * live `Choice.index` carries and that `choose()` expects. Not a
   * post-filter enumeration position: invisible-default choices are
   * filtered out of what's shown but still occupy a slot, so this can
   * skip values (e.g. 0, 2, 3 if index 1 was an invisible default).
   */
  index: number;
}

export interface DebugRng {
  seed: number;
  previous: number;
}

/** A read-only, name-resolved snapshot of the runtime's current state. */
export interface DebugState {
  /** active | waiting_for_choice | done | ended */
  status: string;
  current_location?: string;
  /** Precise `(container_idx, offset)` for the active flow (#3182); mirrors
   *  `call_stack[0].position` when the call stack is non-empty. Resolve to
   *  source with `StoryRunnerHandle.resolveDebugPosition` (D9, #3187). */
  position?: { container_idx: number; offset: number };
  turn_index: number;
  globals: DebugGlobal[];
  call_stack: DebugFrame[];
  visit_counts: DebugVisit[];
  /** See {@link DebugVisitId} (W11/#3304). Always sent by the wasm;
   *  optional for older fixtures/hosts. */
  visit_ids?: DebugVisitId[];
  pending_choices: DebugChoice[];
  rng: DebugRng;
}

/**
 * The program→source resolver's result (D9, #3187) —
 * `StoryRunnerHandle.resolveDebugPosition`'s return shape, mirroring
 * `brink_runtime::DebugSourceLocation`. `file: null` marks the reserved
 * synthetic sentinel (compiler-generated content with no author source),
 * distinct from the whole value being `null` ("doesn't resolve at all" —
 * no `DebugInfo` section, or an out-of-range position).
 */
export interface DebugSourceLocation {
  file: string | null;
  range_start: number;
  range_len: number;
}

/**
 * A bytecode position — the source→program resolvers' answer (W2/#3295:
 * `resolve_source_range`/`resolve_source_line`/`resolve_path_address`) and
 * the same `(container_idx, offset)` currency `DebugState.position`, the
 * breakpoint APIs, and `resolveDebugPosition` already use. The whole value
 * is `null` when the lookup doesn't resolve (no `DebugInfo` section, no
 * executable code on the span/line, unknown path).
 */
export interface ProgramAddress {
  container_idx: number;
  offset: number;
}

// ── Debug control (D8, #3186 — the wasm control-half bridge, #3232) ──
//
// Mirrors `brink_runtime::{Breakpoint, DebugRunOutcome, DebugStopReason,
// StepMode}` — the wire shapes `debugBreakpoints`/`debugRun`/`debugStep`
// (`StoryRunnerHandle`/`StorySessionHandle`, `@brink-lang/web`) exchange.

/** A step's DIRECTION, orthogonal to its granularity (#3264): the same
 *  three modes apply to instruction stepping (`debugStep`) and line
 *  stepping alike. Depth-delta semantics per `docs/debugger-spec.md` §4:
 *  `"into"` executes exactly one instruction, descending into any newly
 *  entered frame; `"over"` runs through a call without stopping inside it;
 *  `"out"` runs until the current frame returns to its caller. */
export type StepMode = "into" | "over" | "out";

/** A bytecode position resolved to author-facing source at BOTH
 *  granularities the debugger serves (W6/#3299): the 0-based `line` (the
 *  author tier's highlight band and paused chip) and the covering debug
 *  entry's exact byte range — carried so finer-than-line consumers need
 *  no new seam (expression-level entries, instruction stepping, and
 *  step-out's mid-line call-site landing, which exists today). Offsets
 *  are UTF-8 BYTE offsets in the file as the compiler consumed it —
 *  convert before using as UTF-16 editor positions. */
export interface DebugLine {
  file: string;
  line: number;
  range_start: number;
  range_len: number;
}

/** One transcript line a debug advance emitted (W5/#3298) — the delta the
 *  call appended to the story transcript, so lines produced while stepping
 *  reach the Player instead of vanishing. */
export interface DebugOutputLine {
  text: string;
  tags: string[];
  /** Where the line came from in the author's source (W7/#3300
   *  transcript provenance) — absent when the line's table entry
   *  carries no location. */
  source?: SourceLocation;
}

/** Mirrors `brink_format::SourceLocation` (W7/#3300): a delivered
 *  line's origin. `range_start`/`range_end` are UTF-8 BYTE offsets in
 *  the file as the compiler consumed it — convert before using as
 *  editor (UTF-16) positions. */
export interface SourceLocation {
  file: string;
  range_start: number;
  range_end: number;
}

/** One persisted structural-transcript part (RULED 2026-08-30, "Studio
 *  saves carry the structural transcript"): the wire mirror of
 *  `brink-web`'s `transcript_json::PartJson`. `line` is a deferred
 *  line-table reference — `container`/`line` index the compile the
 *  transcript is RENDERED against, not save-time text; `slots` are
 *  runtime `Value`s in their serde-JSON shape (opaque here). */
export type TranscriptPart =
  | { part: "text"; text: string }
  | { part: "line"; container: number; line: number; slots?: unknown[]; flags?: number }
  | { part: "value"; value: unknown }
  | { part: "newline" }
  | { part: "spring" }
  | { part: "glue" }
  | { part: "tag"; tag: string };

/** The structural-transcript envelope `exportTranscript` returns and
 *  `renderTranscript` consumes — human-readable JSON (the `.brkt`
 *  content model; binary stays the shipping-game format). `checksum` is
 *  the exporting compile's CRC-32, advisory only: rendering against a
 *  DIFFERENT compile is the point (edit → reload re-renders). */
export interface StructuralTranscript {
  version: number;
  checksum: number;
  parts: TranscriptPart[];
  fragments?: { parts: TranscriptPart[]; tags?: string[] }[];
}

/** One line of a re-rendered structural transcript: resolved against the
 *  rendering session's CURRENT program/line tables, provenance included. */
export interface RenderedTranscriptLine {
  text: string;
  tags: string[];
  source?: SourceLocation;
}

/** One breakpoint: an unconditional halt at a `(container_idx, offset)`
 *  bytecode position, checked before that instruction executes. `id`
 *  addresses it for `debugBreakpointRemove`/`debugBreakpointSetEnabled`. */
export interface Breakpoint {
  id: number;
  container_idx: number;
  offset: number;
  name: string;
  enabled: boolean;
}

/** Why a `debugRun`/`debugStep` call stopped — internally tagged on `type`,
 *  the same convention `DebugValue` above uses. */
export type DebugStopReason =
  | { type: "breakpoint"; id: number; name: string }
  | {
      type: "watchpoint";
      global_idx: number;
      /** The watched global's author name (W18/#3311) — the chip's
       *  "paused on write" label. */
      name?: string;
    }
  /** A choice point was reached — distinct from `"terminal"`: `choose()`
   *  followed by `continueSingle`/`debugRun`/`debugStep` can resume from
   *  here, unlike an actual `-> DONE`/`-> END`. */
  | { type: "choices" }
  /** The requested step (into/over/out) completed normally. */
  | { type: "step" }
  /** The flow reached a terminal VM outcome (`-> DONE`/`-> END`, or content
   *  otherwise exhausted) before the requested stop condition was reached. */
  | { type: "terminal" }
  /** `StepMode: "out"` was requested from the outermost frame (or a thread
   *  frame), which has no caller to return to — refused rather than running
   *  the story to its own end. */
  | { type: "noStepOutTarget" }
  /** A LINE-granular step was requested (#3264) on an artifact that cannot
   *  say which line execution is on — no debug info, or a file compiled
   *  without source text so it carries no line index. Reported rather than
   *  quietly behaving like instruction stepping, which would turn a missing
   *  line index into "why does step take four presses" instead of a legible
   *  "this build has no line info". */
  | { type: "noLineInfo" }
  /** A bound external's handler deferred (#3224) — the `External` frame is
   *  intact; resolve out-of-band (`resolveExternal`), then resume with any
   *  debug verb. */
  | { type: "awaitingExternal" };

/** The result of a `debugRun`/`debugStep` call: why it stopped, the
 *  resulting position (absent for a frame with an empty container stack,
 *  e.g. after a terminal step), and the resulting call-stack depth. */
export interface DebugRunOutcome {
  reason: DebugStopReason;
  position?: { container_idx: number; offset: number };
  depth: number;
  /** The transcript delta this call produced (W5/#3298); empty when the
   *  stop emitted nothing (a code step, an immediate breakpoint). */
  lines: DebugOutputLine[];
}

// ── Program model (Program Explorer) ─────────────────────────────

export interface ProgramGlobal {
  name: string;
  ty: string;
  default: string;
  mutable: boolean;
}

export interface ProgramListItem {
  name: string;
  ordinal: number;
}

export interface ProgramList {
  name: string;
  items: ProgramListItem[];
}

export interface ProgramExternal {
  name: string;
  arg_count: number;
  fallback?: string;
}

/**
 * One decoded bytecode instruction, with the byte offset it decoded from
 * (D9, #3187) — the key a live `DebugPosition`/`DebugFrame.position`
 * (D4, #3182) is matched against for a "current instruction" highlight in
 * the Program Explorer.
 */
export interface DisasmLine {
  offset: number;
  text: string;
  /** Source provenance from the DebugInfo section (#3339) — absent when
   *  the compile carried no debug info. Byte offsets. */
  src?: { file: string; start: number; end: number };
}

/** A knot or stitch in the compiled-program tree. */
/** An anonymous child container (gather, choice target, sequence
 *  wrapper), listed under its owning scope in table order and labeled with
 *  the save stamps' `c-N` spelling (#3339 Disassembly view). */
export interface AnonContainer {
  /** The container's real leaf name when it has one (a weave label —
   *  `enter_container barter.opts` finds a row called `opts`), else the
   *  stamps' `c-N` spelling counting unnamed containers only. */
  label: string;
  container_idx: number;
  byte_size: number;
  disasm: DisasmLine[];
}

export interface KnotNode {
  path: string;
  name: string;
  /** "knot" | "stitch" */
  kind: string;
  /** Counting flags: "visits" | "turns" | "start_only" */
  flags: string[];
  path_hash: number;
  /**
   * This container's index in the compiled program's container table — the
   * same `container_idx` a runtime `DebugPosition` addresses. `0xffffffff`
   * (`u32::MAX`) for a synthesized knot node with no backing container
   * (rare — a knot with stitches but no own scope container).
   */
  container_idx: number;
  /** Total bytecode bytes of this scope — the scope container plus its
   *  anonymous children (gathers, choice targets), which are not tree
   *  nodes and are otherwise invisible to size accounting (#3339). */
  byte_size: number;
  /** Containers in the scope, anonymous children included. */
  container_count: number;
  /** This scope's anonymous child containers, in table order. */
  anon: AnonContainer[];
  /** Resolved bytecode disassembly, one instruction per entry. */
  disasm: DisasmLine[];
  children: KnotNode[];
}

/** Structured view of the statically compiled program. */
export interface ProgramModel {
  checksum: string;
  /** Whether this compile carried a DebugInfo section — "no provenance on
   *  these rows" vs "provenance is off". */
  debug_info: boolean;
  globals: ProgramGlobal[];
  lists: ProgramList[];
  externals: ProgramExternal[];
  knots: KnotNode[];
}

// ── Lines table (#366, host-side analysis) ───────────────────────
//
// Mirrors `brink_intl::json_model` (the `export-xliff` `lines.json` shape,
// reused verbatim — see `StoryRunner.lines_table`/`docs/dialect-spec.md`).
// Project-wide: one entry per compiled scope (root/knot/stitch), INCLUDEs
// already resolved by the compile. Each line carries its text (plain or a
// slot/select template) and, when known, its source span (file + byte
// range in that file). The compiler's answer for host-side analyses that
// need to walk emitted lines project-wide — cast detection (#366), per-
// speaker word counts, the #362 line-fit metrics epic.

/** A single named-group slot in a template line. */
export interface LineSlot {
  index: number;
  name: string;
}

/** Where a line came from in the original `.ink` project. */
export interface LineSource {
  file: string;
  range_start: number;
  range_end: number;
}

/** One part of a template line's content. */
export type LinePart =
  | { slot: number }
  | { select: LineSelect }
  | { span: LineSpan }
  | string;

/** A plural/keyword select over a slot value. Each `variants` entry is a
 *  one-key object (`{ "cardinal:One": "..." }`, `{ "=0": "..." }`, etc.) —
 *  mirrors the Rust `format_select_key` tagging exactly. */
export interface LineSelect {
  slot: number;
  variants: Record<string, string>[];
  default: string;
}

/** One `name="value"` attribute on a {@link LineSpan}. */
export interface LineAttr {
  name: string;
  value: string;
}

/** An inline markup span (#1716, `docs/prose-dialect-spec.md` §4.4) —
 *  mirrors the Rust `SpanJson` field-for-field. `attrs`/`children` are
 *  omitted entirely (not just empty) when there are none, matching the
 *  Rust side's `skip_serializing_if = "Vec::is_empty"`. */
export interface LineSpan {
  name: string;
  attrs?: LineAttr[];
  children?: LinePart[];
}

/** A line's content — a plain string, or a template (literal/slot/select parts). */
export type LineContent = string | { template: LinePart[] };

/** One compiled line entry. */
export interface LinesTableLine {
  index: number;
  content?: LineContent;
  /** 16-hex-digit source-identity hash of this line's content. */
  hash: string;
  audio?: string | null;
  slots?: LineSlot[];
  source?: LineSource | null;
}

/** One scope (root, knot, or stitch) in the lines table. */
export interface LinesTableScope {
  /** The scope's qualified name (e.g. a knot or stitch name), when known. */
  name?: string | null;
  /** The scope's `DefinitionId`, formatted `0x{16 hex digits}`. */
  id: string;
  lines: LinesTableLine[];
}

/** Where the `.inkb` bytes go (#3339 Size view) — real on-disk sizes
 *  from the file's own offset table. `shipping` is an exact
 *  re-serialization without the DebugInfo section. */
export interface SizeReport {
  total: number;
  shipping: number;
  debug: number;
  header: number;
  sections: { kind: string; bytes: number }[];
  /** Per-scope line-table bytes; `name` null for the root scope. */
  line_scopes: { name: string | null; bytes: number }[];
}

/** The compiler's lines table (`StoryRunner.linesTable()`, #366):
 *  project-wide, `INCLUDE`s already resolved by the compile. */
export interface LinesTable {
  version: number;
  source_checksum: string;
  scopes: LinesTableScope[];
}

// ── Host capability manifest (tooling / author-time) ────────────
//
// Mirrors `brink_ir::host_manifest`. Authored by the host and passed as JSON
// to `EditorSession.setHostManifest`. Describes the host's external-function
// vocabulary for author-time validation and richer hover/completion. Never
// affects the runtime or codegen. See docs/host-capability-manifest.md.

/** A base type keyword, or the name of a registered semantic type. */
export type TypeRef = string;

/**
 * The underlying base types at an external boundary. `"handle"` (T1d-2,
 * docs/t1d-spec.md §3) is a host-resource handle kind — the semantic
 * type's own `name` field *is* the declared kind name (e.g.
 * `"AudioInstance"`), not a specialization label like the other bases.
 */
export type BaseType = "string" | "int" | "float" | "bool" | "void" | "handle";

/** Presentation/effect category of an external (informational). */
export type ExternalKind = "query" | "effect" | "presentation" | "plain";

/** A closed-domain constraint, checkable against literal arguments. */
export type Constraint =
  | { kind: "enum"; values: string[] }
  | { kind: "regex"; pattern: string }
  | { kind: "range"; min?: number | null; max?: number | null };

/** One pickable value with its host-given display label (Tier 3, #174). */
export interface ValueItem {
  /** The literal inserted into source (e.g. "5"). */
  value: string;
  /** The display label (e.g. "HarborGate"). */
  label: string;
  /** Optional secondary text (e.g. "Switch #5"). */
  detail?: string | null;
}

/**
 * Where a semantic type's pickable values come from (Tier 3, #174). Advisory
 * tooling metadata only — it drives the author-time argument picker and never
 * affects checking or the compiled program. See docs/host-argument-picker-spec.md.
 */
export type ValueSource =
  | { source: "static"; items: ValueItem[] }
  | { source: "host" };

/** A studio-builtin argument widget for a semantic type (Tier 3). */
export interface WidgetDecl {
  /** The built-in widget kind, e.g. `"color"`. */
  kind: string;
}

/** A flat-nominal semantic type: a base type plus one optional constraint. */
export interface SemanticTypeDef {
  name: string;
  base: BaseType;
  constraint?: Constraint | null;
  /** The picker's value source (Tier 3) — `static` (no host) or `host`. */
  values?: ValueSource | null;
  /** A studio-builtin argument widget (Tier 3) — e.g. `{ kind: "color" }`. */
  widget?: WidgetDecl | null;
}

/** A registered external parameter. */
export interface ManifestParam {
  name: string;
  ty?: TypeRef;
}

/** An arg-group widget on an external (argument-widget spec §2): one widget over
 *  several params, with an editor surface + optional inter-arg context. */
export interface ArgGroupWidget {
  /** Argument indices the widget spans, e.g. `[0, 1]`. */
  group: number[];
  /** Semantic type / widget id (matches a host `ArgumentWidget.type`). */
  type: string;
  /** Editor container — `"popover"` (default) or `"modal"`. */
  surface?: "popover" | "modal";
  /** Inter-arg context: key → the sibling arg index supplying it, e.g. `{ map: 1 }`. */
  context?: Record<string, number>;
}

/** A registered external-function signature. */
export interface ManifestExternal {
  name: string;
  params?: ManifestParam[];
  returns?: TypeRef;
  kind?: ExternalKind;
  doc?: string | null;
  /** Arg-group widgets (argument-widget spec §2). */
  widgets?: ArgGroupWidget[];
  /** Category breadcrumb for the Host Functions panel (#210), e.g.
   *  ["Map", "Movement"] → nested collapsible sections. Empty/absent = ungrouped. */
  path?: string[];
}

/**
 * One attribute a {@link ManifestSpanKind} accepts (docs/prose-dialect-spec.md
 * §4.2, issue #1780 gap 1, ruled by issue #1997).
 *
 * Widens the old bare attribute-name-string shape into a record so
 * `required` has somewhere to live, and so a future attribute-value type has
 * somewhere to land later without another wire-format break (issue #1780
 * gap 2) — **schema headroom only: attribute-value typing is NOT
 * implemented**. Span attribute values stay static text by construction, so
 * nothing parses, resolves, or checks a value against a type today.
 */
export interface ManifestSpanAttr {
  /** The attribute name, e.g. "amount" for `<wave amount="3">`. */
  name: string;
  /** Whether a span of this kind must carry this attribute (`E173`).
   *  Defaults to `false` (optional) when absent. */
  required?: boolean;
}

/**
 * One declared inline-markup span kind (docs/prose-dialect-spec.md §4.2,
 * issue #1733; required attributes, issue #1780/#1997). A tag name plus the
 * attributes that tag accepts. Attribute *values* are static text by
 * construction, so they are never type-checked — only the attribute name
 * (and now whether it is required) is vocabulary.
 */
export interface ManifestSpanKind {
  /** The tag name as written in source, e.g. "wave" for `<wave>…</wave>`. */
  name: string;
  /** Attributes this kind accepts, e.g. `[{ name: "amount" }]` for
   *  `<wave amount="3">`. Empty/absent = the kind takes no attributes.
   *
   *  Issue #1997 widened this from `string[]` to `ManifestSpanAttr[]` — a
   *  bare attribute-name array is no longer accepted; migrate `"amount"` to
   *  `{ "name": "amount" }`. */
  attrs?: ManifestSpanAttr[];
}

/** The host-owned, project-wide external vocabulary. */
export interface HostManifest {
  externals?: ManifestExternal[];
  types?: SemanticTypeDef[];
  /**
   * The host's inline markup vocabulary (docs/prose-dialect-spec.md §4.2).
   * Host-authored and co-located with `externals` by §3.4's authorship test
   * — a text-effect plugin can generate its tag declarations the way
   * bindings generate externals. Element conventions are project-authored
   * and live on a different surface; do not conflate them.
   *
   * **Empty/absent means freeform**, which is the default: markup passes
   * through unchecked unless at least one span kind is declared here — an
   * externals-only manifest never enables markup checking. Once declared,
   * an undeclared tag reports `E164`, an undeclared attribute on a declared
   * kind reports `E165`, and a declared kind's span omitting one of its
   * `required` attributes reports `E173`; all three are warnings by
   * default, so a host tightens them with `[lints] E164 = "deny"`.
   */
  markup?: ManifestSpanKind[];
}

// ── Dialogue dialect (#368, tooling / author-time) ──────────────
//
// Mirrors `brink_ir::dialect`. A versioned, pure-JSON schema describing a
// project's dialogue-line conventions (cues, parentheticals, dialogue
// chains) so the editor can classify and decorate lines without hardcoding
// any one convention. Authoring-time/tooling artifact only — never
// runtime-delivered (see docs/dialect-spec.md). Passed as JSON to
// `EditorSession.setDialect`.

/** The 3-way element nature (ruling: 3-way, not 2-way). */
export type ElementNature = "narrative" | "machinery" | "structural";

/** The general portable-regex pattern representation every interpreter
 *  (Rust, TS) executes directly.
 *
 *  Field names are **snake_case** on the wire — this type is serialized
 *  verbatim to/from the Rust `serde` struct (no `rename_all = "camelCase"`
 *  on the Rust side), and the same JSON file is consumed by both
 *  interpreters (the conformance corpus + `EditorSession.setDialect`). */
export interface PatternShape {
  /** Portable-regex pattern (JS `RegExp` ∩ Rust `regex` subset: named groups
   *  yes, lookaround/backreferences no), anchored `^...$` against the
   *  trimmed line. */
  pattern: string;
  /** Which named group is the editable content. Drives `content_span`
   *  geometry (markup/inline-decoration scoping) and classification
   *  `data-*` attrs — for a kind like `parenthetical` this legitimately
   *  includes wrapping punctuation that stays visible on the line. */
  content_group?: string | null;
  /** Which named group's captured value fills `template`'s placeholder for
   *  convert/strip round-trips. Defaults to `content_group` when absent —
   *  additive, byte-identical for every dialect that doesn't set it (#406).
   *  Exists because a kind can need a different answer to "what region is
   *  content" (`content_group`) than to "what value round-trips through
   *  `template`" (`template_group`) — e.g. `parenthetical`'s `content_group`
   *  is wrap-inclusive (parens stay part of the editable/markup-scoped
   *  region) while `template_group` names a nested bare-text group so the
   *  literal parens live in `template` itself, matching how every other
   *  convert/strip consumer treats "Parenthetical content" as bare text.
   *  Never emitted as a `data-*` attr and never hidden. */
  template_group?: string | null;
  /** Named groups whose matched span is hidden geometry. */
  hidden?: string[];
  /** Template string for insertion/conversion/format (e.g. `"@${speaker}:<>"`). */
  template: string;
}

/** Affix sugar: a content slot wrapped in literal prefix/suffix text —
 *  compiles mechanically to the pattern form (see `compileAffix` in the TS
 *  interpreter, mirroring `compile_affix` in Rust). Wire field names are
 *  snake_case (see {@link PatternShape}). */
export interface AffixShape {
  /** Literal prefix before the content (e.g. `"@"`). Hidden by construction. */
  prefix?: string | null;
  /** Literal suffix after the content (e.g. `":"`, or `":<>"` when glued). */
  suffix?: string | null;
  /** Whether the suffix's glue (`<>`) is appended and always hidden. */
  glued?: boolean;
  /** The semantic role of the content slot. Defaults to `"content"`. */
  content_role?: string;
}

/** How an element is recognized/produced in source text: either the general
 *  `pattern` form, or `affix` sugar (compiled to `pattern` before use). */
export type SourceShape = PatternShape | AffixShape;

/** The post-glue shape the runtime sees out of `continue_line()` output.
 *  Positionally constrained — non-reserved-prefix shapes (e.g. a
 *  parenthetical) peel only after a reserved-prefix segment (e.g. a cue).
 *  Wire field names are snake_case (see {@link PatternShape}). */
export interface EmittedShape {
  pattern: string;
  content_group?: string | null;
  reserved_prefix?: boolean;
}

/** A near-miss diagnostic: a pattern that almost matches a kind but doesn't
 *  quite, paired with a message and severity. */
export interface MalformedRule {
  pattern: string;
  message: string;
  severity?: string;
}

/** One declared element kind. */
export interface DialectElement {
  /** Open string taxonomy kind (e.g. `"character"`). CSS class derives as
   *  `brink-<kind>`. */
  kind: string;
  nature: ElementNature;
  /** Absent for chain-only kinds (e.g. `"dialogue"`, produced only by a
   *  chain rule). When absent, content is the whole trimmed line. */
  source?: SourceShape | null;
  emitted?: EmittedShape | null;
  malformed?: MalformedRule[];
}

/** "Narrative immediately after one of `after` becomes `becomes`." Blank
 *  lines always break the chain (not configurable in v1). */
export interface ChainRule {
  after: string[];
  /** Predecessor kinds must produce a line of this kind. Defaults to
   *  `["narrative"]`. */
  is?: string[];
  becomes: string;
  /** Named groups carried forward onto the whole chained run as `data-*`
   *  attributes (e.g. `["speaker"]` → `data-speaker`). */
  carry?: string[];
  /** The emitted-side run rule (#3388, RULED 2026-08-30): kinds whose
   *  appearance ENDS the active run in runtime-emitted text, plus the
   *  reserved `"choices"` turn boundary. A new triggering kind always
   *  starts a fresh run regardless. Applied through `runsOf`. */
  run_ends_at?: string[];
}

/** A transition action. Tagged so shapes are unambiguous. */
export type TransitionAction =
  | { action: "convert"; kind: string }
  | { action: "newline" }
  | { action: "strip" }
  | { action: "clear" }
  | { action: "trap" };

/** One Tab/Enter/Shift-Tab transition row, contributed by the dialect for a
 *  kind it declares — an overlay resolved before the built-in weave table.
 *  Wire field names are snake_case (see {@link PatternShape}). */
export interface TransitionRow {
  /** The kind this row applies to (must be declared, or reserved-structural). */
  on: string;
  key: string;
  has_content?: boolean | null;
  action: TransitionAction;
  /** Editor-facing hint text (status bar, etc). */
  hint?: string | null;
}

/** One picker/template entry for a declared kind. Wire field names are
 *  snake_case (see {@link PatternShape}). */
export interface TemplateEntry {
  kind: string;
  label: string;
  picker_key?: string | null;
  blank_tab?: boolean;
}

/** Editor-overlay template metadata (picker labels, blank-tab behavior). */
export interface Templates {
  entries?: TemplateEntry[];
}

/** A versioned, pure-JSON dialogue dialect. No functions, no `RegExp`
 *  objects — patterns are strings in the portable-regex subset. See
 *  docs/dialect-spec.md. */
export interface DialogueDialect {
  /** Schema version. Only `1` is defined. */
  version: number;
  /** Human-readable dialect name (e.g. `"at-cue"`). */
  name?: string;
  /** Element declarations, in classification precedence order. */
  elements?: DialectElement[];
  /** Chain rules: "narrative immediately after X becomes Y". */
  chain?: ChainRule[];
  /** Editor-overlay transition rows — never travels beyond tooling. */
  transitions?: TransitionRow[];
  /** Editor-overlay templates (picker key, blank-tab behavior, labels). */
  templates?: Templates;
}

// ── Speculative evaluation (F4.3, docs/speculative-eval-spec.md) ─────
//
// `WebSpeculation`'s wire shapes — a sandboxed, side-effect-proof fork of a
// running story (`StoryRunnerHandle.speculate()`), driven by its own
// composable verbs (`goToPath`/`advance`/`choose`/`evalFunction`/
// `resumeFunctionEval`/…). `TypedValue` is the richer sibling of
// `ExternalValue`: a binding argument only ever needs a scalar, but an
// `evalFunction` result is useful information worth keeping structured
// (a list's member names, a divert's destination) rather than collapsing
// to `null` the way the binding boundary does.

/** A structured ink value, richer than {@link ExternalValue} (which collapses
 * lists and divert targets to `null`). Returned by `SpeculationHandle`'s
 * `evalFunction`/`resumeFunctionEval` and the `evaluate()` convenience. */
export type TypedValue =
  | { type: "int"; value: number }
  | { type: "float"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "string"; value: string }
  | { type: "null" }
  | { type: "list"; items: ListMember[] }
  | { type: "divert"; path?: string };

/** One active member of a `"list"`-typed value. */
export interface ListMember {
  /** The origin list's declared name (e.g. `"Weekday"`). */
  origin: string;
  /** The item's unqualified display name (e.g. `"Monday"`). */
  name: string;
  /** The item's ordinal within its origin list. */
  ordinal: number;
}

/** Outcome of `SpeculationHandle.evalFunction`/`resumeFunctionEval`. */
export type SpeculationFunctionEval =
  | { type: "returned"; value: TypedValue }
  | { type: "awaiting_external"; name?: string };

/** Which evaluation regime a speculation's external-kind tiering gates for
 * (see `docs/speculative-eval-spec.md` §7 / `KindTieredHandler`). `"watch"`
 * (default): `effect`-kind externals never fire live. `"eval"`: `effect`
 * externals fire live only when `liveEffects` is also set. */
export type SpeculationContext = "watch" | "eval";

/** Per-external policy tiering a speculation gates its externals by. A name
 * absent from this map is conservatively treated as `"effect"`. */
export type SpeculationKinds = Record<string, "query" | "effect">;

/** Options for `StoryRunnerHandle.speculate()`. All fields optional. */
export interface SpeculationOptions {
  /** VM step budget for a single `advance()`, `evalFunction()`, or
   * `resumeFunctionEval()` call — each call gets its own fresh allowance.
   * Default 100,000. */
  steps?: number;
  /** Total visible-line budget across this speculation's lifetime. Default 1,000. */
  lines?: number;
  /** Default `"watch"`. */
  context?: SpeculationContext;
  /** Arm `effect`-kind externals; only takes effect under `context: "eval"`.
   * Default `false`. */
  liveEffects?: boolean;
  /** Per-external `"query"`/`"effect"` policy tiering. Default `{}` (every
   * external conservatively treated as `"effect"`). */
  kinds?: SpeculationKinds;
}

/** Which externals a speculation let through live versus fell back, across
 * every verb call made on it so far. Diagnostic only. */
export interface SpeculationExternalsReport {
  live: string[];
  fallback: string[];
}

/** One resolved transcript line from a speculation — `SpeculationHandle`'s
 * own shape (no `type` discriminant, unlike {@link SessionLine}: a
 * speculation's transcript is a plain resolved log, not a step outcome). */
export interface SpeculationLine {
  text: string;
  tags: string[];
}

/** Result of the thin `evaluate()` convenience — composes `SpeculationHandle`'s
 * verbs into a single call for the common cases (a knot path, or a function
 * call with literal arguments). */
export interface SpeculationResult {
  /** Present when `source` was a function call. */
  value?: TypedValue;
  /** Resolved `(text, tags)` lines produced by the run. */
  transcript: SpeculationLine[];
  /** Present when the run stopped at a choice point. */
  reachedChoices?: Choice[];
  /** An abort (`opts.signal`) rejects the `evaluate()` promise instead of
   * resolving with a stop value — there is no `"aborted"` variant here. */
  stop: "completed" | "choices" | "step-budget" | "line-budget";
  externals: SpeculationExternalsReport;
  /** Non-empty only when `source` failed to compile as either an expression
   * or content fragment (Tier-1, `docs/speculative-eval-spec.md`'s
   * "mechanism B"), or when it needed Tier-1 but no `opts.projectSource` was
   * supplied; `stop`/`transcript`/`value` are meaningless in that case. */
  diagnostics: string[];
}

/**
 * A project's ink source, supplied by the caller for Tier-1 speculative
 * evaluation (`StoryRunnerHandle.evaluate()`'s fragment path, F5.1): a
 * `StoryRunner` only ever holds an already-linked program, not the file set
 * it was compiled from, so evaluating an arbitrary author-typed fragment
 * (anything beyond a bare knot path or literal-arg call — Tier 0) needs the
 * consumer to hand back the same sources the running program was last
 * compiled from, so the fragment resolves against the live project's real
 * globals/knots/lists.
 */
export interface ProjectSource {
  /** The entry file's path — must be a key of `files`. The fragment's
   * synthetic knot/function is appended to this file's content before
   * recompiling; every other file is served verbatim. */
  entry: string;
  /** Every source file in the project, keyed by path exactly as its
   * `INCLUDE` directives name it.
   *
   * `brink.toml` is **not** implicitly included — this map becomes the
   * literal file set `compile_fragment` compiles over, and its `brink.toml`
   * discovery is a direct read probe of each `{ancestor}/brink.toml`
   * candidate walking up from `entry`'s directory (an O(depth) ancestor
   * probe — it never lists/enumerates this map). That means the key must
   * sit at `entry`'s own directory or one of its ancestors within this map
   * (e.g. entry `"src/main.ink"` with a key at `"src/chapters/brink.toml"`
   * is never on that ancestor chain and is silently ignored); a bare root
   * `"brink.toml"` key is always on the chain and always safe. Include the
   * right `"brink.toml"` key (with the project's real config text) if the
   * fragment compile should honor the project's `dialect`/`types`/`[lints]`
   * policy; omit it and the fragment compiles under
   * `AnalysisOptions::default()` instead — never an error, just the
   * unconfigured defaults, exactly as if no `brink.toml` existed. */
  files: Record<string, string>;
}

// ── Wasm-internal perf counters (measure-first ruling, 2026-08-24) ────

/** One internal counter row from `EditorSession::perf_counters_json`. */
export interface PerfCounterRow {
  count: number;
  totalMs: number;
  maxMs: number;
}

/**
 * The wasm-internal counters, keyed by phase name (`ide.analyze`,
 * `ide.compile`, `ide.byteToUtf16`, …; `ide.snapshotClone`/`ide.applyAnalysis`
 * retired with the off-db road, option A 2026-08-24). Mirrors the
 * JSON `crates/brink-web/src/perf.rs::report_json` emits.
 */
export type PerfCounters = Record<string, PerfCounterRow>;

// ── Editor session protocol wire envelopes (docs/editor-worker-spec.md §5) ──
//
// Hand-maintained mirrors; RUST IS THE SOURCE OF TRUTH (spec §5.4):
// crates/brink-web/src/protocol.rs. The golden wire strings pinning these
// shapes live in that module's tests and, verbatim, in
// packages/ink-editor/src/__tests__/worker-protocol.test.ts — change one
// side and the other's pin fails. Every payload is JSON-serializable by
// construction: no Map/Set, no binary views, no undefined-bearing shapes.

/** Client-assigned id correlating a query with its result/error. */
export type SessionRequestId = number;

/** Scheduling class (spec §6): interactive before background; only
 *  background queries coalesce or drop. */
export type SessionQueryPriority = "interactive" | "background";

/** A single text edit in UTF-16 document coordinates — the shape the
 *  delta-ingress endpoint (`applyEditsDocument`) accepts. */
export interface SessionEditSpan {
  from: number;
  to: number;
  insert: string;
}

/** A named mutation on the session's config or file surface. */
export interface SessionOp {
  method: string;
  args: unknown[];
}

/** Main-thread → session-host messages. The mutation stream
 *  (edit/push/config/files) is strictly ordered, applied FIFO before any
 *  query; queries are unordered beyond their priority class. */
export type SessionRequest =
  | { kind: "edit"; doc: DocumentId; docVersion: number; edits: SessionEditSpan[] }
  | { kind: "push"; doc: DocumentId; docVersion: number; source: string }
  | { kind: "config"; op: SessionOp }
  | { kind: "files"; op: SessionOp }
  | {
      kind: "query";
      id: SessionRequestId;
      priority: SessionQueryPriority;
      doc?: DocumentId;
      docVersion?: number;
      /** Background-only supersession handle (spec §6): a queued
       *  background query is dropped when a newer query with the SAME
       *  key sits behind it. Absent = never coalesces. Client-chosen —
       *  never derived from `method` alone, because same-method queries
       *  with different args (per-segment slices) are distinct work. */
      coalesceKey?: string;
      method: string;
      args: unknown[];
    }
  | { kind: "cancel"; id: SessionRequestId };

/** Session-host → main-thread messages. */
export type SessionResponse =
  | { kind: "ack"; doc: DocumentId; docVersion: number; applied: boolean }
  | {
      kind: "result";
      id: SessionRequestId;
      /** The doc's version at execution time (absent for doc-less
       *  queries). Staleness policy is the consumer's call (spec §5.3). */
      docVersion?: number;
      configEpoch: number;
      value: unknown;
    }
  | {
      kind: "error";
      id: SessionRequestId;
      /** Policy drops use the `dropped:` prefix (`dropped:superseded`,
       *  `dropped:stale`, `dropped:cancelled`) — distinguishable from
       *  faults. */
      message: string;
    }
  | { kind: "event"; event: unknown };
