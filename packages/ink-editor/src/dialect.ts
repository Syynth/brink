/**
 * Dialogue dialect — re-exported from `@brink-lang/dialect` (#3393, RULED
 * 2026-08-30 "Engines consume the RESOLVED dialect as a compile output"):
 * the artifact, validator, parser, overlay, cast detection and the run
 * rule live in that pure-TS package so a game engine can import them
 * without this editor package. Everything the editor exported before is
 * still exported from here, unchanged.
 *
 * The one editor-coupled piece stays local: `convertibleShapesOf`, the
 * convert/strip geometry `@brink/ink-operations` consumes.
 */
export * from "@brink-lang/dialect";

import type { ResolvedDialect } from "@brink-lang/dialect";
import type { ConvertibleShape } from "@brink/ink-operations";

/**
 * The `ConvertibleShape`s of a resolved dialect's pattern-bearing kinds —
 * what the built-in convert/strip actions match against. Uses
 * `template_group` (falling back to `content_group`) for the extracted
 * group (#406): a kind's `content_group` may be wrap-inclusive for
 * `content_span` geometry purposes (e.g. `parenthetical`), but a
 * convert/strip round-trip needs the bare value `template` itself wraps —
 * matching `DEFAULT_CONVERTIBLE_SHAPES`'s and the built-in
 * `convertToParenthetical`/`stripToNarrative` actions' "Parenthetical
 * content is the bare text between the parens" convention.
 */
export function convertibleShapesOf(dialect: ResolvedDialect): ConvertibleShape[] {
  const shapes: ConvertibleShape[] = [];
  for (const el of dialect.elements) {
    if (!el.shape) continue;
    shapes.push({
      pattern: el.shape.pattern,
      contentGroup: el.shape.template_group ?? el.shape.content_group ?? null,
    });
  }
  return shapes;
}
