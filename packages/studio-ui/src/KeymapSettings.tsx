/**
 * Settings ▸ Keymap — rebind commands by pressing the keys.
 *
 * This replaces a raw JSON textarea, which asked an author to know both a
 * command id and the `"Mod-Shift-P"` spelling, and gave no way to discover
 * either. The JSON is kept as an escape hatch below the table, the same
 * form-plus-text split `brink.toml`'s own settings use.
 *
 * Capture records through `chordFromEvent` — the SAME function the global
 * key handler dispatches through — so what an author presses is exactly
 * what will fire. A typed binding can be spelled correctly and still not
 * be the chord the keyboard produces.
 *
 * Rebinding DISPLACES (ruled 2026-08-30). `Keymap.byChord` is a
 * `Map<chordId, commandId>`, so two commands holding one chord means the
 * last registered silently wins and the other is dead. Rather than let an
 * author build that state, taking a chord takes it off its previous owner
 * and says so. See `keymap-model.ts` for the rule; this file is its view.
 */

import { useMemo, useRef, useState } from "react";
import {
  type Chord,
  type KeymapRow,
  bindChord,
  chordFromEvent,
  chordId,
  formatChord,
  keymapRows,
  resetCommand,
  unbindChord,
  useShell,
} from "@brink/studio-shell";

/** Which row is recording, if any. */
interface Capture {
  commandId: string;
  chord: Chord | null;
  /** The command this chord would take it from — shown before saving. */
  conflict: { title: string; nowUnbound: boolean } | null;
}

export function KeymapSettings() {
  const { commands, keymapOverrides, isMac } = useShell();
  const [, bump] = useState(0);
  const [query, setQuery] = useState("");
  const [byKey, setByKey] = useState(false);
  const [capture, setCapture] = useState<Capture | null>(null);

  const overrides = keymapOverrides.current;
  const all = useMemo(
    () => commands.list().map((c) => ({ id: c.id, title: c.title, keybinding: c.keybinding })),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- registry is stable; rows re-derive below
    [commands],
  );
  const rows = keymapRows(all, overrides);

  const apply = (next: typeof overrides): void => {
    keymapOverrides.set(next);
    bump((n) => n + 1);
  };

  const visible = rows.filter((row) => matches(row, query, byKey, isMac));
  const customised = rows.filter((row) => row.overridden).length;

  return (
    <div className="keymap">
      <div className="keymap-toolbar">
        <input
          className="keymap-search"
          type="text"
          value={query}
          placeholder={byKey ? "Press or type a key…" : "Search commands"}
          aria-label="Search commands"
          onChange={(e) => setQuery(e.target.value)}
        />
        <button
          type="button"
          className={`keymap-bykey${byKey ? " active" : ""}`}
          aria-pressed={byKey}
          onClick={() => setByKey((v) => !v)}
          title="Match the search against keybindings instead of names"
        >
          Search by key
        </button>
      </div>

      {visible.length === 0 ? (
        <p className="keymap-empty">No command matches “{query}”.</p>
      ) : (
        <ul className="keymap-rows">
          {withHeadings(visible).map((entry) =>
            entry.kind === "heading" ? (
              <li key={`h:${entry.category}`} className="keymap-heading">
                {entry.category}
              </li>
            ) : (
              <KeymapRowView
                key={entry.row.id}
                row={entry.row}
                isMac={isMac}
                capture={capture?.commandId === entry.row.id ? capture : null}
                onStartCapture={() =>
                  setCapture({ commandId: entry.row.id, chord: null, conflict: null })
                }
                onCancelCapture={() => setCapture(null)}
                onCaptureChord={(chord) => {
                  const probe = bindChord(all, overrides, entry.row.id, chord);
                  setCapture({
                    commandId: entry.row.id,
                    chord,
                    conflict:
                      probe.displaced === null
                        ? null
                        : { title: probe.displaced.title, nowUnbound: probe.displaced.nowUnbound },
                  });
                }}
                onCommit={(chord) => {
                  apply(bindChord(all, overrides, entry.row.id, chord).overrides);
                  setCapture(null);
                }}
                onRemove={(chord) => apply(unbindChord(all, overrides, entry.row.id, chord))}
                onReset={() => apply(resetCommand(overrides, entry.row.id))}
              />
            ),
          )}
        </ul>
      )}

      <div className="keymap-footer">
        <span>
          {customised === 0
            ? "No customised bindings"
            : `${customised} customised binding${customised === 1 ? "" : "s"}`}
        </span>
        <button
          type="button"
          className="keymap-reset-all"
          disabled={customised === 0}
          onClick={() => apply({})}
        >
          Reset all
        </button>
      </div>
    </div>
  );
}

