/**
 * The `[dialogue]` table of `brink.toml`, as TypeScript — and the resolver
 * that turns it into a `DialogueDialect`, mirroring
 * `brink_ide::dialect_config::resolve_dialogue_config` and the affix
 * builders in `brink_ir::dialect` (`affix_element`, `emitted_for_affix`).
 *
 * Why a mirror exists at all: the Conventions editor (#3392) proposes a
 * configuration and must show the author what it *means* — classified
 * lines, Player runs — before anything is written and before the compiler
 * has seen it. That preview has to resolve the table exactly the way the
 * Rust resolver will, or the preview lies.
 *
 * The table form is deliberately narrower than the artifact (no chain
 * rules, no emitted shape for pattern elements): `toDialogueConfig` says
 * whether a dialect fits it, and the editor falls back to the file form
 * (`[dialogue] file = "…"`, the ruled escape hatch) when it does not.
 */

import type { AffixShape, DialectElement, DialogueDialect, ElementNature, EmittedShape } from "@brink/wasm-types";
import { AT_CUE_DIALECT, extendDialect } from "./index.js";

/** One `[[dialogue.elements]]` row. Keys as written in TOML. */
export interface DialogueElementConfig {
  kind: string;
  nature?: ElementNature;
  prefix?: string;
  suffix?: string;
  glued?: boolean;
  /** `content-role` — the named content group (`"content"` by default). */
  contentRole?: string;
  /** Explicit pattern form (`pattern` + `template`), for kinds the affix sugar cannot express. */
  pattern?: string;
  template?: string;
}

/** The `[dialogue]` table (table form only — the file form is a string). */
export interface DialogueConfig {
  preset?: string;
  /** `run-ends-at` — the emitted-side run rule. */
  runEndsAt?: string[];
  elements?: DialogueElementConfig[];
}

/** The shipped preset names (mirrors `PRESET_NAMES`). */
export const PRESET_NAMES: readonly string[] = ["at-cue"];

/** A shipped preset by its `preset = "…"` name (mirrors `preset_by_name`).
 *  A function, not a table: `index.ts` re-exports this module, so a
 *  module-level read of `AT_CUE_DIALECT` would run before it exists. */
export function presetByName(name: string): DialogueDialect | null {
  return name === "at-cue" ? AT_CUE_DIALECT : null;
}

