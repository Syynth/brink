/**
 * Extensible inline-markup rules (#367).
 *
 * The editor owns WHERE markup may live; hosts own WHAT it looks like. A host
 * registers `InlineMarkupRule`s (a single `pattern`, or an `open`/`close` pair
 * for wrapping spans) and gets back a CM6 extension that decorates matches as
 * `brink-markup-<name>` mark decorations, with `data-*` attributes lifted from
 * named capture groups. The pair form additionally classes the wrapped content
 * so a host can dim the tags and restyle the inside without the editor knowing
 * what the tag means.
 *
 * CRITICAL invariant — content-region scoping: rules run only within the
 * content text of narrative-natured lines (per the element classification in
 * `element-type.ts`) and never over ink syntax. Concretely:
 *
 *  - non-narrative lines (headers, diverts, logic, VAR/CONST/LIST, comments,
 *    INCLUDE/EXTERNAL, tag lines, blanks) are skipped entirely;
 *  - thread lines (`<- target`) are skipped entirely;
 *  - choice/gather sigil prefixes (`* * *`, `- -`) are excluded — this also
 *    keeps rule marks clear of the depth-widget replace decorations that the
 *    screenplay pass applies over the same prefix;
 *  - the hidden screenplay sigils (`@`/`:<>` on character lines, trailing `<>`
 *    on parentheticals) are excluded, coordinating with the sigil-hiding
 *    replace decorations and atomic ranges;
 *  - content is truncated at a divert arrow (`->`) — the target is syntax;
 *  - glue `<>` and mid-line thread `<-` tokens split content regions, so
 *    `<wave>` tokenizes but `<>` never does, for any host regex;
 *  - choice brackets `[`/`]` split content regions on choice lines, so a match
 *    can never span over a bracket (the bracket pass keeps its own marks; text
 *    inside the brackets is still content and still matchable).
 *
 * A rule match can never cross a region boundary because each rule executes
 * against the region's text slice, not the raw line.
 *
 * Ships ZERO rules by default — `inlineMarkup([])` is inert. The RMMZ-style
 * angle-tag rule is exported as an optional preset (`rmmzAngleTagRule`). All
 * styling is host-side: classes and data attributes only, no inline styles
 * (consistent with #363).
 */

import { type Extension, type Range } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  type EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import { elementTypeField, ElementType } from "./element-type.js";
import { CHAR_SUFFIX_LEN, GLUE_LEN, leadingWsLen } from "./screenplay.js";

// ── Public rule shapes ─────────────────────────────────────────────

/** Single-pattern rule: every match decorates as `brink-markup-<name>`. */
export interface InlineMarkupPatternRule {
  /** Rule name; becomes the `brink-markup-<name>` class (sanitized). */
  name: string;
  /**
   * Match pattern. Named capture groups surface as `data-*` attributes on the
   * decoration (`(?<tag>…)` → `data-tag="…"`; `_` in a group name becomes `-`).
   */
  pattern: RegExp;
}

/** Pair rule: `open`…`close` spans; the wrapped content is classed too. */
export interface InlineMarkupPairRule {
  /** Rule name; becomes the `brink-markup-<name>` class (sanitized). */
  name: string;
  /** Opening-token pattern. Named groups → `data-*` attributes. */
  open: RegExp;
  /** Closing-token pattern. Named groups → `data-*` attributes. */
  close: RegExp;
  /**
   * Class for the text wrapped between an open/close pair. Defaults to
   * `brink-markup-<name>-content`.
   */
  contentClass?: string;
}

export type InlineMarkupRule = InlineMarkupPatternRule | InlineMarkupPairRule;

// ── Content-region scoping ─────────────────────────────────────────

/** A line-relative half-open span `[from, to)` of narrative content text. */
export interface MarkupRegion {
  from: number;
  to: number;
}

/** Line element types whose content text may carry inline markup. */
const NARRATIVE_TYPES: ReadonlySet<ElementType> = new Set([
  ElementType.NarrativeText,
  ElementType.Choice,
  ElementType.ChoiceBody,
  ElementType.Gather,
  ElementType.Character,
  ElementType.Parenthetical,
  ElementType.Dialogue,
]);

/** Skip past a choice/gather sigil prefix (sigils + interleaved spaces). */
function sigilPrefixEnd(text: string, ws: number, validSigils: readonly string[]): number {
  let pos = ws;
  let seen = false;
  while (pos < text.length) {
    // A `-` that begins a divert arrow (`->`) or sits before a thread (`<-`)
    // is not a gather sigil — `- -> END` is a gather followed by a divert.
    if (validSigils.includes(text[pos]) && !text.startsWith("->", pos)) {
      seen = true;
      pos++;
      while (pos < text.length && text[pos] === " ") pos++;
    } else {
      break;
    }
  }
  return seen ? pos : ws;
}

