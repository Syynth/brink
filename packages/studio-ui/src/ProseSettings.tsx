/**
 * `[prose]` settings — the project's spell/grammar checking (#3211).
 *
 * Dialect is not a nicety and not deferrable. Measured against the checker
 * with the American dialect, `"The colour of the harbour at night."` reports
 * BOTH words as misspellings — so a British-English author with no way to
 * say so gets their entire manuscript underlined, which is indistinguishable
 * from the feature being broken.
 *
 * `enable` is separate from whether a checker is registered at all: an
 * embedder decides whether the engine is available (it is a separate 6.5 MB
 * module), and this decides whether a project that HAS it wants its prose
 * checked. Those are different decisions by different people.
 */

import { useMemo, useReducer } from "react";
import { getTomlBool, setTomlBool, setTomlString } from "@brink/studio-store";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { isConfigPath } from "./ConfigFormPanel.js";
import { SettingsGroup, SettingsRow, SettingsToggle } from "./SettingsRow.js";

/** Must match `brink_project_config::ProseDialect::as_str`. */
const DIALECTS = [
  { id: "american", label: "American" },
  { id: "british", label: "British" },
  { id: "canadian", label: "Canadian" },
  { id: "australian", label: "Australian" },
] as const;

const DEFAULT_DIALECT = "american";

/** Read `[prose] dialect` out of the raw config text. */
function readDialect(source: string): string {
  // The line-oriented read the other config helpers use — comment-preserving
  // edits mean the file is text, not a parsed model.
  const match = /^\s*dialect\s*=\s*"([^"]*)"/m.exec(proseSection(source));
  return match?.[1] ?? DEFAULT_DIALECT;
}

/**
 * The `[prose]` table's body.
 *
 * Scoped rather than searched whole-file, because `[project]` has a
 * `dialect` key of its own — the ink/native surface dialect — and reading
 * that one here would silently show the wrong value and then overwrite it.
 */
function proseSection(source: string): string {
  const start = /^\s*\[prose\]\s*$/m.exec(source);
  if (start === null) return "";
  const after = source.slice(start.index + start[0].length);
  const next = /^\s*\[[^\]]+\]\s*$/m.exec(after);
  return next === null ? after : after.slice(0, next.index);
}

export function ProseSettings() {
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
        This project has no <code>brink.toml</code>, so there is nothing to write prose
        settings to yet.
      </p>
    );
  }

  const dialect = readDialect(source);
  const enabled = getTomlBool(source, "prose", "enable") ?? true;

  const write = (next: string): void => {
    const project = storeApi.getState()._project;
    if (project === null) return;
    const current = project.getSession().getFileSource(configPath);
    if (current === null || next === current) return;
    project.applyEdit(configPath, next);
    const docs = storeApi.getState()._documents;
    docs?.refreshExternal(configPath);
    docs?.triggerCompile();
    bump();
  };

  return (
    <>
      <SettingsGroup title="Checking">
        <SettingsRow
          title="Check prose"
          description="Spelling and light grammar over the manuscript's prose — never over diverts, tags, or logic."
        >
          <SettingsToggle
            checked={enabled}
            label="Check prose"
            onChange={(next) => write(setTomlBool(source, "prose", "enable", next))}
          />
        </SettingsRow>

        <SettingsRow
          title="Dialect"
          description="Which English the checker judges by. Set this before anything else — under the wrong dialect an author who writes “colour” sees their whole manuscript underlined."
        >
          <select
            className="settings-select"
            value={dialect}
            onChange={(event) =>
              write(setTomlString(source, "prose", "dialect", event.target.value))
            }
          >
            {DIALECTS.map((d) => (
              <option key={d.id} value={d.id}>
                {d.label}
              </option>
            ))}
          </select>
        </SettingsRow>
      </SettingsGroup>

      <p className="settings-group-hint">
        Your project&rsquo;s own names — knots, stitches, and the character cues that say
        who the story is about — are known words automatically. Nothing to configure:
        writing the manuscript teaches the dictionary.
      </p>
    </>
  );
}
