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
   *  `validateDialect`). Compiled with the `d` flag so match indices
   *  (per-named-group spans) are available on every match without
   *  recompiling or re-matching. */
  static compile(dialect: DialogueDialect): ResolvedDialect {
    const elements: ResolvedElement[] = [];
    for (const decl of dialect.elements ?? []) {
      if (decl.source) {
        const shape = resolveSourceShape(decl.source);
        const re = new RegExp(shape.pattern, "d");
        elements.push({ decl, re, shape });
      } else {
        elements.push({ decl, re: null, shape: null });
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
      if (m) return buildMatch(el.decl.kind, m, el.shape, leadingWs);
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

  /** The `content_group` name for a declared kind (e.g. `"speaker"` for
   *  `character`) — the named placeholder in that kind's `template` a
   *  `convert` transition action's extracted content fills. `null` for a
   *  pattern-less (chain-only) or unknown kind. */
  contentGroupFor(kind: string): string | null {
    const el = this.elements.find((e) => e.decl.kind === kind);
    return el?.shape?.content_group ?? null;
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
   */
  convertibleShapes(): ConvertibleShape[] {
    const shapes: ConvertibleShape[] = [];
    for (const el of this.elements) {
      if (!el.shape) continue;
      shapes.push({ pattern: el.shape.pattern, contentGroup: el.shape.content_group ?? null });
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
): DialectMatch {
  const groups = m.groups ?? {};
  const groupIndices = (m as IndexedMatch).indices?.groups ?? {};
  const hidden = new Set(shape.hidden ?? []);
  const attrs: Array<[string, string]> = [];
  const hiddenSpans: Array<[number, number]> = [];
  let contentSpan: [number, number] | null = null;

  for (const name of Object.keys(groups)) {
    const span = groupIndices[name];
    if (!span) continue;
    const [s, e] = span;
    if (hidden.has(name)) {
      hiddenSpans.push([leadingWs + s, leadingWs + e]);
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
        pattern: "^(?<content>\\([^)]*\\))(?<tail><>)$",
        content_group: "content",
        hidden: ["tail"],
        template: "${content}<>",
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
