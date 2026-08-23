/**
 * Structured form view for `brink.toml` (#3015) — rendered above the raw
 * text editor whenever a `brink.toml` is open, so the form and the text
 * stay visibly one document (the text below IS the escape hatch for
 * anything the form doesn't model, which is the issue's ruled shape).
 *
 * Every edit goes through the comment-preserving line editors in
 * `@brink/studio-store` (`setTomlString`) — never a parse-and-reserialize,
 * so an author's comments, key order, and unrelated tables survive.
 *
 * `entry` and `conventions` offer the project's ACTUAL files instead of
 * free text — a typo'd entry reproduces #3010 exactly, which is why this
 * form exists. A configured value that names a missing file is kept in
 * the list, flagged "(missing)", so the misconfiguration is visible
 * rather than silently re-written.
 */

import { useMemo, useReducer } from "react";
import { getTomlString, setTomlString } from "@brink/studio-store";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";

/** Whether `path` is a project config file (any directory's brink.toml). */
export function isConfigPath(path: string): boolean {
  return path === "brink.toml" || path.endsWith("/brink.toml");
}

interface FieldSpec {
  key: "entry" | "conventions" | "dialect" | "types";
  label: string;
  /** Fixed options (dialect/types), or null for file-backed fields. */
  options: { value: string; label: string }[] | null;
  hint: string;
}

const FIELDS: FieldSpec[] = [
  {
    key: "entry",
    label: "Entry file",
    options: null,
    hint: "What the project compiles from.",
  },
  {
    key: "conventions",
    label: "Conventions",
    options: null,
    hint: "A .brink module of @[convention] handlers.",
  },
  {
    key: "dialect",
    label: "Dialect",
    options: [
      { value: "strict-ink", label: "strict-ink" },
      { value: "brink", label: "brink" },
    ],
    hint: "Default: strict-ink.",
  },
  {
    key: "types",
    label: "Types",
    options: [
      { value: "gradual", label: "gradual" },
      { value: "strict", label: "strict (requires dialect = brink)" },
    ],
    hint: "Default follows the dialect.",
  },
];

export function ConfigFormPanel({ path }: { path: string }) {
  const storeApi = useStudioStoreApi();
  const outline = useStudioStore((s) => s.outline);
  // Re-read after our own applies (session content changes synchronously)
  // without waiting for the next compile's store refresh.
  const [version, bump] = useReducer((x: number) => x + 1, 0);

  const source = useMemo(
    () => storeApi.getState()._project?.getSession().getFileSource(path) ?? null,
    // eslint-disable-next-line react-hooks/exhaustive-deps -- version is the re-read signal
    [storeApi, path, version, outline],
  );

  const sourceFiles = useMemo(
    () =>
      outline
        .filter((f) => f.mounted !== true)
        .map((f) => f.path)
        .filter((p) => p.endsWith(".ink") || p.endsWith(".brink"))
        .sort(),
    [outline],
  );
  const brinkFiles = useMemo(() => sourceFiles.filter((p) => p.endsWith(".brink")), [sourceFiles]);

  if (source === null) return null;

  const apply = (key: string, value: string | null): void => {
    const project = storeApi.getState()._project;
    if (project === null) return;
    const current = project.getSession().getFileSource(path);
    if (current === null) return;
    const updated = setTomlString(current, "project", key, value);
    if (updated === current) return;
    project.applyEdit(path, updated);
    const docs = storeApi.getState()._documents;
    docs?.refreshExternal(path);
    docs?.triggerCompile();
    bump();
  };

  const fileOptions = (files: string[], current: string | null) => {
    const opts = files.map((p) => ({ value: p, label: p }));
    if (current !== null && current !== "" && !files.includes(current)) {
      opts.push({ value: current, label: `${current} (missing)` });
    }
    return opts;
  };

  return (
    <div className="brink-config-form" role="form" aria-label="Project configuration">
      <div className="config-form-head">
        <span className="config-form-title">Project configuration</span>
        <span className="config-form-hint">
          Structured edits — comments are preserved. Use the text below for anything the form
          doesn't cover (e.g. [lints]).
        </span>
      </div>
      <div className="config-form-fields">
        {FIELDS.map((field) => {
          const current = getTomlString(source, "project", field.key);
          const options =
            field.options ?? fileOptions(field.key === "conventions" ? brinkFiles : sourceFiles, current);
          return (
            <label key={field.key} className="config-form-field">
              <span className="config-form-label">{field.label}</span>
              <select
                className="config-form-select"
                data-config-key={field.key}
                value={current ?? ""}
                onChange={(e) => apply(field.key, e.target.value === "" ? null : e.target.value)}
              >
                <option value="">(not set)</option>
                {options.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>
              <span className="config-form-note">{field.hint}</span>
            </label>
          );
        })}
      </div>
    </div>
  );
}
