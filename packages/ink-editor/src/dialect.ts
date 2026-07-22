/**
 * The dialogue dialect (#368) — TS interpreter + public surface.
 *
 * A `DialogueDialect` is versioned pure JSON (no functions, no `RegExp`
 * objects) describing a project's dialogue-line conventions. Classification
 * is implemented once, in Rust (`brink_ir::dialect` / `line_context.rs`) —
 * when a wasm document handle is present, `element-type.ts` consumes the
 * `dialect` facet Rust already computed. This module is the **thin TS
 * interpreter over the same JSON** for the regex-fallback path (no wasm
 * session yet), so both paths are pinned to the same conformance corpus
 * (`tests/dialect_fixtures/at_cue.json`).
 *
 * See docs/dialect-spec.md for the full schema contract.
 */

import type {
  DialogueDialect,
  DialectElement,
  ChainRule,
  SourceShape,
  PatternShape,
  AffixShape,
  TransitionRow,
} from "@brink/wasm-types";
import type { ConvertibleShape } from "@brink/ink-operations";

export type {
  DialogueDialect,
  DialectElement,
  ChainRule,
  SourceShape,
  PatternShape,
  AffixShape,
  TransitionRow,
  TransitionAction,
  TemplateEntry,
  Templates,
  ElementNature,
} from "@brink/wasm-types";

// ── Affix sugar compilation (mirrors `compile_affix` in dialect.rs) ──

const GLUE = "<>";

