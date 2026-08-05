import { type Extension } from "@codemirror/state";
import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import type { AutoImportResult, CompletionItem } from "@brink/wasm-types";

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
  // Cue-name completions (#2134): matches the LSP side's
  // `CompletionItemKind::CONSTANT`.
  cue: "constant",
};

/** Map a wasm completion `kind` to a CodeMirror completion `type` (its icon).
 *  Knots/Externals are `"function"` and stitches `"method"` — the types
 *  auto-open-on-completion keys off (#229). Unknown kinds fall back to `"text"`. */
export function completionType(kind: string): string {
  return KIND_MAP[kind] ?? "text";
}

/** Callback that ensures the current file `INCLUDE`s `target`, returning the
 *  whole-file UTF-16 `INCLUDE`-insertion edit (or a no-op when already in
 *  scope). Wired by the host so the completion-accept path can auto-import. */
export type AutoImportFn = (target: string) => AutoImportResult;

/**
 * The bare filename of a project-relative source path, for the "from <file>"
 * affordance — `scenes/economy.ink` shows as `from economy.ink`.
 */
function baseName(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash === -1 ? path : path.slice(slash + 1);
}

/**
 * A completion-accept handler for an out-of-scope symbol (#312 F): insert the
 * symbol text at the cursor as usual AND, when the symbol's file is not yet
 * reachable, insert one `INCLUDE` line into the current file's INCLUDE block.
 * Idempotent — a symbol already in scope adds no INCLUDE (the wasm op reports
 * `already_reachable` and returns no edit).
 */
function outOfScopeApply(
  insertText: string,
  sourceFile: string,
  autoImport: AutoImportFn,
): (view: EditorView, completion: Completion, from: number, to: number) => void {
  return (view, _completion, from, to) => {
    const result = autoImport(sourceFile);
    // The symbol insertion is always applied. When the INCLUDE edit is present
    // (target not yet reachable), fold it into the SAME transaction so both
    // land atomically; its whole-file offsets are stable relative to the
    // symbol insertion (the INCLUDE goes at the top, the symbol near the
    // cursor — non-overlapping in original coordinates).
    const changes: { from: number; to: number; insert: string }[] = [
      { from, to, insert: insertText },
    ];
    if (result.ok && !result.already_reachable && result.edit) {
      changes.push({
        from: result.edit.from,
        to: result.edit.to,
        insert: result.edit.insert,
      });
    }
    view.dispatch({
      changes,
      // Keep the caret after the inserted symbol. The INCLUDE (if any) is
      // inserted before `from`, so account for its length shift.
      selection: {
        anchor:
          from +
          insertText.length +
          (changes.length > 1 && changes[1].from <= from
            ? changes[1].insert.length
            : 0),
      },
    });
  };
}

/**
 * Map a wasm completion to a CodeMirror option. Host value-list items
 * (#174, kind `"value"`) show a human label but insert a different literal
 * (e.g. an item id), so make them matchable by the label, the inserted value,
 * AND the detail (#211): the `label` CM filters on is the combined terms, while
 * `displayLabel` keeps the row showing just the name. Typing the name, the id,
 * or the detail ("Switch #5") all narrow to the right value. Plain completions
 * match and display their name as before.
 *
 * Out-of-scope symbols (#312 F) — defined in a file NOT reachable from the
 * current file's INCLUDE graph — get a "from <file>" detail suffix and, when an
 * `autoImport` callback is provided, a custom apply that inserts the symbol AND
 * the missing `INCLUDE`.
 */
export function toCompletionOption(
  item: CompletionItem,
  autoImport?: AutoImportFn,
): Completion {
  const apply = item.insert ?? undefined;
  const type = completionType(item.kind);
  if (item.kind === "value") {
    const detail = item.detail ?? undefined;
    const terms = [item.name, item.insert, item.detail].filter(Boolean).join(" ");
    return { label: terms, displayLabel: item.name, type, detail, apply };
  }

  if (item.out_of_scope && item.source_file) {
    const from = `from ${baseName(item.source_file)}`;
    // Combine any existing detail (e.g. a typed signature) with the source-file
    // affordance so the row reads "(x: int) · from economy.ink".
    const detail = item.detail ? `${item.detail} · ${from}` : from;
    return {
      label: item.name,
      type,
      detail,
      // Only offer the auto-inserting apply when a callback is wired; without
      // it, the row still inserts the symbol name by default (no INCLUDE).
      apply: autoImport
        ? outOfScopeApply(item.insert ?? item.name, item.source_file, autoImport)
        : (item.insert ?? undefined),
    };
  }

  const detail = item.detail ?? undefined;
  return { label: item.name, type, detail, apply };
}

export interface CompletionsOptions {
  getCompletions: (source: string, offset: number) => CompletionItem[];
  /** Auto-import (#312 F): ensure the current file `INCLUDE`s the symbol's
   *  source file on accepting an out-of-scope completion. Optional — without
   *  it, out-of-scope rows still insert the symbol but add no INCLUDE. */
  autoImport?: AutoImportFn;
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
          options: items.map((item) => toCompletionOption(item, options.autoImport)),
        };
      },
    ],
  });
}
