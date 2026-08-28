/**
 * `[project]` formatting settings (#3149, design round 2026-08-27).
 *
 * Indent size was configurable in `brink.toml` from #3149 and had no UI at
 * all — the only way to set it was the raw text. It is the one setting that
 * every component reads (the formatter emits it, the editor types it, the
 * guides are drawn against it), which is exactly why it deserves a control
 * rather than a line an author has to know exists.
 *
 * Deliberately its own section rather than a group inside General: it is
 * about how the project's TEXT is shaped, which is a different question
 * from what it compiles and how it is typed.
 */

import { useMemo, useReducer } from "react";
import { getTomlInteger, setTomlInteger } from "@brink/studio-store";
import { DEFAULT_INDENT } from "@brink-lang/editor";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { isConfigPath } from "./ConfigFormPanel.js";
import { SettingsGroup, SettingsRow, SettingsStepper } from "./SettingsRow.js";

/** `brink_project_config::INDENT_RANGE`. */
const MIN_INDENT = 1;
const MAX_INDENT = 16;

export function FormattingSettings() {
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
        This project has no <code>brink.toml</code>, so there is nothing to write formatting
        settings to yet.
      </p>
    );
  }

  const configured = getTomlInteger(source, "project", "indent");
  const indent = configured ?? DEFAULT_INDENT;

  const write = (next: number | null): void => {
    const project = storeApi.getState()._project;
    if (project === null) return;
    const current = project.getSession().getFileSource(configPath);
    if (current === null) return;
    const updated = setTomlInteger(current, "project", "indent", next);
    if (updated === current) return;
    project.applyEdit(configPath, updated);
    const docs = storeApi.getState()._documents;
    docs?.refreshExternal(configPath);
    docs?.triggerCompile();
    bump();
  };

  return (
    <SettingsGroup title="Indentation">
      <SettingsRow
        title="Indent size"
        description={
          <>
            One value, read by everything that indents — the formatter writes it, the editor
            types it, and the guides are drawn against it.
            {configured === null && ` Not set, so the default of ${DEFAULT_INDENT} applies.`}
          </>
        }
      >
        <div className="settings-row-actions">
          <SettingsStepper
            value={indent}
            min={MIN_INDENT}
            max={MAX_INDENT}
            label="indent size"
            suffix=" spaces"
            onChange={(next) => write(next)}
          />
          {configured !== null && (
            // Removing the key is not the same as writing the default value:
            // one says "this project has no opinion", the other pins it, and
            // only the first follows a later change to the default.
            <button
              type="button"
              className="settings-revert"
              title={`Remove the key — the default of ${DEFAULT_INDENT} applies`}
              aria-label="Reset indent size to the default"
              onClick={() => write(null)}
            >
              <svg
                width="12"
                height="12"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden
              >
                <path d="M9 14L4 9l5-5" />
                <path d="M4 9h11a5 5 0 0 1 0 10h-3" />
              </svg>
            </button>
          )}
        </div>
      </SettingsRow>

      <SettingsRow
        title={
          <>
            Indent character <span className="settings-row-badge">not ruled</span>
          </>
        }
        description={
          <>
            The formatter already models tabs, but <code>brink.toml</code> has no key for it —
            tabs-vs-spaces was deliberately left out of the indent-size ruling.
          </>
        }
      >
        <div className="settings-segmented is-disabled" aria-disabled="true">
          <span className="on">spaces</span>
          <span>tabs</span>
        </div>
      </SettingsRow>
    </SettingsGroup>
  );
}