/**
 * Compute the narrative content regions of a line (line-relative offsets).
 * Returns `[]` for lines that carry no markupable content. Pure — exported as
 * the unit-testable core of the scoping invariant.
 */
export function contentRegions(text: string, type: ElementType): MarkupRegion[] {
  if (!NARRATIVE_TYPES.has(type)) return [];

  const ws = leadingWsLen(text);
  const trimmed = text.slice(ws);

  // Thread lines are pure ink syntax (`<- target`), never content.
  if (trimmed.startsWith("<-")) return [];

  let start = ws;
  let end = text.length;
  const isChoice = type === ElementType.Choice;

  switch (type) {
    case ElementType.Character:
      // `@Name:<>` — content is the name between the hidden @ and :<> sigils.
      start = ws + 1;
      end = Math.max(start, text.length - CHAR_SUFFIX_LEN);
      break;
    case ElementType.Parenthetical:
      // `(text)<>` — exclude the hidden trailing glue.
      end = Math.max(start, text.length - GLUE_LEN);
      break;
    case ElementType.Choice:
      start = sigilPrefixEnd(text, ws, ["*", "+"]);
      break;
    case ElementType.Gather:
      start = sigilPrefixEnd(text, ws, ["-"]);
      break;
    default:
      break;
  }

  // Truncate at a divert arrow — everything from `->` on is target syntax.
  const divert = text.indexOf("->", start);
  if (divert >= 0 && divert < end) end = divert;

  // Split at ink syntax tokens: glue `<>`, inline logic `{…}` (excluded
  // wholesale, nesting-aware), and (on choice lines) the `[` / `]` bracket
  // characters. A mid-line thread `<-` or an unescaped tag `#` ends the
  // content entirely — everything after is target/tag syntax. A rule match
  // can never span a split because each region is matched independently.
  const regions: MarkupRegion[] = [];
  let cur = start;
  let i = start;
  let braceDepth = 0;
  while (i < end) {
    if (braceDepth > 0) {
      if (text[i] === "{") braceDepth++;
      else if (text[i] === "}") {
        braceDepth--;
        if (braceDepth === 0) cur = i + 1;
      }
      i++;
      continue;
    }
    if (text.startsWith("<-", i) || (text[i] === "#" && text[i - 1] !== "\\")) {
      end = i;
      break;
    }
    if (text[i] === "{") {
      if (i > cur) regions.push({ from: cur, to: i });
      braceDepth = 1;
      i++;
      continue;
    }
    let tokenLen = 0;
    if (text.startsWith("<>", i)) tokenLen = 2;
    else if (isChoice && (text[i] === "[" || text[i] === "]")) tokenLen = 1;

    if (tokenLen > 0) {
      if (i > cur) regions.push({ from: cur, to: i });
      i += tokenLen;
      cur = i;
    } else {
      i++;
    }
  }
  if (end > cur && braceDepth === 0) regions.push({ from: cur, to: end });

  return regions;
}

// ── Rule compilation ───────────────────────────────────────────────

interface CompiledPattern {
  kind: "pattern";
  className: string;
  regex: RegExp;
}

interface CompiledPair {
  kind: "pair";
  className: string;
  open: RegExp;
  close: RegExp;
  contentClass: string;
}

type CompiledRule = CompiledPattern | CompiledPair;

/** Rule names become class names; keep them CSS-safe. */
function sanitizeName(name: string): string {
  return name.replace(/[^\w-]+/g, "-");
}

/** Clone a host regex, guaranteeing the `g` flag so `exec` iterates. */
function globalized(re: RegExp): RegExp {
  return re.global ? new RegExp(re.source, re.flags) : new RegExp(re.source, re.flags + "g");
}

function compileRule(rule: InlineMarkupRule): CompiledRule {
  const className = `brink-markup-${sanitizeName(rule.name)}`;
  if ("pattern" in rule) {
    return { kind: "pattern", className, regex: globalized(rule.pattern) };
  }
  return {
    kind: "pair",
    className,
    open: globalized(rule.open),
    close: globalized(rule.close),
    contentClass: rule.contentClass ?? `${className}-content`,
  };
}

/** Named capture groups → `data-*` attributes (`_` → `-`, lowercased). */
function dataAttributes(match: RegExpExecArray): Record<string, string> | undefined {
  const groups = match.groups;
  if (!groups) return undefined;
  let attrs: Record<string, string> | undefined;
  for (const key of Object.keys(groups)) {
    const value = groups[key];
    if (value === undefined) continue;
    attrs ??= {};
    attrs[`data-${key.toLowerCase().replace(/_/g, "-")}`] = value;
  }
  return attrs;
}

// ── Decoration building ────────────────────────────────────────────

function markFor(className: string, match: RegExpExecArray): Decoration {
  return Decoration.mark({ class: className, attributes: dataAttributes(match) });
}

