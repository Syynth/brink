import { Facet, StateEffect, StateField, type EditorState, type Transaction } from "@codemirror/state";
import type { LineContext, WeaveElement } from "@brink/wasm-types";
import { documentHandleFacet } from "./document-handle.js";
import { AT_CUE_DIALECT, ResolvedDialect } from "./dialect.js";

export { type LineContext } from "@brink/wasm-types";

// ── Element kind — open string taxonomy (#368) ──────────────────────
//
// `ElementType` used to be a numeric TS `enum`. It is now a `const` object of
// kebab-case string kinds (CSS classes derive as `brink-<kind>`, so the
// scheme and the class taxonomy are the same string), with a derived union
// type. This keeps `ElementType.Character`-style call sites working
// mechanically (the const's values still compare equal to themselves and to
// `LineInfo.type`), while making the *type* an open string union — kinds a
// registered dialect declares (beyond the built-in reserved-structural set)
// flow through as plain strings that just aren't named on this object.
//
// BREAKING CHANGE (0.8.0, ruled 2026-07-05): the wire values changed from
// PascalCase enum member names (e.g. `"Character"`, `"NarrativeText"`) to
// kebab-case kind strings (e.g. `"character"`, `"narrative"`). See the
// PascalCase→kebab mapping table in docs/editor-consumer-guide.md and the
// changeset body.
export const ElementType = {
  KnotHeader: "knot-header",
  StitchHeader: "stitch-header",
  NarrativeText: "narrative",
  Choice: "choice",
  ChoiceBody: "choice-body",
  Gather: "gather",
  Divert: "divert",
  Logic: "logic",
  VarDecl: "var-decl",
  Comment: "comment",
  Todo: "todo",
  Include: "include",
  External: "external",
  Tag: "tag",
  Blank: "blank",
  Character: "character",
  Parenthetical: "parenthetical",
  Dialogue: "dialogue",
} as const;

/** The built-in kind strings. Open union: a dialect may classify a line as a
 *  kind not named here (its declared `kind`, e.g. a custom dialect's
 *  `"channel"`) — `LineInfo.type` is typed as plain `string` so those flow
 *  through without a cast. */
export type ElementType = string;

export interface LineInfo {
  type: ElementType;
  depth: number;
  /** Whether the choice/gather uses sticky (+) sigils */
  sticky: boolean;
  /** Whether a divert is standalone (just "-> target", not a tunnel) */
  standalone: boolean;
  /**
   * Option identity: the full lineage of option indices through the weave for
   * `Choice` and `ChoiceBody` lines (e.g. `[0, 2, 1]` — third option under the
   * first option's sub-weave, second option of that group). Zero-based per
   * weave level; gathers close their level's groups so a following option at
   * the same depth starts a new group at index 0. Absent on all other lines.
   */
  optionPath?: readonly number[];
  /**
   * Dialect-classification geometry (#368), present only on dialect-matched
   * lines. Computed ONCE here (from the wasm `dialect` facet, or the TS
   * interpreter fallback) and cached — `screenplay.ts`'s decorations/atomic-
   * ranges/edit-guard and `keybindings.ts`'s name-surgery handlers read this
   * directly and never re-match a pattern in a per-keystroke hot path.
   */
  dialect?: DialectGeometry;
}

/** Cached per-line dialect match: attrs + geometry, byte spans already
 *  resolved to UTF-16 code-unit offsets relative to the line start. */
export interface DialectGeometry {
  kind: string;
  attrs: readonly (readonly [string, string])[];
  hiddenSpans: readonly (readonly [number, number])[];
  contentSpan: readonly [number, number] | null;
}

const BASE_CLASSES: Record<string, string> = {
  [ElementType.KnotHeader]: "brink-knot-header",
  [ElementType.StitchHeader]: "brink-stitch-header",
  [ElementType.NarrativeText]: "brink-narrative",
  [ElementType.Choice]: "brink-choice",
  [ElementType.ChoiceBody]: "brink-choice-body",
  [ElementType.Gather]: "brink-gather",
  [ElementType.Divert]: "brink-divert",
  [ElementType.Logic]: "brink-logic",
  [ElementType.VarDecl]: "brink-var-decl",
  [ElementType.Comment]: "brink-comment",
  [ElementType.Todo]: "brink-todo",
  [ElementType.Include]: "brink-include",
  [ElementType.External]: "brink-external",
  [ElementType.Tag]: "brink-tag",
  [ElementType.Blank]: "brink-blank",
  [ElementType.Character]: "brink-character",
  [ElementType.Parenthetical]: "brink-parenthetical",
  [ElementType.Dialogue]: "brink-dialogue",
};

