/**
 * The `[lints]` section of Settings (#3148, design round 2026-08-27).
 *
 * Two lists, and **which list a code is in IS whether it is in
 * `brink.toml`** — there is no second state to read. "Configure" moves a
 * code up, writing the key at its CURRENT default so the first click
 * changes nothing about the build; the down arrow moves it back out,
 * removing the key.
 *
 * What is listed is decided by the compiler, not by this file:
 *
 * - `getDiagnosticRegistry()` is the code list. A hand-maintained copy here
 *   would be wrong the moment a code is added, and wrong silently — the
 *   missing code simply never appears (#3169).
 * - `overridable` gates the lower list. Only 30 of 189 codes can be set at
 *   all; offering a level picker for the rest would build the silent no-op
 *   this surface exists to prevent.
 * - `surfaces` filters by what this project can actually produce, so a
 *   `.ink`-only project is not offered settings for `.brink` markup spans.
 *
 * Severity glyphs are the Problems panel's own, showing each code's
 * EFFECTIVE level — so a row reads the same here as the problem it will
 * produce.
 */

import { useMemo, useReducer, useState } from "react";
import {
  getTomlBool,
  getTomlString,
  setTomlBool,
  setTomlString,
  tomlTableKeys,
} from "@brink/studio-store";
import type { DiagnosticInfo } from "@brink-lang/web";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { isConfigPath } from "./ConfigFormPanel.js";

/** The levels `[lints]` accepts, in escalating order. */
const LEVELS = ["allow", "hint", "warn", "deny"] as const;
type Level = (typeof LEVELS)[number];

const isLevel = (v: string | null): v is Level =>
  v !== null && (LEVELS as readonly string[]).includes(v);

/** The Problems panel's own glyphs (`BUCKET_GLYPH` in ProblemsView). */
const GLYPH = { error: "●", warning: "▲", info: "ℹ" } as const;

/**
 * The Problems bucket a code's EFFECTIVE level lands in.
 *
 * `allow` produces no problem at all, so it gets no glyph rather than a
 * quiet one — an author scanning the list should see nothing where nothing
 * will be reported.
 */
function bucketFor(level: Level | null, fallback: DiagnosticInfo["default_severity"]) {
  if (level === null) return fallback;
  if (level === "allow") return null;
  if (level === "deny") return "error" as const;
  if (level === "warn") return "warning" as const;
  return "info" as const;
}

function SeverityGlyph({ bucket }: { bucket: "error" | "warning" | "info" | null }) {
  if (bucket === null) {
    return <span className="lint-sev is-allow" aria-label="no problem reported" />;
  }
  return (
    <span className={`lint-sev is-${bucket}`} aria-label={bucket}>
      {GLYPH[bucket]}
    </span>
  );
}

function LevelPicker({
  value,
  onPick,
}: {
  value: Level | null;
  onPick: (level: Level) => void;
}) {
  return (
    <div className="lint-levels" role="group">
      {LEVELS.map((level) => (
        <button
          key={level}
          type="button"
          className={"lint-level" + (value === level ? ` on is-${level}` : "")}
          aria-pressed={value === level}
          onClick={() => onPick(level)}
        >
          {level}
        </button>
      ))}
    </div>
  );
}

