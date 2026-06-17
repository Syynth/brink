import { type Extension } from "@codemirror/state";
import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import type { CompletionItem } from "@brink/wasm-types";

// Keys are the wasm `symbol_kind_str` values (snake_case) — NOT the Rust
// `SymbolKind` variant names. A mismatch here silently falls back to `"text"`,
// which both mis-icons every completion and (because nothing is then typed
// `"function"`/`"method"`) disables auto-open-on-completion (#229).
const KIND_MAP: Record<string, string> = {
  knot: "function",
  stitch: "method",
  variable: "variable",
  constant: "constant",
  list: "enum",
  list_item: "enumMember",
  external: "function",
  label: "property",
  param: "variable",
  temp: "variable",
  // Host value-picker items (#174): a labelled value for an argument slot.
  value: "enum",
};

/** Map a wasm completion `kind` to a CodeMirror completion `type` (its icon).
 *  Knots/Externals are `"function"` and stitches `"method"` — the types
 *  auto-open-on-completion keys off (#229). Unknown kinds fall back to `"text"`. */
export function completionType(kind: string): string {
  return KIND_MAP[kind] ?? "text";
}

/**
 * Map a wasm completion to a CodeMirror option. Host value-list items
 * (#174, kind `"value"`) show a human label but insert a different literal
 * (e.g. an item id), so make them matchable by the label, the inserted value,
 * AND the detail (#211): the `label` CM filters on is the combined terms, while
 * `displayLabel` keeps the row showing just the name. Typing the name, the id,
 * or the detail ("Switch #5") all narrow to the right value. Plain completions
 * match and display their name as before.
 */
export function toCompletionOption(item: CompletionItem): Completion {
  const apply = item.insert ?? undefined;
  const detail = item.detail ?? undefined;
  const type = completionType(item.kind);
  if (item.kind === "value") {
    const terms = [item.name, item.insert, item.detail].filter(Boolean).join(" ");
    return { label: terms, displayLabel: item.name, type, detail, apply };
  }
  return { label: item.name, type, detail, apply };
}

export interface CompletionsOptions {
  getCompletions: (source: string, offset: number) => CompletionItem[];
}

export function completionsExtension(options: CompletionsOptions): Extension {
  return autocompletion({
    override: [
      (ctx: CompletionContext): CompletionResult | null => {
        const word = ctx.matchBefore(/[\w.]+/);
        if (!word && !ctx.explicit) return null;

        const from = word ? word.from : ctx.pos;
        const source = ctx.state.doc.toString();

        let items: CompletionItem[];
        try {
          items = options.getCompletions(source, ctx.pos);
        } catch {
          return null;
        }

        if (items.length === 0) return null;

        return {
          from,
          options: items.map(toCompletionOption),
        };
      },
    ],
  });
}