/** CSS class for an element kind: `brink-<kind>` — the open scheme (#368).
 *  Kinds not in the built-in table (a dialect-declared kind) still derive
 *  mechanically, so a custom dialect's kinds are stylable with zero editor
 *  changes. */
export function elementClass(type: ElementType): string {
  return BASE_CLASSES[type] ?? `brink-${type}`;
}

// ── LineContext → LineInfo conversion ────────────────────────────────

function lineElementToType(element: string): ElementType {
  switch (element) {
    case "knot_header": return ElementType.KnotHeader;
    case "stitch_header": return ElementType.StitchHeader;
    case "narrative": return ElementType.NarrativeText;
    case "choice": return ElementType.Choice;
    case "gather": return ElementType.Gather;
    case "divert": return ElementType.Divert;
    case "logic": return ElementType.Logic;
    case "var_decl": return ElementType.VarDecl;
    case "comment": return ElementType.Comment;
    case "include": return ElementType.Include;
    case "external": return ElementType.External;
    case "tag": return ElementType.Tag;
    default: return ElementType.Blank;
  }
}

function isSticky(weaveElement: WeaveElement): boolean {
  if (typeof weaveElement === "object" && "choice_line" in weaveElement) {
    return weaveElement.choice_line.sticky;
  }
  return false;
}

/** Convert a Rust `DialectLineInfo` (byte spans) into `DialectGeometry`
 *  (UTF-16 spans, line-relative). Source lines here are always plain ASCII
 *  sigils/prefixes-and-suffixes-wise for the spans that matter (hidden
 *  affixes), but content may be arbitrary text — byte and UTF-16 offsets can
 *  diverge inside multi-byte content. The dialect's hidden/content spans are
 *  always anchored on the literal affix text (ASCII in every known
 *  convention), so re-deriving via `indexOf`-free byte counting up front
 *  keeps this correct without a full UTF-16↔byte index. */
function toGeometry(
  d: NonNullable<LineContext["dialect"]>,
  lineText: string,
): DialectGeometry {
  const byteToUtf16 = makeByteToUtf16(lineText);
  return {
    kind: d.kind,
    attrs: d.attrs,
    hiddenSpans: d.hidden_spans.map(([s, e]) => [byteToUtf16(s), byteToUtf16(e)] as const),
    contentSpan: d.content_span
      ? [byteToUtf16(d.content_span[0]), byteToUtf16(d.content_span[1])]
      : null,
  };
}

/** Build a byte-offset → UTF-16-offset mapping function for one line. Cheap
 *  for the common (all-ASCII) case; correct for any content, including
 *  astral-plane characters (emoji, etc.) — iterates Unicode CODE POINTS
 *  (`for...of`), not UTF-16 code units, so a surrogate pair is measured as
 *  one 4-byte-UTF-8 unit advancing the UTF-16 offset by 2, never split into
 *  two lone-surrogate encodes (which `TextEncoder` would replace with U+FFFD
 *  and silently corrupt the table). The reverse map is a direct `Map` lookup
 *  (O(1)), not a linear `indexOf` scan, since spans are only ever queried at
 *  a handful of fixed byte offsets (hidden/content span endpoints) per line. */
function makeByteToUtf16(text: string): (byteOffset: number) => number {
  // Fast path: pure ASCII text (the overwhelming common case for dialect
  // sigil lines) — byte offset === UTF-16 offset.
  // eslint-disable-next-line no-control-regex
  if (/^[\x00-\x7F]*$/.test(text)) {
    return (b) => b;
  }
  const encoder = new TextEncoder();
  const byteToUtf16 = new Map<number, number>();
  let byte = 0;
  let utf16 = 0;
  byteToUtf16.set(0, 0);
  for (const ch of text) {
    byte += encoder.encode(ch).length;
    utf16 += ch.length; // 1 for BMP, 2 for a surrogate-pair code point
    byteToUtf16.set(byte, utf16);
  }
  return (b) => byteToUtf16.get(b) ?? text.length;
}