/** One row in either list. */
function LintRow({
  info,
  level,
  onPick,
  onConfigure,
  onRemove,
}: {
  info: DiagnosticInfo;
  /** The level written in `brink.toml`, or null when unconfigured. */
  level: Level | null;
  onPick?: (level: Level) => void;
  onConfigure?: () => void;
  onRemove?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const configured = level !== null;
  const hasExplanation = info.explanation !== undefined;

  return (
    <div className={"lint-row" + (configured ? " is-configured" : "")}>
      <div className="lint-row-main">
        {hasExplanation ? (
          <button
            type="button"
            className={"lint-disclose" + (open ? " open" : "")}
            aria-expanded={open}
            aria-label={`Explain ${info.code}`}
            onClick={() => setOpen((v) => !v)}
          >
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <path d={open ? "M6 9l6 6 6-6" : "M9 6l6 6-6 6"} />
            </svg>
          </button>
        ) : (
          <span className="lint-disclose is-empty" aria-hidden="true" />
        )}
        <SeverityGlyph bucket={bucketFor(level, info.default_severity)} />
        <span className="lint-code">{info.code}</span>
        <span className="lint-title">{info.title}</span>
        {configured ? (
          <>
            <span className="lint-default">default {info.default_severity}</span>
            {onPick && <LevelPicker value={level} onPick={onPick} />}
            <button
              type="button"
              className="lint-move"
              title="Stop configuring this code — removes it from brink.toml"
              aria-label={`Stop configuring ${info.code}`}
              onClick={onRemove}
            >
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 5v14M6 13l6 6 6-6" />
              </svg>
            </button>
          </>
        ) : (
          <>
            <span className="lint-default">{info.default_severity}</span>
            <button type="button" className="lint-configure" onClick={onConfigure}>
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 19V5M6 11l6-6 6 6" />
              </svg>
              Configure
            </button>
          </>
        )}
      </div>
      {open && info.explanation !== undefined && (
        <p className="lint-explanation">{info.explanation}</p>
      )}
    </div>
  );
}

/** Group rows under their category heading, categories in first-seen order. */
function grouped(rows: DiagnosticInfo[]): [string, DiagnosticInfo[]][] {
  const out = new Map<string, DiagnosticInfo[]>();
  for (const row of rows) {
    const key = row.category ?? "Other";
    const list = out.get(key);
    if (list) list.push(row);
    else out.set(key, [row]);
  }
  return [...out.entries()];
}

export function LintSettings() {
  const storeApi = useStudioStoreApi();
  const outline = useStudioStore((s) => s.outline);
  const [version, bump] = useReducer((x: number) => x + 1, 0);
  const [query, setQuery] = useState("");

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

  const registry = useMemo<DiagnosticInfo[]>(() => {
    const project = storeApi.getState()._project as
      | { getDiagnosticRegistry?: () => DiagnosticInfo[] }
      | null;
    // Feature-detected on the ProjectSession, which is itself feature-
    // detected against the wasm handle: an older build degrades to an empty
    // section rather than throwing out of the Settings render.
    return typeof project?.getDiagnosticRegistry === "function"
      ? project.getDiagnosticRegistry()
      : [];
    // `outline` is the readiness signal, not a dependency of the data: the
    // registry is static for a build, but `_project` is null until the
    // session exists, and `storeApi` alone is stable — so keying on it
    // alone captured an empty registry on first render and kept it forever.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- see above
  }, [storeApi, outline]);

  /**
   * Which surfaces this project actually writes. Derived from the files it
   * has rather than from `dialect`: a `.brink` file is the native surface
   * whatever the dialect says, and the question here is "can this project
   * produce that diagnostic", not "what dialect is configured".
   */
  const surfaces = useMemo(() => {
    const own = outline.filter((f) => !f.mounted);
    const set = new Set<string>();
    if (own.some((f) => f.path.endsWith(".ink"))) set.add("ink");
    if (own.some((f) => f.path.endsWith(".brink"))) set.add("native");
    // Before the first outline arrives, show everything rather than
    // nothing — an empty list would read as "no diagnostics exist".
    return set.size === 0 ? new Set(["ink", "native"]) : set;
  }, [outline]);

  const write = (next: string): void => {
    const project = storeApi.getState()._project;
    if (project === null || configPath === null || source === null) return;
    if (next === source) return;
    project.applyEdit(configPath, next);
    const docs = storeApi.getState()._documents;
    docs?.refreshExternal(configPath);
    docs?.triggerCompile();
    bump();
  };

  const setLevel = (code: string, level: Level | null): void => {
    if (source === null) return;
    write(setTomlString(source, "lints", code, level));
  };

  const denyWarnings = source === null ? null : getTomlBool(source, "lints", "deny-warnings");

  /** Codes named in `[lints]`, excluding the policy key. */
  const configuredCodes = useMemo(
    () => (source === null ? [] : tomlTableKeys(source, "lints").filter((k) => k !== "deny-warnings")),
    [source],
  );

  const byCode = useMemo(
    () => new Map(registry.map((r) => [r.code, r])),
    [registry],
  );

  const applicable = useMemo(
    () => registry.filter((r) => r.surfaces.some((s) => surfaces.has(s))),
    [registry, surfaces],
  );

  const configured = configuredCodes
    .map((code) => byCode.get(code))
    .filter((r): r is DiagnosticInfo => r !== undefined);

  /** Codes in the file this compiler does not know — kept, never dropped. */
  const unknown = configuredCodes.filter((code) => !byCode.has(code));

  const unconfigured = applicable.filter(
    (r) => r.overridable && !configuredCodes.includes(r.code),
  );

  const needle = query.trim().toLowerCase();
  const matches = needle === ""
    ? unconfigured
    : unconfigured.filter(
        (r) =>
          r.code.toLowerCase().includes(needle) ||
          r.title.toLowerCase().includes(needle) ||
          (r.category ?? "").toLowerCase().includes(needle),
      );

  if (configPath === null || source === null) {
    return (
      <section className="settings-section">
        <h2 className="settings-section-title">Diagnostics</h2>
        <p className="settings-section-hint">
          This project has no <code>brink.toml</code>, so there is nothing to configure
          diagnostics in yet.
        </p>
      </section>
    );
  }

  return (
    <section className="settings-section lint-settings">
      <h2 className="settings-section-title">Diagnostics</h2>
      <p className="settings-section-hint">
        Written to <code>[lints]</code> in <code>{configPath}</code>.
      </p>

      <label className="lint-policy">
        <input
          type="checkbox"
          checked={denyWarnings === true}
          onChange={(e) => write(setTomlBool(source, "lints", "deny-warnings", e.target.checked || null))}
        />
        <span className="lint-policy-label">Deny warnings</span>
        <span className="lint-policy-hint">
          Promote every warning to an error, the way <code>-D warnings</code> does.
        </span>
      </label>

      <div className="lint-list-head">
        <span className="lint-list-title">Project lint configuration</span>
        <span className="lint-list-count">
          {configured.length + unknown.length} code
          {configured.length + unknown.length === 1 ? "" : "s"}
        </span>
      </div>
      {configured.length === 0 && unknown.length === 0 ? (
        <p className="lint-empty">
          Nothing configured — every diagnostic is running at its built-in default.
        </p>
      ) : (
        <div className="lint-list">
          {grouped(configured).map(([category, rows]) => (
            <div key={category} className="lint-group">
              <div className="lint-group-head">{category}</div>
              {rows.map((info) => {
                const raw = getTomlString(source, "lints", info.code);
                return (
                  <LintRow
                    key={info.code}
                    info={info}
                    level={isLevel(raw) ? raw : null}
                    onPick={(level) => setLevel(info.code, level)}
                    onRemove={() => setLevel(info.code, null)}
                  />
                );
              })}
            </div>
          ))}
          {unknown.length > 0 && (
            <div className="lint-group">
              <div className="lint-group-head">Unknown to this compiler</div>
              {unknown.map((code) => (
                <div key={code} className="lint-row is-unknown">
                  <div className="lint-row-main">
                    <span className="lint-disclose is-empty" aria-hidden="true" />
                    <span className="lint-sev is-warning">{GLYPH.warning}</span>
                    <span className="lint-code">{code}</span>
                    <span className="lint-title">
                      Kept — it may belong to a newer compiler.
                    </span>
                    <button
                      type="button"
                      className="lint-move"
                      aria-label={`Remove ${code}`}
                      onClick={() => setLevel(code, null)}
                    >
                      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                        <path d="M12 5v14M6 13l6 6 6-6" />
                      </svg>
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      <div className="lint-list-head">
        <span className="lint-list-title is-muted">Not configured</span>
        <span className="lint-list-hint">Running at their built-in defaults.</span>
        <input
          type="search"
          className="lint-search"
          placeholder="Search codes"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <span className="lint-list-count">
          {matches.length}/{unconfigured.length}
        </span>
      </div>
      <div className="lint-list">
        {grouped(matches).map(([category, rows]) => (
          <div key={category} className="lint-group">
            <div className="lint-group-head">{category}</div>
            {rows.map((info) => (
              <LintRow
                key={info.code}
                info={info}
                level={null}
                // Writes the key at its CURRENT default, so the first click
                // brings the code under the project's control without
                // changing what the build does.
                onConfigure={() =>
                  setLevel(
                    info.code,
                    info.default_severity === "error"
                      ? "deny"
                      : info.default_severity === "warning"
                        ? "warn"
                        : "hint",
                  )
                }
              />
            ))}
          </div>
        ))}
        {matches.length === 0 && (
          <p className="lint-empty">
            {unconfigured.length === 0
              ? "Every configurable diagnostic is already in the list above."
              : `No code matches “${query}”.`}
          </p>
        )}
      </div>
    </section>
  );
}
