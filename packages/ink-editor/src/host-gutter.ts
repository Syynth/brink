/**
 * Host gutter-marker contribution API (#343).
 *
 * Hosts embedding `@brink-lang/editor` (without the studio app) can contribute
 * their own gutter markers — breakpoints, per-line annotations, tool-specific
 * run/flag icons — into a gutter the editor coordinates with its built-in
 * play/fold/diagnostic affordances, instead of bolting on a raw CM6 `gutter()`
 * that doesn't share ordering or click precedence.
 *
 * Purely additive: enabled only when `BrinkStudioOptions.getGutterMarkers` is
 * provided (the same pattern as `getCodeActions`, `getFoldingRanges`, …). The
 * host gutter occupies a defined slot: it renders **after** (to the right of)
 * the built-in play-from-here gutter, immediately beside the text.
 *
 * Markers are keyed by 1-based line. Ordering is deterministic: markers sort
 * by line, and multiple markers on the same line keep the host's array order.
 * Marker sets are recomputed on document changes; when the host's marker set
 * changes for *external* reasons (a breakpoint toggled in another panel, a
 * session ending), the host dispatches `refreshGutterMarkersEffect` — the
 * `refreshGutterMarkers(view)` helper does exactly that.
 */

import {
  RangeSet,
  RangeSetBuilder,
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
} from "@codemirror/state";
import { EditorView, GutterMarker, gutter } from "@codemirror/view";

/** One host-contributed gutter marker, keyed by 1-based `line`. */
export interface HostGutterMarker {
  /** 1-based document line the marker sits on. Out-of-range lines are dropped. */
  line: number;
  /** Extra class on the marker element (base: `brink-host-gutter-marker`). */
  className?: string;
  /** Short text/icon content (e.g. `"●"`, `"⚑"`). */
  text?: string;
  /** Tooltip (`title` attribute) + accessible label. */
  title?: string;
  /** Per-marker click handler. Runs before the shared `onGutterMarkerClick`. */
  onClick?: (line: number) => void;
}

export interface HostGutterOptions {
  /**
   * The host's markers for `source` over the inclusive 1-based line range
   * `[fromLine, toLine]`. Currently queried for the whole document; the range
   * parameters exist so hosts can pre-filter and the contract can later narrow
   * to the viewport without an API change.
   */
  getGutterMarkers: (source: string, fromLine: number, toLine: number) => HostGutterMarker[];
  /** Shared click handler — fires for every host marker click, after the
   *  marker's own `onClick`. */
  onGutterMarkerClick?: (marker: HostGutterMarker, line: number) => void;
}

/**
 * Host-dispatched refresh: recompute the marker set without a document change.
 * Dispatch it (or call `refreshGutterMarkers(view)`) when the data behind
 * `getGutterMarkers` changes — e.g. a breakpoint toggled from another panel.
 */
export const refreshGutterMarkersEffect = StateEffect.define<void>();

/** Convenience wrapper: dispatch `refreshGutterMarkersEffect` on `view`. */
export function refreshGutterMarkers(view: EditorView): void {
  view.dispatch({ effects: refreshGutterMarkersEffect.of(undefined) });
}

/** The rendered CM6 marker wrapping one `HostGutterMarker`. */
class HostMarker extends GutterMarker {
  constructor(
    readonly marker: HostGutterMarker,
    private readonly onGutterMarkerClick?: (marker: HostGutterMarker, line: number) => void,
  ) {
    super();
  }

  override eq(other: HostMarker): boolean {
    return (
      this.marker.line === other.marker.line &&
      this.marker.className === other.marker.className &&
      this.marker.text === other.marker.text &&
      this.marker.title === other.marker.title &&
      this.marker.onClick === other.marker.onClick &&
      this.onGutterMarkerClick === other.onGutterMarkerClick
    );
  }

  override toDOM(): HTMLElement {
    const { marker } = this;
    const clickable = marker.onClick !== undefined || this.onGutterMarkerClick !== undefined;
    const el = document.createElement(clickable ? "button" : "span");
    el.className = marker.className
      ? `brink-host-gutter-marker ${marker.className}`
      : "brink-host-gutter-marker";
    if (marker.text !== undefined) el.textContent = marker.text;
    if (marker.title !== undefined) {
      el.title = marker.title;
      el.setAttribute("aria-label", marker.title);
    }
    if (clickable) {
      el.addEventListener("mousedown", (event) => {
        // Keep the editor from grabbing the event (selection/focus shift).
        event.preventDefault();
        event.stopPropagation();
        marker.onClick?.(marker.line);
        this.onGutterMarkerClick?.(marker, marker.line);
      });
    }
    return el;
  }
}

/** Build the marker `RangeSet` for the current document. Deterministic: sorted
 *  by line, stable (host array order) within a line; invalid lines dropped. */
function buildMarkers(state: EditorState, options: HostGutterOptions): RangeSet<GutterMarker> {
  let markers: HostGutterMarker[];
  try {
    markers = options.getGutterMarkers(state.doc.toString(), 1, state.doc.lines);
  } catch {
    return RangeSet.empty;
  }
  const valid = markers
    .filter((m) => Number.isInteger(m.line) && m.line >= 1 && m.line <= state.doc.lines)
    .map((marker, index) => ({ marker, index }))
    .sort((a, b) => a.marker.line - b.marker.line || a.index - b.index);
  const builder = new RangeSetBuilder<GutterMarker>();
  for (const { marker } of valid) {
    const from = state.doc.line(marker.line).from;
    builder.add(from, from, new HostMarker(marker, options.onGutterMarkerClick));
  }
  return builder.finish();
}

export function hostGutterExtension(options: HostGutterOptions): Extension {
  const markersField = StateField.define<RangeSet<GutterMarker>>({
    create: (state) => buildMarkers(state, options),
    update(value, tr) {
      if (tr.docChanged || tr.effects.some((e) => e.is(refreshGutterMarkersEffect))) {
        return buildMarkers(tr.state, options);
      }
      return value;
    },
  });

  const hostGutter = gutter({
    class: "brink-host-gutter",
    markers: (view) => view.state.field(markersField),
  });

  return [markersField, hostGutter, hostGutterTheme];
}

const hostGutterTheme = EditorView.baseTheme({
  ".brink-host-gutter-marker": {
    all: "unset",
    boxSizing: "border-box",
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    height: "100%",
    padding: "0 2px",
    fontSize: "0.75em",
    lineHeight: "1",
  },
  "button.brink-host-gutter-marker": {
    cursor: "pointer",
  },
});