// Deliberately NOT reusing `escapeRegExp` from `project-search.ts` (same
// one-line body) — this module is a self-contained 1:1 port of dialect.rs
// (which calls Rust's `regex::escape`), and importing an unrelated feature
// module's helper for a coincidental one-liner would add a cross-cutting
// dependency this module otherwise has none of.
function escapeRegexLiteral(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function escapeClassChar(c: string): string {
  return c === "]" || c === "\\" || c === "^" || c === "-" ? `\\${c}` : c;
}

/** Compile affix sugar to the canonical pattern form — the ONE derivation
 *  site (mirrors Rust `compile_affix` exactly; see dialect.rs). */
export function compileAffix(affix: AffixShape): PatternShape {
  const prefix = affix.prefix ?? "";
  let suffix = affix.suffix ?? "";
  if (affix.glued) suffix += GLUE;
  const role = affix.content_role ?? "content";

  let pattern = "^";
  const hidden: string[] = [];
  let template = "";

  if (prefix !== "") {
    pattern += `(?<lead>${escapeRegexLiteral(prefix)})`;
    hidden.push("lead");
    template += prefix;
  }

  pattern += `(?<${role}>[^`;
  if (suffix.length > 0) {
    pattern += escapeClassChar(suffix[0]);
  }
  pattern += "]*)";
  template += `\${${role}}`;

  if (suffix !== "") {
    pattern += `(?<tail>${escapeRegexLiteral(suffix)})`;
    hidden.push("tail");
    template += suffix;
  }
  pattern += "$";

  return { pattern, content_group: role, hidden, template };
}

/** Resolve a `SourceShape` to its canonical `PatternShape`, compiling affix
 *  sugar if necessary. The only derivation site TS classifiers/validators go
 *  through (mirrors `SourceShape::resolve` in dialect.rs). */
export function resolveSourceShape(shape: SourceShape): PatternShape {
  if ("pattern" in shape) return shape;
  return compileAffix(shape);
}

/**
 * Render a kind's `template` string, filling its content-group placeholder
 * (`${role}`) with `content`. Used by a dialect transition row's `convert`
 * action (#368 deliverable 4) to compose the target line — indentation is
 * the caller's job (preserved separately; template rendering never touches
 * leading whitespace, matching the "indentation is preserved in every
 * compose/convert" contract).
 */
export function renderTemplate(template: string, role: string, content: string): string {
  return template.split(`\${${role}}`).join(content);
}

// ── Portable-regex subset check (mirrors `check_portable_pattern`) ───

/** Reject portable-regex-subset violations: lookaround and backreferences —
 *  constructs the Rust `regex` crate cannot express at all, so both
 *  interpreters must reject them explicitly and identically. */
export function checkPortablePattern(pattern: string): string | null {
  let i = 0;
  while (i < pattern.length) {
    if (pattern[i] === "\\") {
      const next = pattern[i + 1];
      if (next !== undefined && next >= "1" && next <= "9") {
        return "backreferences are not allowed";
      }
      if (next === "k" && pattern[i + 2] === "<") {
        return "backreferences are not allowed";
      }
      i += 2;
      continue;
    }
    if (pattern[i] === "(" && pattern[i + 1] === "?") {
      const rest = pattern.slice(i + 2);
      if (rest.startsWith("=") || rest.startsWith("!")) return "lookahead is not allowed";
      if (rest.startsWith("<=") || rest.startsWith("<!")) return "lookbehind is not allowed";
    }
    i++;
  }
  return null;
}

// ── `d`-flag (`hasIndices`) feature detection (#1013) ────────────────
//
// `RegExp`'s `d` flag (per-named-group match `indices`) needs V8 9.0 /
// Chromium 90+. NW.js-hosted embedders (e.g. RPG Maker MZ's bundled NW.js,
// Chromium 88) throw `SyntaxError: Invalid flags supplied to RegExp
// constructor 'd'` at CONSTRUCTION time — before a single line is ever
// classified — black-screening the embedder at boot. Detected ONCE here, at
// module scope: every dialect compiled in this process reuses the same
// answer, so classifying thousands of lines never repeats a construction
// probe. `ResolvedDialect.compile` picks the `d`-flag path when supported
// (indices come straight from the engine) and the capture-group-walk
// fallback otherwise (`walkGroupSpans`, below) — same `DialectMatch` output
// either way, proven equal by `dialect-fallback.test.ts` in brink-studio
// (this package has no test runner of its own; that suite forces the
// fallback path unconditionally since CI's own engine supports `d`).
const SUPPORTS_D_FLAG: boolean = (() => {
  try {
    // scan-allow: chromium88 d-flag feature probe (#1013) — this
    // construction MUST be the only unconditional `d`-flag literal in the
    // editor/studio bundles; see chromium88-regexp-d-flag.test.ts.
    new RegExp("(?:)", "d");
    return true;
  } catch {
    return false;
  }
})();

/** A named-capture-group's position in a pattern's nesting structure — a
 *  group textually enclosed by another named group (e.g. `parenthetical`'s
 *  `content_inner` nested inside `content`) is that group's child. Built
 *  once per compiled pattern (`parseGroupTree`); walked once per match
 *  (`walkGroupSpans`) on the non-`d`-flag fallback path only. */
interface GroupNode {
  readonly name: string;
  readonly children: GroupNode[];
}

/** Parse a pattern's named-capture-group nesting structure — what the `d`
 *  flag's `indices` would otherwise hand us for free. Used only when
 *  `SUPPORTS_D_FLAG` is `false`: `walkGroupSpans` then reconstructs each
 *  named group's `[start, end)` span at match time by locating its captured
 *  text within its nearest enclosing named group's own span (or the whole
 *  match, for a top-level group) — a plain capture-group walk, no `indices`
 *  needed, no re-matching.
 *
 *  Skips character classes (`[...]`) and escape sequences so a literal `(`
 *  or `)` inside them (e.g. `[^)]`, `\(`) is never mistaken for a group
 *  delimiter. Non-capturing (`(?:...)`) and unnamed capturing groups are
 *  tracked only for nesting depth (no node) — a named descendant attaches to
 *  its nearest named ANCESTOR, skipping through them. Lookaround and
 *  backreferences are already rejected by `checkPortablePattern` before a
 *  dialect ever reaches `compile`, so this only has to handle the portable
 *  regex subset dialects are restricted to. */
function parseGroupTree(pattern: string): GroupNode[] {
  const root: GroupNode[] = [];
  const stack: Array<{ node: GroupNode | null }> = [];
  let i = 0;
  while (i < pattern.length) {
    const c = pattern[i];
    if (c === "\\") {
      i += 2;
      continue;
    }
    if (c === "[") {
      i++;
      if (pattern[i] === "^") i++;
      if (pattern[i] === "]") i++;
      while (i < pattern.length && pattern[i] !== "]") {
        i += pattern[i] === "\\" ? 2 : 1;
      }
      i++;
      continue;
    }
    if (c === "(") {
      const isNamed =
        pattern[i + 1] === "?" &&
        pattern[i + 2] === "<" &&
        pattern[i + 3] !== "=" &&
        pattern[i + 3] !== "!";
      if (isNamed) {
        const closeIdx = pattern.indexOf(">", i + 3);
        const name = pattern.slice(i + 3, closeIdx);
        const node: GroupNode = { name, children: [] };
        const parentEntry = [...stack].reverse().find((e) => e.node !== null);
        (parentEntry?.node?.children ?? root).push(node);
        stack.push({ node });
        i = closeIdx + 1;
      } else {
        stack.push({ node: null });
        i++;
      }
      continue;
    }
    if (c === ")") {
      stack.pop();
      i++;
      continue;
    }
    i++;
  }
  return root;
}

/** Reconstruct every participating named group's `[start, end)` span (in
 *  `m[0]`-relative-then-absolute coordinates) by walking the static group
 *  tree alongside the match: for each group at a nesting level, find its
 *  captured text within its parent's span text (searching forward from the
 *  previous sibling's end, so repeated identical substrings at the same
 *  level resolve in source order), then recurse into its children using its
 *  own span as the new parent context. A group that did not participate in
 *  the match (`groups[name] === undefined`) — and so did its entire subtree
 *  — is skipped. */
function walkGroupSpans(
  m: RegExpExecArray,
  tree: readonly GroupNode[],
  groups: Record<string, string | undefined>,
): Record<string, [number, number] | undefined> {
  const out: Record<string, [number, number] | undefined> = {};
  walkGroupSpansInto(tree, m[0], m.index, groups, out);
  return out;
}

function walkGroupSpansInto(
  nodes: readonly GroupNode[],
  parentText: string,
  parentOffset: number,
  groups: Record<string, string | undefined>,
  out: Record<string, [number, number] | undefined>,
): void {
  let searchFrom = 0;
  for (const node of nodes) {
    const value = groups[node.name];
    if (value === undefined) continue;
    const idx = parentText.indexOf(value, searchFrom);
    if (idx === -1) continue;
    const start = parentOffset + idx;
    const end = start + value.length;
    out[node.name] = [start, end];
    searchFrom = idx + value.length;
    if (node.children.length > 0) {
      walkGroupSpansInto(node.children, value, start, groups, out);
    }
  }
}

// ── Reserved structural kinds (mirrors `reserved_structural_kinds`) ──

const RESERVED_STRUCTURAL_KINDS: readonly string[] = [
  "knot_header",
  "stitch_header",
  "narrative",
  "choice",
  "choice_body",
  "gather",
  "divert",
  "logic",
  "var_decl",
  "comment",
  "include",
  "external",
  "tag",
  "blank",
];

/** Kinds reserved by the interpreter's built-in structural taxonomy. Chain/
 *  transition rows may reference these without the dialect declaring them. */
export function reservedStructuralKinds(): readonly string[] {
  return RESERVED_STRUCTURAL_KINDS;
}

// ── Validation (mirrors `validate` in dialect.rs) ────────────────────

export interface DialectValidationError {
  kind:
    | "unsupported_version"
    | "non_portable_pattern"
    | "invalid_pattern"
    | "template_roundtrip_failed"
    | "chain_undeclared_kind"
    | "transition_undeclared_kind"
    | "duplicate_kind"
    | "chain_becomes_undeclared";
  message: string;
}

const ROUNDTRIP_PROBES = ["PROBE", "(PROBE)", "[PROBE]"];

function templateRoundtrips(re: RegExp, shape: PatternShape): boolean {
  // Extract named groups CM/JS RegExp declares.
  const groupNames = extractGroupNames(shape.pattern);
  return ROUNDTRIP_PROBES.some((probe) => {
    let rendered = shape.template;
    for (const name of groupNames) {
      rendered = rendered.split(`\${${name}}`).join(probe);
    }
    const m = re.exec(rendered);
    if (!m || !m.groups) return false;
    if (shape.content_group) {
      return m.groups[shape.content_group] === probe;
    }
    return true;
  });
}

function extractGroupNames(pattern: string): string[] {
  const names: string[] = [];
  const re = /\(\?<([A-Za-z_][A-Za-z0-9_]*)>/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(pattern)) !== null) {
    names.push(m[1]);
  }
  return names;
}