// Mirrors Rust `regex::escape`: every ASCII meta character, including the
// ones JS would let through (`#`, `&`, `~`, `-`, space is not escaped by
// either). String parity matters because `toDialogueConfig` compares a
// resolved dialect against the artifact the inference built.
function escapeRegexLiteralRust(s: string): string {
  return s.replace(/[\\.+*?()|[\]{}^$#&\-~]/g, "\\$&");
}

function escapeClassChar(c: string): string {
  return c === "]" || c === "\\" || c === "^" || c === "-" ? `\\${c}` : c;
}

/** Mirrors `emitted_for_affix` in dialect.rs. */
export function emittedForAffix(affix: AffixShape): EmittedShape {
  const prefix = affix.prefix ?? "";
  const suffix = affix.suffix ?? "";
  const role = affix.content_role ?? "content";
  let pattern = "^";
  if (prefix !== "") {
    pattern += escapeRegexLiteralRust(prefix) + "\\s*";
  }
  if (suffix === "") {
    pattern += `(?<${role}>.*)$`;
  } else {
    pattern += `(?<${role}>[^${escapeClassChar(suffix[0])}]*)`;
    pattern += escapeRegexLiteralRust(suffix) + "\\s*";
  }
  return { pattern, content_group: role, reserved_prefix: prefix !== "" };
}

/** Mirrors `affix_element` in dialect.rs: an element whose source is affix
 *  sugar and whose emitted shape is derived from the same affix. */
export function affixElement(kind: string, nature: ElementNature, affix: AffixShape): DialectElement {
  return {
    kind,
    nature,
    source: { ...affix },
    emitted: emittedForAffix(affix),
    malformed: [],
  };
}

function elementFromConfig(el: DialogueElementConfig): DialectElement {
  const nature = el.nature ?? "narrative";
  const role = el.contentRole ?? "content";
  if (el.pattern !== undefined || el.template !== undefined) {
    if (el.pattern === undefined || el.template === undefined) {
      throw new Error(`dialogue element \`${el.kind}\`: \`pattern\` and \`template\` go together`);
    }
    if (el.prefix !== undefined || el.suffix !== undefined || el.glued !== undefined) {
      throw new Error(
        `dialogue element \`${el.kind}\`: use EITHER the affix keys (prefix/suffix/glued) OR pattern/template, not both`,
      );
    }
    return {
      kind: el.kind,
      nature,
      source: { pattern: el.pattern, content_group: role, template_group: null, hidden: [], template: el.template },
      emitted: null,
      malformed: [],
    };
  }
  if (el.prefix === undefined && el.suffix === undefined) {
    return { kind: el.kind, nature, source: null, emitted: null, malformed: [] };
  }
  return affixElement(el.kind, nature, {
    prefix: el.prefix ?? null,
    suffix: el.suffix ?? null,
    glued: el.glued ?? false,
    content_role: role,
  });
}

/**
 * Resolve a `[dialogue]` table to the dialect the compiler will use —
 * mirrors `resolve_dialogue_config` (table form; the file form is read by
 * the caller). Throws with the resolver's own messages on a bad table.
 */
export function dialectFromConfig(config: DialogueConfig): DialogueDialect {
  let base: DialogueDialect;
  if (config.preset !== undefined) {
    const preset = presetByName(config.preset);
    if (preset === null) {
      throw new Error(
        `unknown dialogue preset \`${config.preset}\` — shipped presets: ${PRESET_NAMES.join(", ")}`,
      );
    }
    base = preset;
  } else {
    base = { version: 1, name: "project", elements: [], chain: [], transitions: [], templates: { entries: [] } };
  }
  // No `name` on the overlay: Rust's `extend_dialect` keeps the base name
  // for an empty override, TS's `extendDialect` would take "" literally.
  const overlay: DialogueDialect = {
    version: 1,
    elements: (config.elements ?? []).map(elementFromConfig),
    chain: [],
    transitions: [],
    templates: { entries: [] },
  };
  const merged = extendDialect(base, overlay);
  const runEndsAt = config.runEndsAt ?? [];
  if (runEndsAt.length > 0) {
    if ((merged.chain ?? []).length === 0) {
      throw new Error(
        "`dialogue.run-ends-at` needs a chain rule to apply to — this dialect declares none (no preset, and no chain in the artifact)",
      );
    }
    merged.chain = (merged.chain ?? []).map((r) => ({ ...r, run_ends_at: [...runEndsAt] }));
  }
  return merged;
}

/** Key-sorted JSON, so two dialects compare by content. */
export function canonicalDialectJson(d: DialogueDialect): string {
  const sort = (v: unknown): unknown => {
    if (Array.isArray(v)) return v.map(sort);
    if (v && typeof v === "object") {
      const out: Record<string, unknown> = {};
      for (const k of Object.keys(v as Record<string, unknown>).sort()) {
        const x = (v as Record<string, unknown>)[k];
        if (x === undefined || x === null) continue;
        if (Array.isArray(x) && x.length === 0 && k !== "elements") continue;
        out[k] = sort(x);
      }
      return out;
    }
    return v;
  };
  return JSON.stringify(sort(d));
}

/**
 * The `[dialogue]` table that resolves to exactly `dialect`, or `null` when
 * the table form cannot express it (a chain rule the preset does not carry,
 * a pattern element that needs an emitted shape, …). Tried against every
 * preset; the first that fits wins. Verified, not inferred: the candidate
 * table is resolved through `dialectFromConfig` and compared by content.
 */
export function toDialogueConfig(dialect: DialogueDialect): DialogueConfig | null {
  const want = canonicalDialectJson(dialect);
  const runEndsAt = dialect.chain?.[0]?.run_ends_at;
  for (const name of PRESET_NAMES) {
    const preset = presetByName(name);
    if (preset === null) continue;
    const overlays: DialogueElementConfig[] = [];
    let fits = true;
    for (const el of dialect.elements ?? []) {
      const same = preset.elements?.find((p) => p.kind === el.kind);
      if (same && canonicalDialectJson({ version: 1, elements: [same] }) === canonicalDialectJson({ version: 1, elements: [el] })) {
        continue;
      }
      const row = affixRowFor(el);
      if (row === null) {
        fits = false;
        break;
      }
      overlays.push(row);
    }
    if (!fits) continue;
    const config: DialogueConfig = { preset: name };
    if (overlays.length > 0) config.elements = overlays;
    if (runEndsAt && runEndsAt.length > 0) config.runEndsAt = [...runEndsAt];
    try {
      if (canonicalDialectJson(dialectFromConfig(config)) === want) return config;
    } catch {
      // A preset the table cannot combine with — try the next.
    }
  }
  return null;
}

/** The `[[dialogue.elements]]` row for an element the affix sugar can express. */
function affixRowFor(el: DialectElement): DialogueElementConfig | null {
  if ((el.malformed ?? []).length > 0) return null;
  if (!el.source) {
    if (el.emitted) return null;
    return el.nature === "narrative" ? { kind: el.kind } : { kind: el.kind, nature: el.nature };
  }
  if ("pattern" in el.source) return null;
  const affix = el.source;
  const expected = emittedForAffix(affix);
  if (!el.emitted || canonicalDialectJson({ version: 1, elements: [{ ...el, emitted: expected }] }) !== canonicalDialectJson({ version: 1, elements: [el] })) {
    return null;
  }
  const row: DialogueElementConfig = { kind: el.kind };
  if (el.nature !== "narrative") row.nature = el.nature;
  if (affix.prefix) row.prefix = affix.prefix;
  if (affix.suffix) row.suffix = affix.suffix;
  if (affix.glued) row.glued = true;
  if (affix.content_role && affix.content_role !== "content") row.contentRole = affix.content_role;
  return row;
}
