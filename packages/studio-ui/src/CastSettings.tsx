/**
 * Settings ▸ Project ▸ Cast (decision log 2026-10-10): the colour each
 * speaker's lines are drawn in, over `brink.toml`'s `[cast]`.
 *
 * One row per declared speaker, as written in the file — the name, the
 * colour the file holds, a colour well and a remove button — and a row that
 * adds a speaker. A speaker the cast does not list keeps the Player's
 * automatic colour, so the list starts empty and holds only the speakers
 * the author cares about. Names match the script's cues ignoring case, so
 * `Mara` here colours a screenplay's `MARA`.
 *
 * Every edit is a targeted line edit through `@brink/studio-store`'s cast
 * helpers (comments and key order survive), written through
 * `project.applyEdit` like every other Project section; the re-applied
 * config recolours the Player.
 */

import { useEffect, useMemo, useReducer, useRef, useState } from "react";
import {
  castEntries,
  normalizeCastColor,
  removeCastMember,
  setCastColor,
} from "@brink/studio-store";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { isConfigPath } from "./ConfigFormPanel.js";
import { SettingsGroup, SettingsRow } from "./SettingsRow.js";

/** What a new speaker's colour well starts at. */
const NEW_SPEAKER_COLOR = "#5b8def";

export function CastSettings() {
  const storeApi = useStudioStoreApi();
  const outline = useStudioStore((s) => s.outline);
  const [version, bump] = useReducer((x: number) => x + 1, 0);

  const configPath = useMemo(
    () => outline.find((f) => !f.mounted && isConfigPath(f.path))?.path ?? null,
    [outline],
  );

  const source = useMemo(
    () =>
      configPath === null
        ? null
        : (storeApi.getState()._project?.getSession().getFileSource(configPath) ?? null),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- version is the re-read signal
    [storeApi, configPath, version, outline],
  );

  if (configPath === null || source === null) {
    return (
      <p className="settings-empty">
        This project has no <code>brink.toml</code>, so there is nothing to write the cast to yet.
      </p>
    );
  }

  /** Apply `edit` to the file's CURRENT text, then write it if it moved. */
  const write = (edit: (current: string) => string): void => {
    const project = storeApi.getState()._project;
    if (project === null) return;
    const current = project.getSession().getFileSource(configPath);
    if (current === null) return;
    const next = edit(current);
    if (next === current) return;
    project.applyEdit(configPath, next);
    const docs = storeApi.getState()._documents;
    docs?.refreshExternal(configPath);
    docs?.triggerCompile();
    bump();
  };

  const entries = castEntries(source);

  return (
    <SettingsGroup title="Cast">
      <p className="settings-group-hint">
        The colour each speaker&rsquo;s lines are drawn in, in the Player. Names match the
        script&rsquo;s cues ignoring case; a speaker not listed here keeps an automatic colour.
      </p>
      {entries.length === 0 && (
        <p className="cast-empty">No speakers yet. Add one below to choose their colour.</p>
      )}
      {entries.map((entry) => (
        <SettingsRow
          key={entry.name}
          title={entry.name}
          description={<code>{entry.color ?? "no colour"}</code>}
        >
          <div className="settings-row-actions">
            <CastColorInput
              value={normalizeCastColor(entry.color ?? "") ?? NEW_SPEAKER_COLOR}
              label={`Colour for ${entry.name}`}
              onCommit={(color) => write((text) => setCastColor(text, entry.name, color))}
            />
            <button
              type="button"
              className="settings-revert"
              title={`Remove ${entry.name} from the cast`}
              aria-label={`Remove ${entry.name} from the cast`}
              onClick={() => write((text) => removeCastMember(text, entry.name))}
            >
              {"×"}
            </button>
          </div>
        </SettingsRow>
      ))}
      <AddSpeaker
        taken={entries.map((e) => e.name)}
        onAdd={(name, color) => write((text) => setCastColor(text, name, color))}
      />
    </SettingsGroup>
  );
}

/**
 * A colour well that writes when the author has CHOSEN, not on every
 * movement of the picker: React's `onChange` on a colour input fires on
 * every `input` event while dragging, and each write re-applies the config.
 * The native `change` event is the commit.
 */
function CastColorInput({
  value,
  label,
  onCommit,
}: {
  value: string;
  label: string;
  onCommit: (color: string) => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = useRef(onCommit);
  commit.current = onCommit;
  useEffect(() => {
    const input = ref.current;
    if (input === null) return undefined;
    const onChange = (): void => {
      const color = normalizeCastColor(input.value);
      if (color !== null) commit.current(color);
    };
    input.addEventListener("change", onChange);
    return () => input.removeEventListener("change", onChange);
  }, []);
  return (
    <input
      ref={ref}
      type="color"
      className="cast-color"
      aria-label={label}
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
    />
  );
}

function AddSpeaker({
  taken,
  onAdd,
}: {
  taken: string[];
  onAdd: (name: string, color: string) => void;
}) {
  const [name, setName] = useState("");
  const [color, setColor] = useState(NEW_SPEAKER_COLOR);
  const trimmed = name.trim();
  const duplicate =
    trimmed !== "" && taken.some((t) => t.toLowerCase() === trimmed.toLowerCase());

  const submit = (): void => {
    if (trimmed === "" || duplicate) return;
    onAdd(trimmed, normalizeCastColor(color) ?? NEW_SPEAKER_COLOR);
    setName("");
  };

  return (
    <div className="cast-add">
      <div className="cast-add-row">
        <input
          type="color"
          className="cast-color"
          aria-label="Colour for the new speaker"
          value={color}
          onChange={(event) => setColor(event.target.value)}
        />
        <input
          className="cast-add-name"
          type="text"
          value={name}
          placeholder="Speaker name, as the script cues it"
          aria-label="Add a speaker"
          aria-invalid={duplicate}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              submit();
            }
          }}
        />
        <button
          type="button"
          className="settings-apply"
          onClick={submit}
          disabled={trimmed === "" || duplicate}
        >
          Add
        </button>
      </div>
      {duplicate && (
        <p className="cast-add-problem">
          {trimmed} is already in the cast — names match ignoring case.
        </p>
      )}
    </div>
  );
}
