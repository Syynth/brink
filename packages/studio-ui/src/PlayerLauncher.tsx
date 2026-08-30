/**
 * The idle Player's launcher (W14/#3307, spec §F17 RULED — the canvas's
 * IdleSaves artboard): "Run from the start" beside a typeahead over
 * knots/stitches (plays from there via the #186 start path), then the
 * checkpoint stores as two stacked sections in the landing screen's
 * Recent-list vocabulary — caps **PROJECT** and **THIS COMPUTER** over
 * bordered row lists. Rows: TURN-count mono chip (amber `OLD` when the
 * save's compile no longer matches), name, right-aligned muted context
 * (knot path · age); hover reveals Load / Fork / remove.
 */
import { useEffect, useMemo, useState } from "react";
import { useShell } from "@brink/studio-shell";
import type { SaveLocation, SaveSlotMeta } from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";
import { KnotIcon, StitchIcon } from "./icons.js";

interface SymbolHit {
  path: string; // "knot" | "knot.stitch"
  kind: "knot" | "stitch";
  file: string;
}

function age(savedAt: number, now: number): string {
  const s = Math.max(0, Math.round((now - savedAt) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  return `${Math.round(s / 86400)}d ago`;
}

function SaveSection({ title, location }: { title: string; location: SaveLocation }) {
  const slots = useStudioStore((s) => s.saveSlots[location]);
  const compiledChecksum = useStudioStore((s) => s.compiledChecksum);
  const loadSave = useStudioStore((s) => s.loadSave);
  const forkSave = useStudioStore((s) => s.forkSave);
  const removeSave = useStudioStore((s) => s.removeSave);
  const now = Date.now();

  return (
    <div className="pl-saves">
      <div className="pl-cap">{title}</div>
      <ul className="pl-save-list">
        {slots.length === 0 && <li className="pl-save-empty">no saves yet</li>}
        {slots.map((slot: SaveSlotMeta) => {
          const old = slot.checksum !== null && slot.checksum !== compiledChecksum;
          return (
            <li key={slot.id} className="pl-save-row">
              <span className={"pl-chip" + (old ? " pl-chip-old" : "")}>
                {old ? "OLD" : `T${slot.turn}`}
              </span>
              <span className="pl-save-name">{slot.name}</span>
              <span className="pl-save-context">
                {slot.knotPath ?? "—"} · {age(slot.savedAt, now)}
              </span>
              <span className="pl-save-actions">
                <button
                  type="button"
                  className="dp-mini"
                  title="Load — attach the session to this save (Save state writes back)"
                  onClick={() => void loadSave(location, slot.id)}
                >
                  load
                </button>
                <button
                  type="button"
                  className="dp-mini"
                  title="Fork — start from a copy; the next save picks a new slot"
                  onClick={() => void forkSave(location, slot.id)}
                >
                  fork
                </button>
                <button
                  type="button"
                  className="dp-x"
                  title="Delete this save"
                  aria-label={`Delete ${slot.name}`}
                  onClick={() => void removeSave(location, slot.id)}
                >
                  ×
                </button>
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

export function PlayerLauncher() {
  const { commands } = useShell();
  const outline = useStudioStore((s) => s.outline);
  const openSession = useStudioStore((s) => s.openSession);
  const refreshSaves = useStudioStore((s) => s.refreshSaves);
  const [query, setQuery] = useState("");

  useEffect(() => {
    void refreshSaves();
  }, [refreshSaves]);

  const symbols = useMemo(() => {
    const hits: SymbolHit[] = [];
    for (const file of outline) {
      for (const knot of file.symbols) {
        if (knot.kind !== "knot") continue;
        hits.push({ path: knot.name, kind: "knot", file: file.path });
        for (const child of knot.children) {
          if (child.kind === "stitch") {
            hits.push({
              path: `${knot.name}.${child.name}`,
              kind: "stitch",
              file: file.path,
            });
          }
        }
      }
    }
    return hits;
  }, [outline]);

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (q === "") return [];
    return symbols.filter((s) => s.path.toLowerCase().includes(q)).slice(0, 8);
  }, [query, symbols]);

  return (
    <div className="pl-launcher">
      <div className="pl-start-row">
        <button
          type="button"
          className="session-placeholder-start pl-run-btn"
          onClick={() => commands.dispatch("story.start")}
        >
          Run from the start
        </button>
        <div className="pl-typeahead">
          <input
            type="text"
            className="pl-typeahead-input"
            placeholder="…or play from a knot"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          {matches.length > 0 && (
            <ul className="pl-typeahead-list">
              {matches.map((m) => (
                <li key={m.path}>
                  <button
                    type="button"
                    className="pl-typeahead-row"
                    onClick={() => {
                      setQuery("");
                      openSession({ path: m.path, label: m.path });
                    }}
                  >
                    {/* The Binder's own symbol icons (maintainer feedback,
                        W14) — same tint classes, instantly familiar. */}
                    <span
                      className={
                        "pl-typeahead-icon " +
                        (m.kind === "knot"
                          ? "brink-binder-icon-knot"
                          : "brink-binder-icon-stitch")
                      }
                    >
                      {m.kind === "knot" ? <KnotIcon /> : <StitchIcon />}
                    </span>
                    <span className="pl-typeahead-text">
                      <span className="pl-save-name">{m.path}</span>
                      <span className="pl-typeahead-file">{m.file}</span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
      <SaveSection title="Project" location="project" />
      <SaveSection title="This computer" location="local" />
    </div>
  );
}
