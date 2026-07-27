/**
 * Pure (wasm-free) dispatch + caching logic for
 * `StoryRunnerHandle.evaluate()` (speculative eval, F4.3 Tier-0 + F5.1
 * Tier-1). Kept in its own module — no `brink-web` import — so the
 * classification, content-hash, and cache-eviction logic is unit-testable
 * without loading the wasm binary. `index.ts` composes these with the wasm
 * runtime.
 */

/** A value that can cross the ink↔JS external-binding boundary. Defined here
 * (the wasm-free module) and re-exported from `index.ts` so both share one
 * definition without `index.ts`'s wasm import leaking into this module. */
export type ExternalValue = number | boolean | string | null;

// ── Tier-0 source classification ────────────────────────────────────

/** How `evaluate()`'s `source` was classified. `"path"`/`"call"` are the
 * two Tier-0 "invoke existing" cases that run without compiling anything;
 * `"invalid"` is not an error but "not Tier-0-shaped" — `evaluate()` routes
 * it to Tier-1 fragment compilation. */
export type ParsedEvaluateSource =
  | { kind: "path"; path: string }
  | { kind: "call"; name: string; args: ExternalValue[] }
  | { kind: "invalid" };

/** A bare dotted identifier path (`"cellar.intro"`) — no parens, no
 * operators; the Tier-0 "invoke existing by path" case. */
const EVALUATE_PATH_RE = /^[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*$/;

/** `name(args)` — captures the callee name and the raw argument text. */
const EVALUATE_CALL_RE = /^([A-Za-z_][A-Za-z0-9_]*)\((.*)\)$/s;

/** Parse `evaluate()`'s `source` into a knot/stitch path or a literal-arg
 * function call — the two Tier-0 "invoke existing" cases (see
 * `docs/scoped-flow-state-spec.md`'s watch/eval tiering) that run without
 * compiling anything. Anything else (an arbitrary expression, content, a
 * call with a non-literal argument) is `"invalid"` here — not an error, just
 * not Tier-0-shaped — and `evaluate()` routes it to Tier-1 fragment
 * compilation instead (`evaluateFragment`). */
export function parseEvaluateSource(source: string): ParsedEvaluateSource {
  const trimmed = source.trim();
  if (EVALUATE_PATH_RE.test(trimmed)) {
    return { kind: "path", path: trimmed };
  }
  const call = EVALUATE_CALL_RE.exec(trimmed);
  if (call) {
    const [, name, argsText] = call;
    const args = parseLiteralArgList(argsText);
    if (args === undefined) {
      return { kind: "invalid" };
    }
    return { kind: "call", name, args };
  }
  return { kind: "invalid" };
}

const UNPARSEABLE_LITERAL = Symbol("evaluate: unparseable literal argument");

/** Parse a comma-separated literal argument list (`"1, \"x\", true"`).
 * `undefined` if any argument isn't a recognized literal. */
function parseLiteralArgList(argsText: string): ExternalValue[] | undefined {
  const trimmed = argsText.trim();
  if (trimmed === "") {
    return [];
  }
  const values: ExternalValue[] = [];
  for (const part of splitTopLevelArgs(trimmed)) {
    const value = parseLiteralArg(part.trim());
    if (value === UNPARSEABLE_LITERAL) {
      return undefined;
    }
    values.push(value);
  }
  return values;
}

/** Split on top-level commas, respecting (only) quoted-string boundaries —
 * literal arguments never nest parens or brackets. */
function splitTopLevelArgs(text: string): string[] {
  const parts: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (quote) {
      current += ch;
      if (ch === quote && text[i - 1] !== "\\") {
        quote = null;
      }
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      current += ch;
      continue;
    }
    if (ch === ",") {
      parts.push(current);
      current = "";
      continue;
    }
    current += ch;
  }
  if (current.trim() !== "") {
    parts.push(current);
  }
  return parts;
}

function parseLiteralArg(text: string): ExternalValue | typeof UNPARSEABLE_LITERAL {
  if (text === "true") {
    return true;
  }
  if (text === "false") {
    return false;
  }
  if (text === "null") {
    return null;
  }
  if (/^-?\d+$/.test(text)) {
    return parseInt(text, 10);
  }
  if (/^-?\d+\.\d+$/.test(text)) {
    return parseFloat(text);
  }
  const stringMatch = /^"((?:[^"\\]|\\.)*)"$|^'((?:[^'\\]|\\.)*)'$/.exec(text);
  if (stringMatch) {
    const raw = stringMatch[1] ?? stringMatch[2] ?? "";
    return raw.replace(/\\(.)/g, "$1");
  }
  return UNPARSEABLE_LITERAL;
}