function KeymapRowView({
  row,
  isMac,
  capture,
  onStartCapture,
  onCancelCapture,
  onCaptureChord,
  onCommit,
  onRemove,
  onReset,
}: {
  row: KeymapRow;
  isMac: boolean;
  capture: Capture | null;
  onStartCapture: () => void;
  onCancelCapture: () => void;
  onCaptureChord: (chord: Chord) => void;
  onCommit: (chord: Chord) => void;
  onRemove: (chord: Chord) => void;
  onReset: () => void;
}) {
  const fieldRef = useRef<HTMLDivElement>(null);

  if (capture !== null) {
    return (
      <li className="keymap-row is-capturing">
        <div className="keymap-row-main">
          <span className="keymap-name">{row.name}</span>
          <div
            ref={fieldRef}
            className="keymap-capture"
            tabIndex={0}
            role="textbox"
            aria-label={`Recording a keybinding for ${row.title}`}
            autoFocus
            onKeyDown={(event) => {
              // Every key belongs to the recording, including Tab and the
              // chords the browser would otherwise act on.
              event.preventDefault();
              event.stopPropagation();
              if (event.key === "Escape") {
                onCancelCapture();
                return;
              }
              if (event.key === "Enter" && capture.chord !== null) {
                onCommit(capture.chord);
                return;
              }
              const chord = chordFromEvent(event.nativeEvent, isMac);
              if (chord !== null) onCaptureChord(chord);
            }}
            onBlur={onCancelCapture}
          >
            {capture.chord === null ? (
              <span className="keymap-capture-hint">Press keys…</span>
            ) : (
              <kbd className="keymap-chord is-capturing">{formatChord(capture.chord, isMac)}</kbd>
            )}
          </div>
          <span className="keymap-source is-capturing">Recording</span>
          <span className="keymap-action" />
        </div>
        <p className="keymap-capture-help">
          {capture.chord === null
            ? "Press the keys you want, then Enter to save. Esc to cancel."
            : "Enter to save · Esc to cancel"}
        </p>
        {capture.conflict !== null && (
          <p className="keymap-conflict">
            <span className="keymap-conflict-chord">
              {capture.chord === null ? "" : formatChord(capture.chord, isMac)}
            </span>{" "}
            is bound to <strong>{capture.conflict.title}</strong>.{" "}
            {capture.conflict.nowUnbound
              ? "Saving leaves that command with no keybinding."
              : "Saving takes this key from it; its other keys are kept."}
          </p>
        )}
      </li>
    );
  }

  return (
    <li className="keymap-row">
      <div className="keymap-row-main">
        <span className="keymap-name">{row.name}</span>
        <div className="keymap-chords">
          {row.chords.length === 0 ? (
            <span className="keymap-unbound">—</span>
          ) : (
            row.chords.map((chord) => (
              <span key={chordId(chord)} className="keymap-chip">
                <kbd className={`keymap-chord${row.source === "custom" ? " is-custom" : ""}`}>
                  {formatChord(chord, isMac)}
                </kbd>
                <button
                  type="button"
                  className="keymap-chip-remove"
                  aria-label={`Remove ${formatChord(chord, isMac)} from ${row.title}`}
                  onClick={() => onRemove(chord)}
                >
                  ×
                </button>
              </span>
            ))
          )}
          <button
            type="button"
            className="keymap-add"
            aria-label={`Add a keybinding for ${row.title}`}
            title="Add a keybinding"
            onClick={onStartCapture}
          >
            +
          </button>
        </div>
        <span className={`keymap-source is-${row.source}`}>{SOURCE_LABEL[row.source]}</span>
        <span className="keymap-action">
          {!row.overridden ? null : (
            <button
              type="button"
              className="keymap-reset"
              aria-label={`Reset ${row.title} to its default keybinding`}
              title="Reset to default"
              onClick={onReset}
            >
              ⟲
            </button>
          )}
        </span>
      </div>
    </li>
  );
}

const SOURCE_LABEL = {
  default: "Default",
  custom: "Custom",
  unbound: "Unbound",
} as const;

/** Rows interleaved with their category headings. */
type Entry = { kind: "heading"; category: string } | { kind: "row"; row: KeymapRow };

function withHeadings(rows: KeymapRow[]): Entry[] {
  const out: Entry[] = [];
  let seen: string | null = null;
  for (const row of rows) {
    if (row.category !== seen) {
      out.push({ kind: "heading", category: row.category });
      seen = row.category;
    }
    out.push({ kind: "row", row });
  }
  return out;
}

/**
 * Whether a row survives the filter.
 *
 * "Search by key" matches the RENDERED chord, so typing what is printed on
 * the keys — `⌘K`, or `ctrl+k` — finds it. Matching the stored `Mod-` form
 * would ask the author to know the spelling this UI exists to hide.
 */
function matches(row: KeymapRow, query: string, byKey: boolean, isMac: boolean): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  if (byKey) {
    return row.chords.some((chord) =>
      formatChord(chord, isMac).toLowerCase().replace(/\s+/g, "").includes(needle.replace(/\s+/g, "")),
    );
  }
  return row.title.toLowerCase().includes(needle) || row.id.toLowerCase().includes(needle);
}
