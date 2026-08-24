/**
 * CM6 viewport instrumentation (measure-first ruling, 2026-08-24), aimed at
 * the reported blank-viewport-while-scrolling symptom: CodeMirror renders
 * only the viewport, so scrolling ahead of rendered lines means the
 * per-visible-line render path could not fill the incoming viewport within a
 * frame. Two signals:
 *
 * - `cm.viewportLag` — time from a scroll event to the next update in which
 *   CM actually moved the viewport. When line rendering keeps up this is
 *   sub-frame; when the user outruns rendering it grows to the blank gap the
 *   user sees.
 * - `cm.viewportUpdate` — a zero-cost counter span marking each
 *   viewport-changing update (meta: new viewport length in lines), so runs
 *   can normalize per-update costs recorded by individual extensions.
 */

import type { Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { isPerfEnabled, perfRecord } from "./probe.js";

/** A scroll older than this cannot plausibly own the next viewport move. */
const SCROLL_ATTRIBUTION_WINDOW_MS = 500;

export function perfViewportProbe(): Extension {
  let lastScrollAt = -1;

  const plugin = ViewPlugin.fromClass(
    class {
      update(update: ViewUpdate): void {
        if (!update.viewportChanged || !isPerfEnabled()) return;
        const now = performance.now();
        const viewport = update.view.viewport;
        const doc = update.state.doc;
        const lines =
          doc.lineAt(Math.min(viewport.to, doc.length)).number -
          doc.lineAt(Math.min(viewport.from, doc.length)).number +
          1;
        perfRecord("cm.viewportUpdate", now, 0, lines);
        if (lastScrollAt >= 0 && now - lastScrollAt < SCROLL_ATTRIBUTION_WINDOW_MS) {
          perfRecord("cm.viewportLag", lastScrollAt, now - lastScrollAt);
          lastScrollAt = -1;
        }
      }
    },
  );

  return [
    plugin,
    EditorView.domEventHandlers({
      scroll: () => {
        if (isPerfEnabled()) lastScrollAt = performance.now();
        return false;
      },
    }),
  ];
}
