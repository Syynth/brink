import { type Extension } from "@codemirror/state";
import { autocompletion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
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
          options: items.map((item) => ({
            label: item.name,
            type: completionType(item.kind),
            detail: item.detail ?? undefined,
            // Host value picker (#174): display `item.name` (the label), insert
            // `item.insert` (the literal). Omitted ⇒ CodeMirror inserts the label.
            apply: item.insert ?? undefined,
          })),
        };
      },
    ],
  });
}