/** Validate a dialect: schema version, pattern portability + compilation,
 *  template round-trip, chain/transition kind references, duplicate kinds.
 *  Mirrors Rust `validate` (dialect.rs) — the same JSON must be accepted or
 *  rejected identically by both interpreters. */
export function validateDialect(dialect: DialogueDialect): DialectValidationError[] {
  const errors: DialectValidationError[] = [];

  if (dialect.version !== 1) {
    errors.push({
      kind: "unsupported_version",
      message: `unsupported dialect version ${dialect.version} (only version 1 is defined)`,
    });
  }

  const seenKinds = new Set<string>();
  const declaredKinds = new Set<string>();
  for (const el of dialect.elements ?? []) {
    if (seenKinds.has(el.kind)) {
      errors.push({ kind: "duplicate_kind", message: `duplicate element kind '${el.kind}'` });
    }
    seenKinds.add(el.kind);
    declaredKinds.add(el.kind);

    if (el.source) {
      const resolved = resolveSourceShape(el.source);
      const portabilityError = checkPortablePattern(resolved.pattern);
      if (portabilityError) {
        errors.push({
          kind: "non_portable_pattern",
          message: `kind '${el.kind}': pattern uses a non-portable construct: ${portabilityError}`,
        });
        continue;
      }
      try {
        const re = new RegExp(resolved.pattern);
        if (!templateRoundtrips(re, resolved)) {
          errors.push({
            kind: "template_roundtrip_failed",
            message: `kind '${el.kind}': template '${resolved.template}' does not round-trip against its pattern`,
          });
        }
      } catch (e) {
        errors.push({
          kind: "invalid_pattern",
          message: `kind '${el.kind}': pattern failed to compile: ${String(e)}`,
        });
      }
    }
  }

  const reserved = new Set(RESERVED_STRUCTURAL_KINDS);
  const isKnown = (k: string) => declaredKinds.has(k) || reserved.has(k);

  for (const rule of dialect.chain ?? []) {
    for (const k of [...rule.after, ...(rule.is ?? ["narrative"])]) {
      if (!isKnown(k)) {
        errors.push({
          kind: "chain_undeclared_kind",
          message: `chain rule references undeclared, non-structural kind '${k}'`,
        });
      }
    }
    if (!declaredKinds.has(rule.becomes)) {
      errors.push({
        kind: "chain_becomes_undeclared",
        message: `chain rule produces undeclared kind '${rule.becomes}' (add it to \`elements\` with no \`source\`)`,
      });
    }
  }

  for (const row of dialect.transitions ?? []) {
    if (!isKnown(row.on)) {
      errors.push({
        kind: "transition_undeclared_kind",
        message: `transition row references undeclared, non-structural kind '${row.on}'`,
      });
    }
    if (row.action.action === "convert" && !isKnown(row.action.kind)) {
      errors.push({
        kind: "transition_undeclared_kind",
        message: `transition row references undeclared, non-structural kind '${row.action.kind}'`,
      });
    }
  }

  return errors;
}

