import type { EditorState } from "@codemirror/state";

/**
 * One full-document string per `EditorState`, memoized (#3064 phase A1).
 *
 * `state.doc.toString()` is an O(doc) rope flatten. Before this cache the
 * per-keystroke transaction materialized the document up to four times —
 * the element-type push, the folding and semantic-token facet calls
 * (whose host callbacks don't even read the argument, but whose PUBLIC
 * facet signatures keep it for external embedders), and the slot
 * mirror-edit listener. `EditorState` values are immutable, so a
 * `WeakMap` keyed on the state is exact: same state, same string, and
 * abandoned states drop their entry with the state itself.
 */
const cache = new WeakMap<EditorState, string>();

export function docString(state: EditorState): string {
  let s = cache.get(state);
  if (s === undefined) {
    s = state.doc.toString();
    cache.set(state, s);
  }
  return s;
}
