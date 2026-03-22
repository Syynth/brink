import { useRef, useEffect, useImperativeHandle, forwardRef, memo } from "react";
import { EditorView, keymap } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { basicSetup } from "codemirror";
import { defaultKeymap } from "@codemirror/commands";
import { forEachDiagnostic } from "@codemirror/lint";
import { brinkStudio, type BrinkStudioOptions } from "./extensions.js";
import { elementTypeField, type LineInfo } from "./element-type.js";
import { getHintsForElement, lineHasContent, buildContext } from "./transitions.js";
import { convertLineToType as cmConvertLineToType } from "./convert.js";
import type { CompileResult } from "@brink/wasm-types";

// ── Public types ───────────────────────────────────────────────────

export interface KeyHint {
  key: string;
  hint: string;
}

export interface InkEditorProps {
  studioOptions: BrinkStudioOptions;
  initialState?: EditorState;
  onContentChange?: (content: string) => void;
  onCursorChange?: (line: number, col: number) => void;
  onLineInfoChange?: (info: LineInfo | null, hints: KeyHint[]) => void;
  onCompileResult?: (result: CompileResult) => void;
  onDocEdited?: () => void;
}

export interface InkEditorHandle {
  setState(state: EditorState): void;
  focus(): void;
  getContent(): string;
  getView(): EditorView;
  triggerCompile(): void;
  setContent(content: string): void;
  scrollTo(pos: number): void;
  convertLineToType(sigil: string): void;
}

// ── Component ──────────────────────────────────────────────────────

export const InkEditor = memo(forwardRef<InkEditorHandle, InkEditorProps>(
  function InkEditor(props, ref) {
    const containerRef = useRef<HTMLDivElement>(null);
    const viewRef = useRef<EditorView | null>(null);

    // Keep latest callbacks in refs so the CM6 listener always sees them
    // without requiring editor remounts.
    const callbacksRef = useRef(props);
    callbacksRef.current = props;

    useImperativeHandle(ref, () => ({
      setState(state: EditorState) {
        viewRef.current?.setState(state);
      },
      focus() {
        viewRef.current?.focus();
      },
      getContent(): string {
        return viewRef.current?.state.doc.toString() ?? "";
      },
      getView(): EditorView {
        if (!viewRef.current) throw new Error("InkEditor: view not mounted");
        return viewRef.current;
      },
      triggerCompile() {
        const view = viewRef.current;
        if (!view) return;
        const source = view.state.doc.toString();
        const result = callbacksRef.current.studioOptions.compile(source);
        callbacksRef.current.onCompileResult?.(result);
      },
      setContent(content: string) {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch({
          changes: { from: 0, to: view.state.doc.length, insert: content },
        });
      },
      scrollTo(pos: number) {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch({
          effects: EditorView.scrollIntoView(pos, { y: "center" }),
        });
      },
      convertLineToType(sigil: string) {
        const view = viewRef.current;
        if (!view) return;
        cmConvertLineToType(view, sigil);
      },
    }), []);

    useEffect(() => {
      const container = containerRef.current;
      if (!container) return;

      // Build the update listener that pushes status data through callbacks
      const updateListener = EditorView.updateListener.of((update) => {
        const cbs = callbacksRef.current;

        if (update.docChanged) {
          const content = update.state.doc.toString();
          cbs.onContentChange?.(content);
          cbs.onDocEdited?.();
        }

        if (update.docChanged || update.selectionSet) {
          const { state } = update.view;
          const pos = state.selection.main.head;
          const line = state.doc.lineAt(pos);
          const col = pos - line.from;

          cbs.onCursorChange?.(line.number, col + 1);

          // Extract line info and key hints
          const infos = state.field(elementTypeField);
          const info = infos[line.number - 1] ?? null;

          let hints: KeyHint[] = [];
          if (info) {
            const hasContent = lineHasContent(line.text, info);
            const lineCtx = buildContext(infos, line.number - 1);
            hints = getHintsForElement(info, hasContent, lineCtx);
          }

          cbs.onLineInfoChange?.(info, hints);
        }
      });

      // Create the initial state
      const state = props.initialState ?? EditorState.create({
        doc: "",
        extensions: [
          brinkStudio(props.studioOptions),
          basicSetup,
          keymap.of(defaultKeymap),
          EditorView.lineWrapping,
          updateListener,
        ],
      });

      const view = new EditorView({ state, parent: container });
      viewRef.current = view;

      // Expose for e2e tests
      (window as any).__brinkView = view;

      return () => {
        view.destroy();
        viewRef.current = null;
        delete (window as any).__brinkView;
      };
      // Mount once — never remount. Props changes are handled via refs.
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    return <div ref={containerRef} style={{ height: "100%", width: "100%" }} />;
  },
));