function matchPattern(
  rule: CompiledPattern,
  slice: string,
  base: number,
  out: Range<Decoration>[],
): void {
  rule.regex.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = rule.regex.exec(slice)) !== null) {
    if (m[0].length === 0) {
      rule.regex.lastIndex++;
      continue;
    }
    out.push(markFor(rule.className, m).range(base + m.index, base + m.index + m[0].length));
  }
}

function matchPair(
  rule: CompiledPair,
  slice: string,
  base: number,
  out: Range<Decoration>[],
): void {
  // Token spans claimed by this rule (open, close, or unpaired-open), so the
  // trailing unpaired-close sweep decorates only genuinely dangling closes.
  const claimed: Array<{ from: number; to: number }> = [];
  rule.open.lastIndex = 0;
  let om: RegExpExecArray | null;
  while ((om = rule.open.exec(slice)) !== null) {
    if (om[0].length === 0) {
      rule.open.lastIndex++;
      continue;
    }
    const openFrom = om.index;
    const openTo = om.index + om[0].length;

    rule.close.lastIndex = openTo;
    const cm = rule.close.exec(slice);
    if (cm !== null && cm[0].length > 0) {
      const closeFrom = cm.index;
      const closeTo = cm.index + cm[0].length;
      out.push(markFor(rule.className, om).range(base + openFrom, base + openTo));
      if (closeFrom > openTo) {
        out.push(
          Decoration.mark({ class: rule.contentClass }).range(base + openTo, base + closeFrom),
        );
      }
      out.push(markFor(rule.className, cm).range(base + closeFrom, base + closeTo));
      claimed.push({ from: openFrom, to: openTo }, { from: closeFrom, to: closeTo });
      rule.open.lastIndex = closeTo;
    } else {
      // Unpaired open token: still an inert literal — decorate it alone.
      out.push(markFor(rule.className, om).range(base + openFrom, base + openTo));
      claimed.push({ from: openFrom, to: openTo });
      rule.open.lastIndex = openTo;
    }
  }

  // Unpaired close tokens are inert literals too — decorate them alone,
  // symmetric with the unpaired-open contract.
  rule.close.lastIndex = 0;
  let cm2: RegExpExecArray | null;
  while ((cm2 = rule.close.exec(slice)) !== null) {
    if (cm2[0].length === 0) {
      rule.close.lastIndex++;
      continue;
    }
    const from = cm2.index;
    const to = cm2.index + cm2[0].length;
    if (!claimed.some((c) => from < c.to && to > c.from)) {
      out.push(markFor(rule.className, cm2).range(base + from, base + to));
    }
  }
}

function buildMarkupDecos(view: EditorView, rules: readonly CompiledRule[]): DecorationSet {
  const infos = view.state.field(elementTypeField);
  const ranges: Range<Decoration>[] = [];

  for (let i = 1; i <= view.state.doc.lines; i++) {
    const info = infos[i - 1];
    if (!info) continue;
    const line = view.state.doc.line(i);
    const regions = contentRegions(line.text, info.type);
    for (const region of regions) {
      const slice = line.text.slice(region.from, region.to);
      const base = line.from + region.from;
      for (const rule of rules) {
        if (rule.kind === "pattern") matchPattern(rule, slice, base, ranges);
        else matchPair(rule, slice, base, ranges);
      }
    }
  }

  // Rules may interleave/overlap, so sort rather than requiring build order.
  return Decoration.set(ranges, true);
}

// ── Extension factory ──────────────────────────────────────────────

/**
 * Build the inline-markup extension from host-registered rules. Zero rules →
 * an inert (empty) extension. Matches decorate as `brink-markup-<name>` marks
 * with `data-*` attributes from named capture groups; the pair form also
 * classes the wrapped content. Styling is entirely host-side.
 */
export function inlineMarkup(rules: InlineMarkupRule[]): Extension {
  const compiled = rules.map(compileRule);
  if (compiled.length === 0) return [];

  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = buildMarkupDecos(view, compiled);
      }

      update(update: ViewUpdate) {
        // Marks are doc-wide and derive only from the doc + elementTypeField
        // (which recomputes on docChanged), so a viewport-only scroll keeps
        // the existing set — same policy as the screenplay passes.
        if (update.docChanged) {
          this.decorations = buildMarkupDecos(update.view, compiled);
        }
      }
    },
    { decorations: (v) => v.decorations },
  );

  return [elementTypeField, plugin];
}

// ── Presets ────────────────────────────────────────────────────────

/**
 * RMMZ-style angle-tag rule (optional preset; NOT enabled by default).
 * Matches `<wave>`, `</wave>`, `<color=3>` … and surfaces `data-tag` /
 * `data-value` from the capture groups. Glue `<>` never matches — the
 * content-region scoping strips it before rules run.
 */
export const rmmzAngleTagRule: InlineMarkupRule = {
  name: "rmmz-tag",
  pattern: /<\/?(?<tag>[a-z]\w*)(?:=(?<value>[^>\s]+))?>/gi,
};
