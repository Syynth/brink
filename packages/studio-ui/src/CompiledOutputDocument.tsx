/**
 * CompiledOutputDocument — the "compiled-output" document type (issue #91,
 * spec §4 "Compiled Output", §7.8).
 *
 * A read-only editor document over the current compile's `.inkt` dump
 * (`programInkt` in the session slice — captured when a successfully
 * compiled program loads). Compile-bound, not session-bound: it survives
 * `story.stop` and live-updates on each successful compile. Singleton — one
 * DocumentRef, opened via the `program.openCompiledOutput` command, so a
 * reopen focuses the existing tab (groups-store reveal policy).
 *
 * Unlike "ink-file" this type does NOT ride the wasm document-handle
 * machinery (DocumentSessions); it renders a plain string in its own CM6
 * view. Read-only is enforced three ways: `EditorView.editable.of(false)`
 * (inert DOM), `EditorState.readOnly.of(true)` (commands/search-replace
 * no-op), and a transaction filter that drops any doc change not carrying
 * the component's own content-replace annotation.
 */

import { useEffect, useRef } from "react";
import { Annotation, EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { foldGutter, foldKeymap } from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import type {
  CommandRegistry,
  DocumentRef,
  DocumentViewProps,
  EditorGroupsStore,
} from "@brink/studio-shell";
import { useStudioStore } from "./StoreContext.js";
import { inktFolding, inktHighlighting, inktLanguage } from "./inkt-mode.js";

export const COMPILED_OUTPUT_TYPE_ID = "compiled-output";
export const COMPILED_OUTPUT_DOC_ID = "program";
export const OPEN_COMPILED_OUTPUT_COMMAND_ID = "program.openCompiledOutput";

/** The singleton DocumentRef — one stable identity, one tab. */
export function compiledOutputRef(): DocumentRef {
  return {
    typeId: COMPILED_OUTPUT_TYPE_ID,
    docId: COMPILED_OUTPUT_DOC_ID,
    title: "Compiled Output",
  };
}

/**
 * Register `program.openCompiledOutput` (palette: "Program: Open Compiled
 * Output", no default keybinding). Opens pinned into the focused group; the
 * groups store's reveal policy focuses an existing tab wherever it lives.
 */
export function registerCompiledOutputCommand(
  commands: CommandRegistry,
  editorGroups: EditorGroupsStore,
): () => void {
  return commands.register({
    id: OPEN_COMPILED_OUTPUT_COMMAND_ID,
    title: "Program: Open Compiled Output",
    run: () =>
      editorGroups.getState().openDocument(compiledOutputRef(), { pinned: true }),
  });
}

/** Marks the component's own content-replace transactions (and nothing else). */
const setCompiledOutput = Annotation.define<boolean>();

/** Dispatch a full content replace through the read-only filter. */
export function replaceCompiledOutput(view: EditorView, text: string): void {
  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: text },
    annotations: setCompiledOutput.of(true),
  });
}

const compiledOutputTheme = EditorView.theme(
  {
    "&": {
      height: "100%",
      backgroundColor: "var(--brink-bg, #1e1e2e)",
      color: "var(--brink-fg, #cdd6f4)",
    },
    ".cm-scroller": {
      overflow: "auto",
      fontFamily: '"JetBrains Mono", "Fira Code", "Cascadia Code", monospace',
      fontSize: "12px",
      lineHeight: "1.6",
    },
    ".cm-gutters": {
      backgroundColor: "var(--brink-bg, #1e1e2e)",
      borderRight: "1px solid var(--brink-border, #45475a)",
      color: "var(--brink-fg-dim, #6c7086)",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
      backgroundColor: "rgba(137, 180, 250, 0.3) !important",
    },
  },
  { dark: true },
);

/** The full extension set for a Compiled Output view (exported for tests). */
export function compiledOutputExtensions(): Extension[] {
  return [
    lineNumbers(),
    foldGutter(),
    inktLanguage,
    inktHighlighting,
    inktFolding,
    highlightSelectionMatches(),
    // Mod-F search panel + fold keys. No editing keymaps — there is nothing
    // to edit (search's replace commands are disabled by readOnly anyway).
    keymap.of([...searchKeymap, ...foldKeymap]),
    EditorState.readOnly.of(true),
    EditorView.editable.of(false),
    // Non-editable content is not focusable by default; the search keymap
    // (and keyboard selection) needs focus inside the view.
    EditorView.contentAttributes.of({ tabindex: "0" }),
    // Belt-and-braces read-only: drop ANY doc change that is not our own
    // content replace — typing, paste, drop, programmatic dispatch.
    EditorState.transactionFilter.of((tr) =>
      tr.docChanged && tr.annotation(setCompiledOutput) !== true ? [] : tr,
    ),
    compiledOutputTheme,
  ];
}

function CompiledOutputEditor({ text }: { text: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const view = new EditorView({
      state: EditorState.create({ extensions: compiledOutputExtensions() }),
      parent: container,
    });
    viewRef.current = view;
    return () => {
      viewRef.current = null;
      view.destroy();
    };
  }, []);

  // Populate on mount and live-update on each successful compile, keeping
  // the scroll position across replaces (cheap best effort — a full replace
  // would otherwise reset it).
  useEffect(() => {
    const view = viewRef.current;
    if (!view || view.state.doc.toString() === text) return;
    const scrollTop = view.scrollDOM.scrollTop;
    replaceCompiledOutput(view, text);
    view.scrollDOM.scrollTop = scrollTop;
  }, [text]);

  return (
    <div
      ref={containerRef}
      className="brink-compiled-output"
      style={{ height: "100%", width: "100%" }}
    />
  );
}

export function CompiledOutputDocument(_props: DocumentViewProps) {
  const inkt = useStudioStore((s) => s.programInkt);

  if (inkt === null) {
    // Compile-bound placeholder: no successful compile has loaded yet.
    return (
      <div className="compiled-output-empty">
        <div className="state-view-empty">
          <p className="state-view-empty-title">No compiled program yet</p>
          <p className="state-view-empty-hint">
            The .inkt dump of the current compile appears here after the
            first successful compile.
          </p>
        </div>
      </div>
    );
  }

  return <CompiledOutputEditor text={inkt} />;
}
