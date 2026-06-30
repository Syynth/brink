import { keymap } from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import { search, highlightSelectionMatches, searchKeymap } from "@codemirror/search";

/// Options for the opt-in Find extension.
export interface FindPanelOptions {
  /// Anchor the search panel to the top of the editor (default `true`).
  /// Set `false` to dock it at the bottom (CodeMirror's stock placement).
  top?: boolean;
}

/// A reusable, opt-in Find extension bundling `@codemirror/search`'s panel,
/// selection-match highlighting, and the standard search keymap.
///
/// This is a pure factory: it does not enable itself anywhere. Hosts opt in by
/// adding the returned `Extension` to their editor configuration.
export function findPanel(options?: FindPanelOptions): Extension {
  return [
    search({ top: options?.top ?? true }),
    highlightSelectionMatches(),
    keymap.of(searchKeymap),
  ];
}
