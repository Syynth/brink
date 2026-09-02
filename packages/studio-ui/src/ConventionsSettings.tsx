/**
 * Settings → Conventions: the teach-by-example editor (#3411; RULED
 * 2026-09-02, "Conventions editor: teach-by-example is the design
 * direction").
 *
 * The author points at a passage (the Player launcher's knot/stitch
 * typeahead, or pasted lines), marks what each line is, and the studio
 * shows back what it learned — plain sentences with the lines that
 * support each — plus what it could not settle. Nothing is written until
 * "Use these rules"; the write goes through the `[dialogue]` section
 * road (#3410), which asks before replacing a section it did not write.
 *
 * Deliberately NOT modelled on `ConfigFormPanel`.
 */

import { useEffect, useMemo, useReducer, useState } from "react";
import type { PassageLine } from "@brink/wasm-types";
import {
  inferDialect,
  toDialogueConfig,
  type Inference,
  type Mark,
  type MarkedLine,
} from "@brink-lang/editor";
import {
  findDialogueSection,
  renderDialogueSection,
  setDialogueSection,
  type DialogueSection,
  type DialogueSpec,
  type TranscriptLine,
} from "@brink/studio-store";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { isConfigPath } from "./ConfigFormPanel.js";
import { SettingsGroup, SettingsRow } from "./SettingsRow.js";
import { KnotIcon, StitchIcon } from "./icons.js";
import { foldPlayerRuns, speakerPaletteIndex } from "./player-runs.js";

interface SymbolHit {
  path: string;
  kind: "knot" | "stitch";
  file: string;
}

const MARKS: ReadonlyArray<{ mark: Mark; label: string }> = [
  { mark: "cue", label: "Cue" },
  { mark: "dialogue", label: "Dialogue" },
  { mark: "action", label: "Action" },
  { mark: "narration", label: "Narration" },
  { mark: "parenthetical", label: "Aside" },
];

const SPEAKER_PALETTE_SIZE = 6;
const ARTIFACT_FILE = "dialect.json";

interface Passage {
  label: string;
  lines: PassageLine[];
}

function dirOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i + 1);
}

