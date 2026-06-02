import { EditorView, keymap } from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { basicSetup } from 'codemirror';
import { defaultKeymap } from '@codemirror/commands';
import { setDiagnostics } from '@codemirror/lint';
import { buildDecorations } from './highlight.js';

// Drives a stateful `EditorSession` (the wasm IDE API): every doc change syncs
// the session's source once, then highlight and compile read its cached analysis.
export function createEditor(container, { initialDoc, session, onCompiled, tokenTypeNames }) {
  const typeNames = tokenTypeNames();
  let compileTimeout = null;

  // Sync the session to `source` and return its semantic tokens.
  function semanticTokens(source) {
    session.update_source(source);
    return JSON.parse(session.semantic_tokens());
  }

  // Semantic highlight plugin
  const highlightPlugin = EditorView.decorations.compute(['doc'], (state) => {
    return buildDecorations(state.doc.toString(), state.doc, typeNames, semanticTokens);
  });

  // Theme
  const theme = EditorView.theme({
    '&': { height: '100%' },
    '.cm-scroller': { overflow: 'auto' },
  });

  function doCompile(view) {
    const source = view.state.doc.toString();
    session.update_source(source);
    const result = JSON.parse(session.compile_project('main.ink'));

    // Map diagnostics
    const diags = [];
    if (result.warnings) {
      for (const w of result.warnings) {
        const from = Math.min(w.start, source.length);
        const to = Math.min(w.end, source.length);
        diags.push({
          from,
          to: Math.max(to, from),
          severity: w.severity === 'Error' ? 'error' : 'warning',
          message: w.message,
        });
      }
    }
    if (result.error) {
      diags.push({
        from: 0,
        to: 0,
        severity: 'error',
        message: result.error,
      });
    }

    view.dispatch(setDiagnostics(view.state, diags));

    onCompiled(result);
  }

  // Update listener for auto-compile
  const updateListener = EditorView.updateListener.of((update) => {
    if (!update.docChanged) return;

    clearTimeout(compileTimeout);
    compileTimeout = setTimeout(() => doCompile(update.view), 500);
  });

  const state = EditorState.create({
    doc: initialDoc,
    extensions: [
      basicSetup,
      keymap.of(defaultKeymap),
      theme,
      highlightPlugin,
      updateListener,
    ],
  });

  const view = new EditorView({ state, parent: container });

  // Initial compile
  setTimeout(() => doCompile(view), 100);

  return {
    triggerCompile() {
      clearTimeout(compileTimeout);
      doCompile(view);
    },
  };
}