// ── Resolved (compiled) dialect + classification ─────────────────────

interface ResolvedElement {
  decl: DialectElement;
  re: RegExp | null;
  shape: PatternShape | null;
  /** Non-`null` only on the non-`d`-flag fallback path — `classify` passes
   *  it to `buildMatch` so spans are reconstructed via `walkGroupSpans`
   *  instead of read off `m.indices`. */
  groupTree: readonly GroupNode[] | null;
}

/** A resolved (compiled) dialect — patterns pre-compiled once. Mirrors Rust
 *  `ResolvedDialect`: building this is the only place regex compilation
 *  happens; classifying a line never re-compiles. */
export class ResolvedDialect {
  private constructor(
    private readonly elements: ResolvedElement[],
    private readonly chain: ChainRule[],
    private readonly transitions: TransitionRow[],
    private readonly source: DialogueDialect,
  ) {}

  /** Compile a dialect's patterns once. Throws if any element's pattern
   *  fails to compile (should not happen for a dialect that passed
   *  `validateDialect`). Compiled with the `d` flag — so match indices
   *  (per-named-group spans) are available on every match without
   *  recompiling or re-matching — when the engine supports it
   *  (`SUPPORTS_D_FLAG`, detected once at module scope); falls back to a
   *  capture-group walk (`walkGroupSpans`) otherwise (#1013), for embedders
   *  on Chromium < 90 where the `d` flag throws at construction. */
  static compile(dialect: DialogueDialect): ResolvedDialect {
    return ResolvedDialect.compileWithDFlagSupport(dialect, SUPPORTS_D_FLAG);
  }

  /** `compile`, but with `d`-flag support forced rather than detected. Not
   *  part of the day-to-day surface — exists so tests can exercise the
   *  non-`d`-flag fallback path unconditionally (CI's own engine supports
   *  `d`, so the fallback needs direct coverage proving it produces
   *  identical `DialectMatch` output; see brink-studio's
   *  `dialect-fallback.test.ts`). */
  static compileWithDFlagSupport(dialect: DialogueDialect, supportsDFlag: boolean): ResolvedDialect {
    const elements: ResolvedElement[] = [];
    for (const decl of dialect.elements ?? []) {
      if (decl.source) {
        const shape = resolveSourceShape(decl.source);
        const re = new RegExp(shape.pattern, supportsDFlag ? "d" : "");
        const groupTree = supportsDFlag ? null : parseGroupTree(shape.pattern);
        elements.push({ decl, re, shape, groupTree });
      } else {
        elements.push({ decl, re: null, shape: null, groupTree: null });
      }
    }
    return new ResolvedDialect(elements, dialect.chain ?? [], dialect.transitions ?? [], dialect);
  }

  /** Classify a single trimmed line against the declared elements, in
   *  declaration order (first match wins). `leadingWs` is the UTF-16 length
   *  of the line's leading whitespace, used to offset spans back into
   *  full-line coordinates. Elements with no `source` (chain-only kinds) are
   *  never matched here. */
  classify(trimmed: string, leadingWs: number): DialectMatch | null {
    for (const el of this.elements) {
      if (!el.re || !el.shape) continue;
      const m = el.re.exec(trimmed);
      if (m) return buildMatch(el.decl.kind, m, el.shape, leadingWs, el.groupTree);
    }
    return null;
  }

  /** Chain rules declared by this dialect. */
  chainRules(): readonly ChainRule[] {
    return this.chain;
  }

  /** Find the chain rule (if any) that fires when a narrative line follows a
   *  line of dialect-kind `prevKind`. */
  chainRuleAfter(prevKind: string): ChainRule | null {
    return this.chain.find((r) => r.after.includes(prevKind)) ?? null;
  }

