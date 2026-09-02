/**
 * Dialogue-dialect diagnostics (#3391): two sources, one Problems bucket.
 *
 * 1. `brink.toml [dialogue]` validation — the session's config warnings
 *    already carry the resolver's readable messages (unknown preset, bad
 *    element shape, missing artifact file…); they were Output-log only.
 *    They become Problems rows keyed to `brink.toml` so a broken
 *    convention declaration fails loudly at compile time.
 * 2. The dialect's own `malformed` near-miss rules — "a cue missing its
 *    terminator" — evaluated on story lines the dialect classified as
 *    plain narrative (a line the dialect DID match is by definition not
 *    malformed). Designed in the spec; this is the first consumer.
 */
import { DialectParser, type DialogueDialect } from "@brink-lang/editor";
import type { Diagnostic } from "@brink/wasm-types";

export const CONFIG_FILE = "brink.toml";
const DIALOGUE_CONFIG_CODE = "dialogue:config";
const DIALOGUE_MALFORMED_CODE = "dialogue:malformed";

/** `brink.toml` rows from the session's config warnings + a discovery
 *  error. A `[dialogue]:`-prefixed warning is the resolver refusing the
 *  declaration → Error; any other warning (unknown key, …) → Warning; a
 *  discovery error (malformed TOML, wrong type) → Error. Offsets are 0:
 *  the config parser reports keys, not spans. */
export function configDiagnostics(
  warnings: readonly string[],
  error: string | null,
): Diagnostic[] {
  const rows: Diagnostic[] = warnings.map((w) => ({
    start: 0,
    end: 0,
    message: w,
    severity: w.startsWith("[dialogue]") ? ("Error" as const) : ("Warning" as const),
    code: DIALOGUE_CONFIG_CODE,
    file: CONFIG_FILE,
  }));
  if (error !== null) {
    rows.push({
      start: 0,
      end: 0,
      message: error,
      severity: "Error",
      code: DIALOGUE_CONFIG_CODE,
      file: CONFIG_FILE,
    });
  }
  return rows;
}

/** Near-miss diagnostics for one file: each narrative-classified line is
 *  tried against every element's `malformed` rules (first hit wins). */
export function malformedCueDiagnostics(
  file: string,
  source: string,
  dialect: DialogueDialect,
): Diagnostic[] {
  const rules = (dialect.elements ?? []).flatMap((el) =>
    (el.malformed ?? []).map((m) => ({ kind: el.kind, ...m })),
  );
  if (rules.length === 0) return [];
  const compiled = rules.map((r) => ({ ...r, re: new RegExp(r.pattern) }));
  const parser = new DialectParser(dialect);
  const classified = parser.parseSource(source);
  const out: Diagnostic[] = [];
  let offset = 0;
  const lines = source.split("\n");
  lines.forEach((text, i) => {
    const kind = classified[i]?.kind ?? null;
    const trimmed = text.trim();
    if (kind === null && trimmed.length > 0) {
      const lead = text.length - text.trimStart().length;
      for (const rule of compiled) {
        if (rule.re.test(trimmed)) {
          out.push({
            start: offset + lead,
            end: offset + lead + trimmed.length,
            message: rule.message,
            severity: rule.severity === "error" ? "Error" : "Warning",
            code: DIALOGUE_MALFORMED_CODE,
            file,
          });
          break;
        }
      }
    }
    offset += text.length + 1;
  });
  return out;
}