// ── Tier-1 wrap-syntax selection (#1598) ────────────────────────────

/** True when `entry`'s final path segment carries the native `.brink`
 * extension — mirrors `brink_driver::source_tree::is_native`'s
 * `Path::extension()` check (a leading-dot dotfile like `.brink` has no
 * extension in that scheme, so `dot` must not be the first character). Picks
 * which wrap syntax `compileFragment` appends the synthetic symbol in:
 * native `fn`/`flow` blocks for a `.brink` entry, ink `=== ===` knots
 * otherwise. */
export function isNativeEntry(entry: string): boolean {
  const base = entry.slice(entry.lastIndexOf("/") + 1);
  const dot = base.lastIndexOf(".");
  return dot > 0 && base.slice(dot + 1) === "brink";
}

/** The expression-wrap synthetic source for a Tier-1 fragment — evaluates
 * `fragmentSource` as a value via a synthetic zero-arg function named
 * `symbolName`. Native (`fn NAME() { return (EXPR); }`) or ink
 * (`=== function NAME() === \n ~ return (EXPR)`) syntax, matching `entry`'s
 * dialect (`native` from {@link isNativeEntry}). */
export function expressionWrapSource(
  symbolName: string,
  fragmentSource: string,
  native: boolean,
): string {
  return native
    ? `fn ${symbolName}() {\n  return (${fragmentSource});\n}\n`
    : `=== function ${symbolName}() ===\n~ return (${fragmentSource})\n`;
}

/** The content-wrap synthetic source for a Tier-1 fragment — runs
 * `fragmentSource` as story content under a synthetic knot/flow named
 * `symbolName`. Native (`flow NAME() { CONTENT }`) or ink (`=== NAME ===
 * \n CONTENT`) syntax, matching `entry`'s dialect (`native` from
 * {@link isNativeEntry}). */
export function contentWrapSource(
  symbolName: string,
  fragmentSource: string,
  native: boolean,
): string {
  return native
    ? `flow ${symbolName}() {\n${fragmentSource}\n}\n`
    : `=== ${symbolName} ===\n${fragmentSource}\n`;
}

// ── Tier-1 fragment identity + cache (F5.1) ─────────────────────────

/** A Tier-1 fragment-compile cache entry — either a successfully classified
 * + compiled synthetic symbol, or the diagnostics from failing to compile as
 * both an expression and content. */
export type FragmentCompileEntry =
  | { ok: true; kind: "expression" | "content"; symbolName: string; storyBytes: Uint8Array }
  | { ok: false; diagnostics: string[] };

/** Cap on a `StoryRunnerHandle`'s fragment cache (FIFO eviction past this) —
 * a long-lived runner fed many distinct one-off watches must not grow the
 * cache without bound. */
export const FRAGMENT_CACHE_LIMIT = 200;

/** Deterministic FNV-1a 32-bit hash of `source`, as 8 lowercase hex digits —
 * used to name a Tier-1 fragment's synthetic symbol (`__eval_<hash>`).
 * Content-addressed and stable across calls/sessions; not cryptographic, and
 * doesn't need to be — collisions only risk two distinct fragments briefly
 * sharing a synthetic name within one compile, which resolves to whichever
 * wrap actually compiles (an extremely unlikely, self-correcting case, not a
 * correctness hazard). */
export function fragmentContentHash(source: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < source.length; i += 1) {
    hash ^= source.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

/** Insert `entry` under `key`, first evicting the oldest entry if `cache` is
 * already at `limit` (a `Map` iterates in insertion order, so its first key
 * is the oldest — FIFO). Returns `entry` for call-site chaining. Guards
 * against unbounded growth across a long session of one-off watches. */
export function cacheFragmentInto(
  cache: Map<string, FragmentCompileEntry>,
  key: string,
  entry: FragmentCompileEntry,
  limit: number = FRAGMENT_CACHE_LIMIT,
): FragmentCompileEntry {
  if (!cache.has(key) && cache.size >= limit) {
    const oldest = cache.keys().next().value;
    if (oldest !== undefined) {
      cache.delete(oldest);
    }
  }
  cache.set(key, entry);
  return entry;
}