  /** Editor-overlay transition rows this dialect contributes (#368
   *  deliverable 4) — rows for declared kinds only; resolved before the
   *  built-in structural weave table by `transitions.ts`'s `findTransition`. */
  transitionRows(): readonly TransitionRow[] {
    return this.transitions;
  }

  /** The resolved `template` string for a declared kind (e.g.
   *  `"@${speaker}:<>"` for `character`), or `null` for a kind with no
   *  `source` (pattern-less/chain-only) or an unknown kind. Used by a
   *  dialect transition row's `convert` action to compose the target line. */
  templateFor(kind: string): string | null {
    const el = this.elements.find((e) => e.decl.kind === kind);
    if (!el?.shape) return null;
    return el.shape.template;
  }

  /** The template-fill placeholder name for a declared kind (e.g.
   *  `"speaker"` for `character`) — the named placeholder in that kind's
   *  `template` a `convert` transition action's extracted content fills.
   *  This is `template_group` when the kind declares one (#406 — e.g.
   *  `parenthetical`'s `content_group` is wrap-inclusive for `content_span`
   *  geometry, but the template's placeholder is the bare inner group),
   *  falling back to `content_group` for every kind that doesn't need the
   *  distinction. `null` for a pattern-less (chain-only) or unknown kind. */
  contentGroupFor(kind: string): string | null {
    const el = this.elements.find((e) => e.decl.kind === kind);
    return el?.shape?.template_group ?? el?.shape?.content_group ?? null;
  }

  /** The underlying dialect JSON (for `templates.entries` picker metadata
   *  and other editor-overlay reads that don't need compiled patterns). */
  raw(): DialogueDialect {
    return this.source;
  }

  /**
   * Every declared kind's source pattern + content group, reduced to
   * `@brink/ink-operations`'s `ConvertibleShape` (#395). Used by a `convert`
   * transition action to extract a line's content generically: the source
   * line may be classified as ANY declared wrapping kind (not just the
   * built-in `character`/`parenthetical`), so extraction tries every declared
   * shape rather than one hardcoded pair of regexes. Pattern-less (chain-only)
   * kinds have no shape and are omitted.
   *
   * Uses `template_group` (falling back to `content_group`) for the
   * extracted group (#406): a kind's `content_group` may be wrap-inclusive
   * for `content_span` geometry purposes (e.g. `parenthetical`), but a
   * convert/strip round-trip needs the bare value `template` itself wraps —
   * matching `DEFAULT_CONVERTIBLE_SHAPES`'s and the built-in
   * `convertToParenthetical`/`stripToNarrative` actions' "Parenthetical
   * content is the bare text between the parens" convention.
   */
  convertibleShapes(): ConvertibleShape[] {
    const shapes: ConvertibleShape[] = [];
    for (const el of this.elements) {
      if (!el.shape) continue;
      shapes.push({
        pattern: el.shape.pattern,
        contentGroup: el.shape.template_group ?? el.shape.content_group ?? null,
      });
    }
    return shapes;
  }
}

/** One classified dialect match on a line (mirrors Rust `DialectMatch`). */
export interface DialectMatch {
  kind: string;
  attrs: Array<readonly [string, string]>;
  hiddenSpans: Array<readonly [number, number]>;
  contentSpan: readonly [number, number] | null;
}

/** A `RegExpExecArray` from a pattern compiled with the `d` flag: `indices`
 *  carries per-named-group `[start, end)` spans alongside `groups`. */
type IndexedMatch = RegExpExecArray & {
  indices?: RegExpIndicesArray & { groups?: Record<string, [number, number] | undefined> };
};

function buildMatch(
  kind: string,
  m: RegExpExecArray,
  shape: PatternShape,
  leadingWs: number,
  groupTree: readonly GroupNode[] | null,
): DialectMatch {
  const groups = m.groups ?? {};
  const groupIndices = groupTree
    ? walkGroupSpans(m, groupTree, groups)
    : ((m as IndexedMatch).indices?.groups ?? {});
  const hidden = new Set(shape.hidden ?? []);
  const attrs: Array<[string, string]> = [];
  const hiddenSpans: Array<[number, number]> = [];
  let contentSpan: [number, number] | null = null;
  // `template_group` (#406), when it names a DIFFERENT group than
  // `content_group`, is a template-fill-only helper group — it must not leak
  // into `attrs`/`data-*` (byte-identical-attrs contract) or `hiddenSpans`
  // (it's not a hiding instruction; a kind that wants it hidden puts it in
  // `hidden` too). It stays visible on the line, simply excluded from both
  // derived outputs.
  const templateOnlyGroup =
    shape.template_group && shape.template_group !== shape.content_group ? shape.template_group : null;

  for (const name of Object.keys(groups)) {
    const span = groupIndices[name];
    if (!span) continue;
    const [s, e] = span;
    if (hidden.has(name) || name === templateOnlyGroup) {
      if (hidden.has(name)) hiddenSpans.push([leadingWs + s, leadingWs + e]);
      continue;
    }
    const value = groups[name];
    if (value !== undefined) attrs.push([name, value]);
    if (shape.content_group === name) {
      contentSpan = [leadingWs + s, leadingWs + e];
    }
  }

  attrs.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  return { kind, attrs, hiddenSpans, contentSpan };
}

