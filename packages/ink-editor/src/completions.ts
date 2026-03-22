import { type Extension } from "@codemirror/state";
import { autocompletion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import type { CompletionItem } from "@brink/wasm-types";

const KIND_MAP: Record<string, string> = {
  Knot: "function",
  Stitch: "method",
  Variable: "variable",
  Constant: "constant",
  List: "enum",
  ListItem: "enumMember",
  External: "function",
  Label: "property",
  Param: "variable",
  Temp: "variable",
};

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
            type: KIND_MAP[item.kind] ?? "text",
            detail: item.detail ?? undefined,
          })),
        };
      },
    ],
  });
}