export function ConventionsSettings() {
  const storeApi = useStudioStoreApi();
  const outline = useStudioStore((s) => s.outline);
  const projectDialect = useStudioStore((s) => s.projectDialect);
  const [version, bump] = useReducer((x: number) => x + 1, 0);

  const [query, setQuery] = useState("");
  const [passage, setPassage] = useState<Passage | null>(null);
  const [pasting, setPasting] = useState(false);
  const [pasted, setPasted] = useState("");
  const [marks, setMarks] = useState<ReadonlyMap<number, Mark>>(new Map());
  const [replaceAsk, setReplaceAsk] = useState<DialogueSection | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const configPath = useMemo(
    () => outline.find((f) => !f.mounted && isConfigPath(f.path))?.path ?? null,
    [outline],
  );

  const symbols = useMemo(() => {
    const hits: SymbolHit[] = [];
    for (const file of outline) {
      if (file.mounted) continue;
      for (const knot of file.symbols) {
        if (knot.kind !== "knot") continue;
        hits.push({ path: knot.name, kind: "knot", file: file.path });
        for (const child of knot.children) {
          if (child.kind === "stitch") {
            hits.push({ path: `${knot.name}.${child.name}`, kind: "stitch", file: file.path });
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
  }, [symbols, query]);

  const pick = (hit: SymbolHit): void => {
    const project = storeApi.getState()._project;
    const lines = project?.passageLines(hit.path) ?? null;
    setQuery("");
    setPasting(false);
    setMarks(new Map());
    setStatus(null);
    if (lines === null || lines.length === 0) {
      setPassage(null);
      setStatus(`${hit.path} has no lines to mark.`);
      return;
    }
    setPassage({ label: hit.path, lines });
  };

  const usePasted = (): void => {
    const lines = pasted
      .split("\n")
      .map((t) => t.trim())
      .filter((t) => t !== "")
      .map((text, i) => ({ text, tags: [], file: "", line: i + 1, origin: "line" as const }));
    setMarks(new Map());
    setStatus(null);
    setPassage(lines.length === 0 ? null : { label: "pasted lines", lines });
    setPasting(false);
  };

  const marked: MarkedLine[] = useMemo(
    () =>
      (passage?.lines ?? []).map((l, i) => {
        const mark = marks.get(i);
        return { text: l.text, tags: l.tags, origin: l.origin, ...(mark ? { mark } : {}) };
      }),
    [passage, marks],
  );

  const inference: Inference | null = useMemo(
    () => (marks.size === 0 ? null : inferDialect(marked)),
    [marked, marks.size],
  );

  // Preview: the passage as the Player would fold it, under the proposed
  // dialect (or the project's current one until something is marked).
  const previewDialect = inference?.dialect ?? projectDialect;
  const groups = useMemo(() => {
    // The Player sees EMITTED lines, so apply what the runtime would: a
    // line ending in glue (`<>`) joins the next one. Without this a glued
    // cue shows up as its own `<>` row under the speaker header.
    const lines: TranscriptLine[] = [];
    let carry: { text: string; tags: string[] } | null = null;
    for (const l of passage?.lines ?? []) {
      const text = carry === null ? l.text : carry.text + l.text;
      const tags = carry === null ? l.tags : [...carry.tags, ...l.tags];
      if (text.endsWith("<>")) {
        carry = { text: text.slice(0, -2), tags };
        continue;
      }
      carry = null;
      lines.push({ text, kind: "line", tags });
    }
    if (carry !== null) lines.push({ text: carry.text, kind: "line", tags: carry.tags });
    return foldPlayerRuns(lines, previewDialect);
  }, [passage, previewDialect]);

  const flagged = useMemo(() => {
    const set = new Set<number>();
    for (const d of inference?.decisions ?? []) for (const i of d.lines) set.add(i);
    return set;
  }, [inference]);

  const toggle = (index: number, mark: Mark): void => {
    setMarks((prev) => {
      const next = new Map(prev);
      if (next.get(index) === mark) next.delete(index);
      else next.set(index, mark);
      return next;
    });
    setReplaceAsk(null);
  };

  const canConfirm =
    inference !== null && inference.dialect !== null && inference.decisions.length === 0;

  const write = (force: boolean): void => {
    if (!canConfirm || inference?.dialect == null) return;
    const project = storeApi.getState()._project;
    if (project === null) return;
    const dialect = inference.dialect;
    const table = toDialogueConfig(dialect);
    const spec: DialogueSpec = table
      ? { form: "table", table }
      : { form: "file", file: ARTIFACT_FILE };
    const path = configPath ?? "brink.toml";
    const current = project.getSession().getFileSource(path);
    const existing = current === null ? null : findDialogueSection(current);
    if (existing !== null && existing.owner !== "editor" && !force) {
      setReplaceAsk(existing);
      return;
    }
    const next = setDialogueSection(current ?? "", renderDialogueSection(spec));
    if (current === null) void project.addFile(path, next);
    else project.applyEdit(path, next);
    if (spec.form === "file") {
      const artifactPath = `${dirOf(path)}${ARTIFACT_FILE}`;
      const json = `${JSON.stringify(dialect, null, 2)}\n`;
      if (project.getSession().getFileSource(artifactPath) === null) {
        void project.addFile(artifactPath, json);
      } else {
        project.applyEdit(artifactPath, json);
      }
    }
    const docs = storeApi.getState()._documents;
    docs?.refreshExternal(path);
    docs?.triggerCompile();
    setReplaceAsk(null);
    setStatus(
      spec.form === "table"
        ? `Written to ${path} as the ${spec.table.preset ?? "project"} recipe with your rules.`
        : `Written: ${path} now points at ${ARTIFACT_FILE}, which holds your rules in full.`,
    );
    bump();
  };

  // A compile after the write refreshes `projectDialect`; nothing else to
  // re-read here, but keep the memo chain honest about the version.
  useEffect(() => {
    /* version is a re-read signal for the status line */
  }, [version]);

  const current =
    projectDialect === null
      ? "None — lines print as plain text."
      : `${projectDialect.name ?? "project"} — ${(projectDialect.elements ?? []).map((e) => e.kind).join(", ")}`;

  return (
    <>
      <SettingsGroup title="Teach the studio your script">
        <p className="settings-group-hint">
          Point at a passage the way you actually write it, then mark what each line is. The
          studio works out the rules and shows them back to you before anything is saved.
        </p>
        <SettingsRow title="Current conventions" description={current}>
          <span className="settings-value sv-mono">{projectDialect === null ? "none" : (projectDialect.name ?? "project")}</span>
        </SettingsRow>
      </SettingsGroup>

      <div className="conv-editor">
        <div className="conv-lines">
          <div className="settings-group-label">Your lines</div>
          <div className="conv-picker">
            {pasting ? (
              <textarea
                className="settings-preview-input sv-mono"
                rows={6}
                value={pasted}
                placeholder="Paste a few lines the way you write them"
                aria-label="Pasted lines"
                spellCheck={false}
                onChange={(e) => setPasted(e.target.value)}
              />
            ) : (
              <input
                type="text"
                className="pl-typeahead-input"
                aria-label="Pull lines from a knot or stitch"
                placeholder={passage === null ? "Pull lines from a knot or stitch…" : passage.label}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            )}
            {pasting ? (
              <button type="button" className="settings-apply" onClick={usePasted}>
                Use these lines
              </button>
            ) : (
              <button type="button" className="settings-apply" onClick={() => setPasting(true)}>
                Paste instead
              </button>
            )}
            {matches.length > 0 && !pasting && (
              <ul className="pl-typeahead-list" role="listbox" aria-label="Matching knots and stitches">
                {matches.map((m) => (
                  <li key={m.path}>
                    <button type="button" className="pl-typeahead-row" onClick={() => pick(m)}>
                      <span
                        className={
                          "pl-typeahead-icon " +
                          (m.kind === "knot" ? "brink-binder-icon-knot" : "brink-binder-icon-stitch")
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
          {passage !== null && (
            <>
              <div className="conv-line-count">
                {passage.label} · {passage.lines.length} {passage.lines.length === 1 ? "line" : "lines"}
              </div>
              <div className="conv-line-list" role="list">
                {passage.lines.map((l, i) => (
                  <div
                    key={i}
                    role="listitem"
                    className={"conv-line" + (flagged.has(i) ? " is-flagged" : "")}
                  >
                    <span className="conv-line-text sv-mono" title={l.text}>
                      {l.text}
                    </span>
                    <div className="conv-marks" role="group" aria-label={`Line ${(i + 1).toString()}`}>
                      {MARKS.map(({ mark, label }) => (
                        <button
                          key={mark}
                          type="button"
                          className={"conv-mark" + (marks.get(i) === mark ? " on" : "")}
                          aria-pressed={marks.get(i) === mark}
                          aria-label={`Mark line ${(i + 1).toString()} as ${label}`}
                          onClick={() => toggle(i, mark)}
                        >
                          {label}
                        </button>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
              <p className="settings-group-hint">
                Mark at least one of each kind you use. Lines you leave unmarked are checked
                against the rules, not taught from.
              </p>
            </>
          )}
          {status !== null && <p className="conv-status">{status}</p>}
        </div>

        <div className="conv-preview">
          <div className="settings-group-label">How it reads in the Player</div>
          <div className="player settings-conv-player">
            <div className="story-text">
              {passage === null ? (
                <p className="conv-preview-empty">Pick a passage to see it here.</p>
              ) : (
                groups.map((group, gi) => {
                  const rows = group.rows.map((row) => {
                    const text =
                      row.segments.length === 0
                        ? row.line.text
                        : row.segments
                            .filter((s, si) => !(si === 0 && group.speaker !== null && s.kind === group.kind))
                            .map((s) => s.text)
                            .join("");
                    return (
                      <div
                        key={row.index}
                        className={`player-line-row kind-line${row.kind ? ` dialect-${row.kind}` : ""}`}
                      >
                        <p>{text.trim() === "" ? " " : text}</p>
                      </div>
                    );
                  });
                  if (group.speaker === null) return rows;
                  const palette = speakerPaletteIndex(group.speaker, SPEAKER_PALETTE_SIZE);
                  return (
                    <div key={`g${gi.toString()}`} className={`player-run dialect-${group.kind ?? "run"}`}>
                      <p className={`player-run-cue speaker-${palette.toString()}`}>{group.speaker}</p>
                      {rows}
                    </div>
                  );
                })
              )}
            </div>
          </div>
          {passage !== null && <p className="settings-group-hint">Updates as you mark lines.</p>}
        </div>
      </div>

      {inference !== null && (
        <SettingsGroup title="What the studio learned">
          {inference.learned.length === 0 && inference.decisions.length === 0 && (
            <p className="settings-empty">Nothing yet — mark a cue to start.</p>
          )}
          <div className="conv-learned" role="list">
            {inference.learned.map((l) => (
              <div key={l.id} role="listitem" className="conv-learned-row">
                <span className="conv-learned-mark is-ok" aria-hidden="true">
                  ✓
                </span>
                <span className="conv-learned-text">{l.sentence}</span>
                <span className="conv-learned-count sv-mono">
                  {l.support.length.toString()} of {l.total.toString()}
                </span>
              </div>
            ))}
            {inference.decisions.map((d) => (
              <div key={d.id} role="listitem" className="conv-learned-row is-decision">
                <span className="conv-learned-mark is-decision" aria-hidden="true">
                  ✕
                </span>
                <span className="conv-learned-text">
                  {d.message}
                  {d.lines.length > 0 && (
                    <span className="conv-learned-lines sv-mono">
                      {" "}
                      (line{d.lines.length === 1 ? "" : "s"} {d.lines.map((i) => i + 1).join(", ")})
                    </span>
                  )}
                </span>
                <span className="conv-learned-count sv-mono">needs a decision</span>
              </div>
            ))}
          </div>
        </SettingsGroup>
      )}

      {replaceAsk !== null && (
        <div className="conv-ask" role="alert">
          <p className="conv-ask-text">
            {replaceAsk.owner === "hand"
              ? "brink.toml already has a [dialogue] section written by hand. Replace it with these rules?"
              : "The [dialogue] section in brink.toml was edited since the studio last wrote it. Replace it with these rules?"}
          </p>
          <pre className="conv-ask-block sv-mono">{replaceAsk.text}</pre>
          <div className="settings-row-actions">
            <button type="button" className="settings-apply" onClick={() => setReplaceAsk(null)}>
              Keep it
            </button>
            <button type="button" className="settings-apply is-primary" onClick={() => write(true)}>
              Replace it
            </button>
          </div>
        </div>
      )}

      <div className="conv-footer">
        <span className="settings-group-hint">Nothing is written until you confirm.</span>
        <span className="conv-footer-spacer" />
        <button
          type="button"
          className="settings-apply"
          onClick={() => {
            setMarks(new Map());
            setReplaceAsk(null);
            setStatus(null);
          }}
          disabled={marks.size === 0}
        >
          Start over
        </button>
        <button
          type="button"
          className="settings-apply is-primary"
          onClick={() => write(false)}
          disabled={!canConfirm}
          title={
            canConfirm
              ? undefined
              : inference === null
                ? "Mark some lines first"
                : "Settle the decisions above first"
          }
        >
          Use these rules
        </button>
      </div>
    </>
  );
}