// ── The at-cue preset (mirrors `at_cue_preset` in dialect.rs) ────────

/** The `@Name:<>` at-cue preset — byte-identical to the Rust `Default`
 *  (`brink_ir::DialogueDialect::default()` / `at_cue_preset()`). Reproduces
 *  today's hardcoded screenplay behavior exactly. */
export const AT_CUE_DIALECT: DialogueDialect = {
  version: 1,
  name: "at-cue",
  elements: [
    {
      kind: "character",
      nature: "narrative",
      source: {
        pattern: "^(?<lead>@)(?<speaker>[^:]*)(?<tail>:<>)$",
        content_group: "speaker",
        hidden: ["lead", "tail"],
        template: "@${speaker}:<>",
      },
      emitted: {
        pattern: "^@(?<speaker>[^:]*):\\s*",
        content_group: "speaker",
        reserved_prefix: true,
      },
      malformed: [
        {
          pattern: "^@[^:]*$",
          message: "Character cue is missing the ':<>' terminator",
          severity: "warning",
        },
      ],
    },
    {
      kind: "parenthetical",
      nature: "narrative",
      source: {
        // `content` (outer, parens-inclusive) drives `content_span` — the
        // parens stay visible/editable/markup-scoped content (see
        // `screenplay.ts`: "Parenthetical's leading paren is content, not
        // hidden"). `content_inner` (nested, bare) is `template_group` — the
        // group whose value fills the template placeholder, so a
        // convert/strip row targeting `parenthetical` from a bare-content
        // source round-trips correctly (#406): the literal parens live in
        // `template` itself, matching every other convert/strip consumer's
        // "Parenthetical content is the bare text between the parens"
        // convention (`@brink/ink-operations`'s `DEFAULT_CONVERTIBLE_SHAPES`,
        // the built-in `convertToParenthetical`/`stripToNarrative` actions).
        pattern: "^(?<content>\\((?<content_inner>[^)]*)\\))(?<tail><>)$",
        content_group: "content",
        template_group: "content_inner",
        hidden: ["tail"],
        template: "(${content_inner})<>",
      },
      emitted: {
        pattern: "^(?<content>\\([^)]*\\))\\s*",
        content_group: "content",
        reserved_prefix: false,
      },
      malformed: [
        {
          pattern: "^\\([^)]*\\)$",
          message: "Parenthetical is missing the '<>' terminator",
          severity: "warning",
        },
      ],
    },
    {
      kind: "dialogue",
      nature: "narrative",
    },
  ],
  chain: [
    {
      after: ["character", "parenthetical", "dialogue"],
      is: ["narrative"],
      becomes: "dialogue",
      carry: ["speaker"],
    },
  ],
  transitions: [],
  templates: {
    entries: [
      { kind: "character", label: "Character cue", picker_key: "@", blank_tab: true },
      { kind: "parenthetical", label: "Parenthetical", picker_key: "(", blank_tab: false },
    ],
  },
};

/** Add/override a kind on a dialect without forking the whole object (#368
 *  deliverable 5). `overrides.elements` are appended (or replace an existing
 *  element of the same `kind`); `chain`/`transitions` are appended;
 *  `templates.entries` are appended (or replace a same-`kind` entry). */
export function extendDialect(
  base: DialogueDialect,
  overrides: Partial<DialogueDialect>,
): DialogueDialect {
  const elements = [...(base.elements ?? [])];
  for (const el of overrides.elements ?? []) {
    const i = elements.findIndex((e) => e.kind === el.kind);
    if (i >= 0) elements[i] = el;
    else elements.push(el);
  }

  const entries = [...(base.templates?.entries ?? [])];
  for (const entry of overrides.templates?.entries ?? []) {
    const i = entries.findIndex((e) => e.kind === entry.kind);
    if (i >= 0) entries[i] = entry;
    else entries.push(entry);
  }

  return {
    version: overrides.version ?? base.version,
    name: overrides.name ?? base.name,
    elements,
    chain: [...(base.chain ?? []), ...(overrides.chain ?? [])],
    transitions: [...(base.transitions ?? []), ...(overrides.transitions ?? [])],
    templates: { entries },
  };
}