function lineContextToLineInfo(ctx: LineContext, lineText: string): LineInfo {
  let type = lineElementToType(ctx.element);
  // Narrative inside a choice body is "choice body", not plain narrative.
  // Blank lines inside a body carry the body weave too (#478 — the Rust
  // classifier inherits it through blank runs), so Tab works anywhere in
  // the body; this replaces the old TS blank-after-choice post-pass.
  if (
    (type === ElementType.NarrativeText || type === ElementType.Blank) &&
    ctx.weave.element === "choice_body"
  ) {
    type = ElementType.ChoiceBody;
  }
  const depth = ctx.weave.depth;
  const sticky = isSticky(ctx.weave.element);

  // Structural facts from the Rust classifier (#480) — no text sniffing,
  // no TS weave re-walk. `lineText` is still used by callers for dialect
  // geometry conversion.
  void lineText;
  const info: LineInfo = { type, depth, sticky, standalone: ctx.standalone };
  if (ctx.option_path !== undefined) info.optionPath = ctx.option_path;
  return info;
}

// ── Regex fallback for when session hasn't been updated yet ─────────

export function classifyLine(text: string): LineInfo {
  const trimmed = text.trimStart();

  if (trimmed === "") {
    return { type: ElementType.Blank, depth: 0, sticky: false, standalone: false };
  }

  if (/^={2,}\s*\w/.test(trimmed) || /^={3,}/.test(trimmed)) {
    return { type: ElementType.KnotHeader, depth: 0, sticky: false, standalone: false };
  }

  if (/^=\s+\w/.test(trimmed) || (trimmed.startsWith("=") && !trimmed.startsWith("==") && /^=\s*\w/.test(trimmed))) {
    return { type: ElementType.StitchHeader, depth: 0, sticky: false, standalone: false };
  }

  if (/^[*+]/.test(trimmed)) {
    let depth = 0;
    let sticky = false;
    let i = 0;
    while (i < trimmed.length && (trimmed[i] === "*" || trimmed[i] === "+")) {
      if (trimmed[i] === "+") sticky = true;
      depth++;
      i++;
      while (i < trimmed.length && trimmed[i] === " ") i++;
    }
    return { type: ElementType.Choice, depth, sticky, standalone: false };
  }

  if (trimmed.startsWith("-") && !trimmed.startsWith("->")) {
    let depth = 0;
    let i = 0;
    while (i < trimmed.length && trimmed[i] === "-") {
      depth++;
      i++;
      while (i < trimmed.length && trimmed[i] === " ") i++;
    }
    return { type: ElementType.Gather, depth, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("->")) {
    const isTunnel = /^->.*->/.test(trimmed);
    return { type: ElementType.Divert, depth: 0, sticky: false, standalone: !isTunnel };
  }

  if (trimmed.startsWith("~")) {
    return { type: ElementType.Logic, depth: 0, sticky: false, standalone: false };
  }

  if (/^(VAR|CONST|LIST)\s/.test(trimmed)) {
    return { type: ElementType.VarDecl, depth: 0, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("//") || trimmed.startsWith("/*")) {
    return { type: ElementType.Comment, depth: 0, sticky: false, standalone: false };
  }

  // `TODO` is a keyword only when it opens the line; the colon is optional
  // (the parser's `author_warning` consumes `KW_TODO` then the rest of the
  // line). `\b` keeps `TODOS are…` narrative, matching the lexer's
  // longest-identifier rule.
  if (/^TODO\b/.test(trimmed)) {
    return { type: ElementType.Todo, depth: 0, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("INCLUDE ")) {
    return { type: ElementType.Include, depth: 0, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("EXTERNAL ")) {
    return { type: ElementType.External, depth: 0, sticky: false, standalone: false };
  }

  if (trimmed.startsWith("#")) {
    return { type: ElementType.Tag, depth: 0, sticky: false, standalone: false };
  }

  return { type: ElementType.NarrativeText, depth: 0, sticky: false, standalone: false };
}

// ── Option identity post-pass (#364) ────────────────────────────────
// Assigns every Choice line and its ChoiceBody lines an option path — the
// full lineage of zero-based option indices through the weave — so hosts can
// tell consecutive options at the same depth apart (and know which parent a
// nested option belongs to) without re-deriving the weave themselves.
//
// Rules:
// - A Choice at depth d closes any open options deeper than d, takes the next
//   index in the current depth-d group, and becomes the open option at d.
// - A Gather at depth d closes the depth-d group (and everything deeper);
//   the next Choice at depth d starts a new group at index 0.
// - ChoiceBody lines inherit the innermost open option's path.
// - Knot/stitch headers reset the weave entirely.

/** Mutates `infos` in place, setting `optionPath` on Choice/ChoiceBody lines. */
export function assignOptionPaths(infos: LineInfo[]): void {
  // Open options, outermost first. Each entry remembers its weave DEPTH
  // (sigil count) separately from its lineage position: valid ink can skip
  // depths (`*` straight to `* * *`), so lineage length and depth are not
  // interchangeable — a sibling at depth d must pop every open option at
  // depth >= d, however deep the lineage runs.
  const open: Array<{ depth: number; index: number }> = [];
  // counters[d - 1] = next option index for the open group at weave depth d.
  const counters: number[] = [];

  for (const info of infos) {
    switch (info.type) {
      case ElementType.KnotHeader:
      case ElementType.StitchHeader:
        open.length = 0;
        counters.length = 0;
        break;

      case ElementType.Choice: {
        const d = Math.max(1, info.depth);
        // Close options at this depth or deeper; keep the depth-d group counting.
        while (open.length > 0 && open[open.length - 1].depth >= d) open.pop();
        if (counters.length > d) counters.length = d;
        while (counters.length < d) counters.push(0);
        const index = counters[d - 1];
        counters[d - 1] = index + 1;
        open.push({ depth: d, index });
        info.optionPath = open.map((o) => o.index);
        break;
      }

      case ElementType.Gather: {
        const d = Math.max(1, info.depth);
        // A gather at depth d closes its level's group and everything deeper.
        while (open.length > 0 && open[open.length - 1].depth >= d) open.pop();
        if (counters.length >= d) counters.length = d - 1;
        break;
      }

      case ElementType.ChoiceBody:
      // Screenplay retypes (cue/parenthetical/dialogue) inside an open option
      // are still body lines — without a path they'd punch holes in the
      // per-branch rail the feature exists to enable.
      case ElementType.Character:
      case ElementType.Parenthetical:
      case ElementType.Dialogue:
        if (open.length > 0) info.optionPath = open.map((o) => o.index);
        break;

      default:
        // Other lines (narrative, logic, diverts, blanks, …) neither open nor
        // close option groups.
        break;
    }
  }
}

// ── Dialect post-pass (#368) ─────────────────────────────────────────
//
// Two sources, one contract: when a wasm document handle is present, the
// dialect facet already lives on `LineContext.dialect` (Rust classified it —
// `line_contexts_with_dialect`). When there's no handle (regex-fallback
// path), the TS interpreter in `dialect.ts` runs the SAME dialect JSON
// against the plain-narrative/choice-body lines the regex classifier
// produced. Either way, the result lands in the same place: `LineInfo.type`
// retyped to the dialect kind, plus cached `LineInfo.dialect` geometry.

/**
 * The active dialect, as a CM6 Facet — NOT a module-level variable. Each
 * `EditorState` carries its own value (set from `BrinkStudioOptions.dialect`
 * via `extensions.ts`'s `dialectFacet.of(...)` in the extension array), so
 * two views mounted with different dialects (or one `DocumentSessions`
 * instance managing several sessions) never clobber each other — a module
 * global here would let mounting/reconfiguring one view's dialect silently
 * change classification for every other live view. Defaults to the at-cue
 * preset (byte-identical to the pre-#368 hardcoded behavior) when no
 * extension in a state's config provides one (e.g. a bare `EditorState`
 * built without `brinkStudio(...)`, as some unit tests do). `null` (the
 * `dialect: null` mount option, #368 deliverable 5) disables the whole
 * screenplay layer — the post-pass below becomes a no-op and callers never
 * see dialect kinds.
 */
export const dialectFacet = Facet.define<ResolvedDialect | null, ResolvedDialect | null>({
  combine: (values) => (values.length > 0 ? values[0] : ResolvedDialect.compile(AT_CUE_DIALECT)),
});

/**
 * Whether a trimmed, non-empty line is a multiline branch header
 * (`- cond:` / `- else:` — a `-`-bulleted, non-`->` line whose last
 * non-whitespace character is `:`) — mirrors Rust
 * `is_conditional_branch_header_line` in `line_context.rs` exactly (#413).
 * Deliberately does NOT match `- else: -> busy` (inline-divert branch
 * shorthand): that line's own divert/gather classification from
 * `classifyLine` is left untouched.
 *
 * Unlike the brace check (see `applyConditionalScaffoldFallback`), this
 * may override a line `classifyLine` already swept to `Gather` (its bare
 * `-` heuristic can't tell a branch header apart from a weave gather) in
 * addition to a still-`Blank` line.
 */
function isConditionalBranchHeaderLine(trimmed: string): boolean {
  return trimmed.startsWith("-") && !trimmed.startsWith("->") && trimmed.endsWith(":");
}

/**
 * Whether a trimmed, non-empty line is a conditional/sequence opening
 * brace (bare `{`, or `{` followed by a switch expression whose own last
 * non-whitespace character is `:`, e.g. `{ get_variable(17) >= 1:`) or a
 * bare closing brace (exactly `}`, nothing else on the line) — mirrors
 * Rust `is_conditional_brace_scaffold_line` in `line_context.rs` exactly
 * (#413).
 *
 * Deliberately narrower than "starts with `{`" / "ends with `}`": a
 * narrative line can itself start or end with a brace due to ink's
 * inline-logic syntax without being the block's own routing scaffold —
 * e.g. a standalone inline conditional used as narrative content,
 * `{visited: You were here before.}` (starts with `{` but does NOT end
 * with `:` — it ends with prose closed by `}` on the same line), or
 * narrative ending in a value interpolation, `You have {gold}` (ends
 * with `}` but does not start with `{`). Neither shape is a genuine
 * scaffold brace, so neither is matched. The predicate is precise enough
 * that `applyConditionalScaffoldFallback` applies it unconditionally
 * (not gated to a prior `Blank`/`Gather` classification like the
 * branch-header check) — see that function's comment for why.
 */
function isConditionalBraceScaffoldLine(trimmed: string): boolean {
  if (trimmed === "{" || trimmed === "}") return true;
  return trimmed.startsWith("{") && trimmed.endsWith(":");
}

/**
 * Conditional/sequence scaffold + arm-descent pass (#413), text-only
 * mirror of Rust `apply_conditional_scaffold`. The regex fallback has no
 * HIR, so brace nesting is tracked directly from source text: any line
 * whose net open-brace depth (counting `{`/`}` on that line) is greater
 * than zero before the line starts, OR which itself opens a brace, is
 * "inside" a conditional/sequence block. Within such a region:
 *
 * - a branch-header line (see `isConditionalBranchHeaderLine`) becomes
 *   `Logic`, overriding `Blank` or `Gather` — this corrects `- else:`
 *   away from the `classifyLine` bare-`-` Gather heuristic, which doesn't
 *   know about conditional nesting;
 * - a brace-scaffold line (see `isConditionalBraceScaffoldLine`) becomes
 *   `Logic`, but ONLY when still `Blank` — never overriding `NarrativeText`,
 *   so ordinary narrative containing inline logic that starts/ends with a
 *   brace keeps its narrative classification instead of being swept into
 *   scaffold;
 * - a line the base classifier left as `Blank` or `Gather` (and isn't
 *   itself scaffold) is promoted to `NarrativeText` so the dialect
 *   classify pass (which only looks at `NarrativeText`/`ChoiceBody`) can
 *   see cues/dialogue written inside the arm. Lines the base classifier
 *   already placed with confidence (a nested `Choice`, a `Divert` from an
 *   inline `- cond: -> target` shorthand, etc.) are left untouched — this
 *   only fills gaps.
 *
 * Runs BEFORE `applyDialectFallback`'s classify pass so promoted arm lines
 * are visible to it.
 */
function applyConditionalScaffoldFallback(infos: LineInfo[], lineTexts: string[]): void {
  let depth = 0;
  for (let i = 0; i < infos.length; i++) {
    const text = lineTexts[i];
    const trimmed = text.trim();
    const enteringDepth = depth;
    for (const ch of trimmed) {
      if (ch === "{") depth++;
      else if (ch === "}") depth = Math.max(0, depth - 1);
    }
    const insideConditional = enteringDepth > 0 || trimmed.startsWith("{") || trimmed.endsWith("}");
    if (!insideConditional || trimmed === "") continue;

    const isGap = infos[i].type === ElementType.Blank || infos[i].type === ElementType.Gather;

    if (isGap && isConditionalBranchHeaderLine(trimmed)) {
      infos[i] = { type: ElementType.Logic, depth: 0, sticky: false, standalone: false };
      continue;
    }

    // The brace-scaffold predicate is syntactically precise (bare `{`/`}`,
    // or `{`-opener-with-trailing-`:`) — it structurally cannot match
    // ordinary narrative containing inline logic, so unlike the header
    // check above it doesn't need to be gated to `isGap`: a bare `{`/`}`
    // scaffold line is classified as `NarrativeText` by `classifyLine`
    // (which has no notion of conditional scaffold), not `Blank`, so
    // restricting this to `isGap` would leave real scaffold braces
    // unclassified in the regex-fallback path (no HIR to leave a true
    // gap the way the Rust pass does).
    if (isConditionalBraceScaffoldLine(trimmed)) {
      infos[i] = { type: ElementType.Logic, depth: 0, sticky: false, standalone: false };
      continue;
    }

    if (isGap) {
      infos[i] = { type: ElementType.NarrativeText, depth: 0, sticky: false, standalone: false };
    }
  }
}

/**
 * Dialect post-pass over already-computed `infos` (regex-fallback path
 * only): classify + chain using `dialect`'s TS interpreter, mirroring the
 * Rust classify/chain split exactly (`line_context.rs`'s `apply_dialect`) —
 * classify runs on narrative AND choice-body base lines (depth preserved);
 * chaining runs on plain top-level narrative and conditional/sequence-arm
 * narrative (#413), never choice-body narrative.
 */
function applyDialectFallback(
  dialect: ResolvedDialect | null,
  infos: LineInfo[],
  lineTexts: string[],
): void {
  if (dialect === null) return;

  applyConditionalScaffoldFallback(infos, lineTexts);

  // ── Classify pass ──
  for (let i = 0; i < infos.length; i++) {
    const info = infos[i];
    if (info.type !== ElementType.NarrativeText && info.type !== ElementType.ChoiceBody) continue;
    const text = lineTexts[i];
    const trimmed = text.trimStart();
    if (trimmed === "") continue;
    const leadingWs = text.length - trimmed.length;
    const match = dialect.classify(trimmed, leadingWs);
    if (match) {
      infos[i] = {
        ...info,
        type: match.kind,
        dialect: { kind: match.kind, attrs: match.attrs, hiddenSpans: match.hiddenSpans, contentSpan: match.contentSpan },
      };
    }
  }

  // ── Chain pass (top-level or conditional/sequence-arm narrative only;
  //    blank always breaks) ──
  // `runCarry` mirrors the Rust `run_carry: Vec<(String, String)>` — a `Map`
  // preserves insertion order identically and gives O(1) update-in-place
  // (`.set`) instead of rebuilding the whole array per attr per line.
  let runCarry = new Map<string, string>();
  for (let i = 0; i < infos.length; i++) {
    const text = lineTexts[i];
    const isBlank = text.trim() === "";
    if (isBlank) {
      runCarry = new Map();
      continue;
    }
    const info = infos[i];
    if (
      i > 0 &&
      info.type === ElementType.NarrativeText &&
      !info.dialect &&
      infos[i - 1].dialect
    ) {
      const prevKind = infos[i - 1].dialect!.kind;
      const rule = dialect.chainRuleAfter(prevKind);
      if (rule) {
        const carried: Array<[string, string]> = [];
        for (const name of rule.carry ?? []) {
          const value = runCarry.get(name);
          if (value !== undefined) carried.push([name, value]);
        }
        infos[i] = {
          ...info,
          type: rule.becomes,
          dialect: { kind: rule.becomes, attrs: carried, hiddenSpans: [], contentSpan: null },
        };
      }
    }
    if (infos[i].dialect) {
      for (const [k, v] of infos[i].dialect!.attrs) {
        runCarry.set(k, v);
      }
    } else {
      runCarry = new Map();
    }
  }
}

// ── StateField ──────────────────────────────────────────────────────

function computeLineInfos(state: EditorState): LineInfo[] {
  // The view's own document handle (per-view DocId, see document-handle.ts).
  // Pushing here keeps the wasm session in sync with this view on every doc
  // change, before any extension queries run against the new state.
  const handle = state.facet(documentHandleFacet)?.handle ?? null;
  if (handle) {
    handle.pushSource(state.doc.toString());
    const contexts = handle.lineContexts();

    const infos: LineInfo[] = [];
    for (let i = 0; i < contexts.length && i < state.doc.lines; i++) {
      const line = state.doc.line(i + 1);
      const ctx = contexts[i];
      const info = lineContextToLineInfo(ctx, line.text);
      if (ctx.dialect) {
        const geometry = toGeometry(ctx.dialect, line.text);
        info.type = ctx.dialect.kind;
        info.dialect = geometry;
      }
      infos.push(info);
    }
    // Fill remaining lines with regex fallback (shouldn't happen normally)
    const contextCount = infos.length;
    for (let i = infos.length; i < state.doc.lines; i++) {
      const line = state.doc.line(i + 1);
      infos.push(classifyLine(line.text));
    }
    // A handle that yields fewer contexts than lines — not attached yet, not
    // synced, or a host mock (`line_contexts_doc` returning `[]`) — means the
    // tail lines above came from bare `classifyLine`, with no dialect
    // classification applied. Without this, dialect classes silently vanish
    // for those lines (#426). Run the same TS dialect fallback interpreter
    // used on the no-handle path below so the drop doesn't happen silently.
    if (contextCount < state.doc.lines) {
      const lineTexts: string[] = [];
      for (let i = 1; i <= state.doc.lines; i++) lineTexts.push(state.doc.line(i).text);
      applyDialectFallback(state.facet(dialectFacet), infos, lineTexts);
    }
    // Option identity comes from the Rust classifier (#480) — the TS
    // weave re-walk (`assignOptionPaths`) serves only regex-classified
    // lines. A handle that yields no contexts at all (not attached yet, or
    // a host mock) means every line above came from `classifyLine`, so the
    // fallback walk still applies.
    if (contexts.length === 0) assignOptionPaths(infos);
    return infos;
  }

  // Fallback: no session yet, use regex classifier + TS dialect interpreter
  const infos: LineInfo[] = [];
  const lineTexts: string[] = [];
  for (let i = 1; i <= state.doc.lines; i++) {
    const line = state.doc.line(i);
    lineTexts.push(line.text);
    infos.push(classifyLine(line.text));
  }
  applyDialectFallback(state.facet(dialectFacet), infos, lineTexts);
  assignOptionPaths(infos);
  return infos;
}

/**
 * Force reclassification without a doc change (#368): `setDialect(view, d)`
 * dispatches this alongside swapping the screenplay compartment and re-
 * running the wasm `set_dialect`, so `elementTypeField` recomputes even
 * though the document text itself didn't change.
 */
export const reclassifyEffect = StateEffect.define<void>();

export const elementTypeField = StateField.define<LineInfo[]>({
  create(state) {
    return computeLineInfos(state);
  },
  update(value, tr: Transaction) {
    if (!tr.docChanged && !tr.effects.some((e) => e.is(reclassifyEffect))) return value;
    return computeLineInfos(tr.state);
  },
});