// ── DialectParser (#366): public, pure-TS parser over source/emitted text ──
//
// A standalone (no CodeMirror, no wasm) parser over a `DialogueDialect`.
// `parseSource` mirrors `element-type.ts`'s `applyDialectFallback` classify +
// chain passes exactly (same interpreter, `ResolvedDialect`), but walks plain
// source text line-by-line instead of a CM6 `EditorState`. `parseEmitted`
// walks *runtime-emitted* text (the post-glue output of `continue_line()`)
// using each declared kind's `emitted` shape, per the pinned **composite-
// segment iteration protocol**: a cue + parenthetical + text emitting as ONE
// line is the normal case (see docs/dialect-spec.md "emitted hardening").
//
// `detectCast` is the #366 answer to cast detection — it walks `parseSource`
// output and collects distinct `character`-kind speaker values. Per the
// dialect-spec ruling, `characterName()` (screenplay.ts) stays package-
// internal; this is the public replacement.

/** One classified line from `DialectParser.parseSource` — a line's dialect
 *  kind (or `null` when no element/chain rule matched) plus its captured
 *  attrs (from the winning element's non-hidden named groups, or carried
 *  forward by a chain rule) and its original 0-based line index. */
export interface SourceLine {
  index: number;
  /** The full, untrimmed source line text. */
  text: string;
  /** The dialect kind of this line (e.g. `"character"`, `"dialogue"`), or
   *  `null` if the line didn't classify (plain narrative/blank/no dialect
   *  match). */
  kind: string | null;
  /** Captured named-group attrs (e.g. `[["speaker", "Alice"]]`), sorted by
   *  attr name — empty when `kind` is `null`. */
  attrs: Array<readonly [string, string]>;
}

/** One segment of a composite emitted line from `DialectParser.parseEmitted`.
 *  `kind: null` marks a plain-text remainder segment (no declared `emitted`
 *  shape matched at that position). */
export interface EmittedSegment {
  kind: string | null;
  /** The segment's raw matched text (including any literal affixes/glue
   *  consumed by the pattern) as it appeared in the emitted line. */
  text: string;
  /** The segment's extracted content-group value, or `null` for a plain-text
   *  segment / a matched kind with no `content_group`. */
  content: string | null;
}

// A trimmed line beginning with one of these is ink STRUCTURAL syntax
// (divert, thread, tag, logic, choice/gather, knot/stitch header, comment,
// INCLUDE/EXTERNAL/VAR/CONST/LIST decl) — never dialect content. Mirrors
// `element-type.ts`'s `classifyLine` prefix set (house rule: content/geometry
// code must never treat ink syntax as content). Checked BEFORE the chain rule
// so a structural line occurring right after a classified cue does not get
// swept into the chain merely because the previous line had a `kind`.
const STRUCTURAL_LINE_PATTERN =
  /^(->|<-|#|~|\*|\+|-(?!>)|=|\/\/|\/\*|\{|INCLUDE |EXTERNAL |VAR|CONST|LIST )/;

/** Whether a (already-trimmed) source line begins with ink structural
 *  syntax — a divert/thread/tag/logic/choice/gather/header/comment/decl —
 *  and must therefore never chain into dialect content. */
function isStructuralLine(trimmed: string): boolean {
  return STRUCTURAL_LINE_PATTERN.test(trimmed);
}

/**
 * Pure-TS parser over a `DialogueDialect` — no CM6, no wasm session. Public
 * (#366 deliverable 3): construct once per dialect (patterns compiled once,
 * mirroring `ResolvedDialect`'s compile-once discipline), reuse across many
 * lines/emitted strings.
 */
export class DialectParser {
  private readonly resolved: ResolvedDialect;

  constructor(dialect: DialogueDialect) {
    this.resolved = ResolvedDialect.compile(dialect);
  }

  /**
   * Classify plain `.ink`-style source text into `SourceLine` records — one
   * per input line, in order. Mirrors `element-type.ts`'s dialect classify +
   * chain passes: an element's `source` pattern is tried against each
   * (trimmed) line in declaration order (first match wins); a narrative line
   * immediately following a classified line chains per the dialect's `chain`
   * rules (carrying forward the declared `carry` attrs), and a blank line
   * always breaks the chain. This parser does not classify structural ink
   * syntax (`->`, `<-`, `#`, `{}`, choices/gathers, headers, decls) into any
   * dialect `kind` — such a line is always `kind: null`, matching
   * `element-type.ts`'s `NarrativeText`-only chain promotion (house rule:
   * content/geometry code must never treat ink syntax as content). This
   * applies even immediately after a classified cue: a structural line
   * breaks the chain rather than being swept into it.
   */
  parseSource(source: string): SourceLine[] {
    const rawLines = source.split("\n");
    const out: SourceLine[] = [];
    let runCarry = new Map<string, string>();

    for (let i = 0; i < rawLines.length; i++) {
      const text = rawLines[i];
      const trimmed = text.trim();
      if (trimmed === "") {
        runCarry = new Map();
        out.push({ index: i, text, kind: null, attrs: [] });
        continue;
      }

      const leadingWs = text.length - text.trimStart().length;
      const match = this.resolved.classify(text.trimStart(), leadingWs);
      if (match) {
        out.push({ index: i, text, kind: match.kind, attrs: match.attrs });
        runCarry = new Map(match.attrs);
        continue;
      }

      const prev = out[i - 1];
      if (prev?.kind && !isStructuralLine(trimmed)) {
        const rule = this.resolved.chainRuleAfter(prev.kind);
        if (rule) {
          const carried: Array<[string, string]> = [];
          for (const name of rule.carry ?? []) {
            const value = runCarry.get(name);
            if (value !== undefined) carried.push([name, value]);
          }
          out.push({ index: i, text, kind: rule.becomes, attrs: carried });
          for (const [k, v] of carried) runCarry.set(k, v);
          continue;
        }
      }

      runCarry = new Map();
      out.push({ index: i, text, kind: null, attrs: [] });
    }

    return out;
  }

  /**
   * Parse ONE runtime-emitted text line (the post-glue output of
   * `continue_line()`) into its composite segments, per the pinned
   * iteration protocol:
   *
   * 1. Walk left to right over the remaining text.
   * 2. At each position, try every declared kind with an `emitted` shape, in
   *    declaration order. At position 0 (the start of the line), only
   *    `reserved_prefix: true` shapes are eligible — a non-reserved shape
   *    (e.g. a parenthetical) never opens a composite line (this is what
   *    makes `@channel: hello` and standalone `(aside)` prose fail to parse
   *    as cue/parenthetical). At any later position, both reserved and
   *    non-reserved shapes are eligible (they're always a *continuation*
   *    after the opening reserved segment).
   * 3. The first kind (in declaration order) whose `emitted` pattern matches
   *    at the current position wins; its matched text is consumed and a
   *    segment is emitted.
   * 4. If no declared kind matches at the current position, the run of
   *    remaining plain text up to (but not including) the next position at
   *    which some kind's pattern matches — or the end of the line — becomes
   *    one plain-text segment (`kind: null`).
   *
   * A cue + parenthetical + trailing text emitting as ONE `EmittedSegment[]`
   * (three segments) is the normal case this protocol pins.
   */
  parseEmitted(text: string): EmittedSegment[] {
    const segments: EmittedSegment[] = [];
    let pos = 0;
    let first = true;

    while (pos < text.length) {
      const rest = text.slice(pos);
      const hit = this.matchEmittedAt(rest, first);
      if (hit) {
        segments.push(hit.segment);
        pos += hit.length;
        first = false;
        continue;
      }

      // No declared kind matches here — grow a plain-text segment up to the
      // next position where one does (or end of string).
      let end = text.length;
      for (let p = pos + 1; p < text.length; p++) {
        if (this.matchEmittedAt(text.slice(p), false)) {
          end = p;
          break;
        }
      }
      segments.push({ kind: null, text: text.slice(pos, end), content: null });
      pos = end;
      first = false;
    }

    return segments;
  }

  private matchEmittedAt(
    rest: string,
    atStart: boolean,
  ): { segment: EmittedSegment; length: number } | null {
    for (const decl of this.resolved.raw().elements ?? []) {
      const emitted = decl.emitted;
      if (!emitted) continue;
      if (atStart && !emitted.reserved_prefix) continue;
      const re = new RegExp(emitted.pattern);
      const m = re.exec(rest);
      if (!m || m.index !== 0) continue;
      const content =
        emitted.content_group && m.groups ? (m.groups[emitted.content_group] ?? null) : null;
      return {
        segment: { kind: decl.kind, text: m[0], content },
        length: m[0].length,
      };
    }
    return null;
  }
}

/**
 * Detect the distinct cast (speaker names) from already-classified source
 * lines — the #366 answer to cast detection (`characterName()` stays
 * package-internal; this is the public replacement).
 *
 * Dialect-agnostic: a "speaker" attr is whichever named group a chain rule
 * `carry`s forward onto a chained run (in the shipped at-cue preset, that's
 * `speaker`, carried from `character` onto `dialogue`). This covers both the
 * cue line itself (which captures the attr directly via its content group)
 * and any line whose kind was produced by chaining (which carries the attr
 * without its own content group). Accepts `DialectParser.parseSource`'s
 * output directly. Returns names in first-appearance order, deduplicated; an
 * empty captured value (e.g. `@:<>`) is skipped (no speaker name to report).
 */
export function detectCast(lines: readonly SourceLine[], dialect: DialogueDialect): string[] {
  const carriedAttrs = new Set<string>();
  for (const rule of dialect.chain ?? []) {
    for (const name of rule.carry ?? []) carriedAttrs.add(name);
  }
  if (carriedAttrs.size === 0) return [];

  const seen = new Set<string>();
  const cast: string[] = [];
  for (const line of lines) {
    if (!line.kind) continue;
    for (const [attr, value] of line.attrs) {
      if (!carriedAttrs.has(attr) || !value || seen.has(value)) continue;
      seen.add(value);
      cast.push(value);
    }
  }
  return cast;
}
