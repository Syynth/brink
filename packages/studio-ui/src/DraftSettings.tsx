/**
 * `[project] drafts` settings — the author's draft patterns (#3145).
 *
 * The globs have been readable by the compiler since #3145 and editable
 * nowhere: an author could only reach them by hand-editing `brink.toml`,
 * and nothing showed whether what they typed had worked.
 *
 * A list of glob strings is a poor thing to show back on its own, because
 * the two ordinary mistakes are both invisible in it. A pattern that
 * matches nothing — a typo, or a folder since renamed — looks exactly like
 * one that is working. And a pattern matching a file the story still
 * reaches produces no draft at all, under the "reachability wins" ruling
 * (2026-08-27): draft status can never remove a file the story needs, so a
 * glob naming one silently does nothing.
 *
 * So each row is shown with what it currently matches, split those two
 * ways. The split is read from `getDraftGlobReport`, never recomputed here:
 * the ruling has one implementation, on the Rust side, and a second one in
 * TS would be free to drift from it.
 */

import { useEffect, useMemo, useReducer, useState } from "react";
import type { DraftGlobReport } from "@brink-lang/web";
import {
  PRUNABLE_DIRS,
  draftGlobProblem,
  draftGlobs,
  isUnpruned,
  withDraftGlob,
  withUnprunedDir,
  withoutDraftGlob,
  withoutUnprunedDir,
} from "@brink/studio-store";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { isConfigPath } from "./ConfigFormPanel.js";
import { SettingsGroup, SettingsRow, SettingsToggle } from "./SettingsRow.js";

export function DraftSettings() {
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

  // Pulled through `projectQuery`, NOT off `getSession()` directly. The
  // session the main thread holds is not the one that compiles: when the
  // worker road is live the replica does, and only the replica has a
  // compilation closure. Asking the main-thread session returns a report
  // that is structurally valid and always says `compiled: false` — so every
  // pattern renders as "not checked yet" and the panel looks broken in the
  // real studio while its unit tests, which stub the session, stay green.
  //
  // Re-pulled on `outline` as well as `version`: the match sets change on
  // every compile, not only when this panel writes.
  const [report, setReport] = useState<DraftGlobReport | null>(null);
  useEffect(() => {
    let live = true;
    const project = storeApi.getState()._project;
    if (project === null) return;
    project
      .projectQuery<DraftGlobReport>("getDraftGlobReport", [], {
        coalesceKey: "settings:draft-globs",
      })
      .then((next) => {
        if (live) setReport(next);
      })
      .catch(() => {
        // Superseded by a newer pull, or the session is tearing down. Keep
        // the last good report rather than flashing every row back to
        // "not checked yet".
      });
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- version/outline are the re-pull signals
  }, [storeApi, version, outline]);

  if (configPath === null || source === null) return null;

  const globs = draftGlobs(source);

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
    <SettingsGroup title="Drafts">
      <p className="settings-group-hint">
        Work in progress the story does not reach yet — scratch scenes, cut material,
        notes. A draft is left out of the build and does not raise “not part of the
        story” warnings. A file becomes a draft when it matches one of these patterns{" "}
        <em>and</em> nothing includes it; a file the story still reaches is never a
        draft, so these patterns can never break a divert.
      </p>

      <GlobList
        globs={globs}
        report={report}
        onAdd={(glob) => {
          const next = withDraftGlob(source, glob);
          if (next !== null) write(next);
        }}
        onRemove={(glob) => {
          const next = withoutDraftGlob(source, glob);
          if (next !== null) write(next);
        }}
      />
    </SettingsGroup>

    <SettingsGroup title="Discovery">
      <p className="settings-group-hint">
        Finding the project&rsquo;s files never descends into{" "}
        <code>target</code>, <code>.git</code> or <code>node_modules</code> — they are
        large, and a stray source file inside one is almost never part of the story.
        Turn one on if this project genuinely keeps <code>.brink</code> or{" "}
        <code>.ink</code> files there.
      </p>
      {PRUNABLE_DIRS.map((dir) => (
        <SettingsRow key={dir} title={dir} description={DISCOVERY_HINTS[dir]}>
          <SettingsToggle
            checked={isUnpruned(source, dir)}
            label={`Search ${dir}`}
            onChange={(next) => {
              const updated = next
                ? withUnprunedDir(source, dir)
                : withoutUnprunedDir(source, dir);
              if (updated !== null) write(updated);
            }}
          />
        </SettingsRow>
      ))}
    </SettingsGroup>
    </>
  );
}

/**
 * Why a project might want each one walked.
 *
 * Keyed by the shipping constant rather than restated as its own list, so a
 * fourth pruned directory cannot arrive with no explanation beside it.
 */
const DISCOVERY_HINTS: Record<(typeof PRUNABLE_DIRS)[number], string> = {
  target: "Rust build output. Worth searching only if a build step generates story files into it.",
  ".git": "Version-control internals. Almost never holds story files.",
  "node_modules":
    "Installed packages. Worth searching if the project consumes story files from a dependency.",
};

/** What one glob currently matches, or null when the report has not seen it. */
function matchesFor(report: DraftGlobReport | null, glob: string) {
  return report?.globs.find((g) => g.glob === glob) ?? null;
}

function GlobList({
  globs,
  report,
  onAdd,
  onRemove,
}: {
  globs: string[];
  report: DraftGlobReport | null;
  onAdd: (glob: string) => void;
  onRemove: (glob: string) => void;
}) {
  const [draft, setDraft] = useState("");
  const problem = draft.trim() === "" ? null : draftGlobProblem(draft);

  const submit = (): void => {
    const trimmed = draft.trim();
    if (trimmed === "" || draftGlobProblem(trimmed) !== null) return;
    onAdd(trimmed);
    setDraft("");
  };

  return (
    <div className="draft-globs">
      <div className="draft-globs-add">
        <input
          className="draft-globs-input"
          type="text"
          value={draft}
          placeholder="scratch/**"
          aria-label="Add a draft pattern"
          aria-invalid={problem !== null}
          onChange={(event) => setDraft(event.target.value)}
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
          disabled={draft.trim() === "" || problem !== null}
        >
          Add
        </button>
      </div>
      {problem !== null && <p className="draft-globs-problem">{problem}</p>}

      {globs.length === 0 ? (
        <p className="draft-globs-empty">
          No draft patterns. Add one — <code>scratch/**</code> marks everything in a
          folder, <code>notes/*.ink</code> just that folder&rsquo;s files, and{" "}
          <code>**/cut-*.ink</code> a naming convention anywhere in the project.
        </p>
      ) : (
        <ul className="draft-globs-list">
          {globs.map((glob) => (
            <GlobRow
              key={glob}
              glob={glob}
              matches={matchesFor(report, glob)}
              compiled={report?.compiled ?? false}
              onRemove={() => onRemove(glob)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function GlobRow({
  glob,
  matches,
  compiled,
  onRemove,
}: {
  glob: string;
  matches: ReturnType<typeof matchesFor>;
  compiled: boolean;
  onRemove: () => void;
}) {
  const drafts = matches?.drafts ?? [];
  const inStory = matches?.inStory ?? [];
  // A pattern this session has never compiled with reads as unknown, not as
  // "matches nothing" — those are the same empty lists and opposite facts.
  const known = compiled && matches !== null;

  return (
    <li className="draft-globs-row">
      <div className="draft-globs-head">
        <code className="draft-globs-pattern">{glob}</code>
        <span
          className={`draft-globs-count${
            known && drafts.length === 0 && inStory.length === 0 ? " is-empty" : ""
          }`}
        >
          {!known
            ? "not checked yet"
            : drafts.length === 0 && inStory.length === 0
              ? "matches nothing"
              : `${drafts.length} draft${drafts.length === 1 ? "" : "s"}`}
        </span>
        <button
          type="button"
          className="draft-globs-remove"
          aria-label={`Remove the pattern ${glob}`}
          title={`Remove ${glob}`}
          onClick={onRemove}
        >
          ×
        </button>
      </div>

      {drafts.length > 0 && (
        <details className="draft-globs-files">
          <summary>
            {drafts.length === 1 ? "1 file" : `${drafts.length} files`}
          </summary>
          <ul>
            {drafts.map((path) => (
              <li key={path}>
                <code>{path}</code>
              </li>
            ))}
          </ul>
        </details>
      )}

      {inStory.length > 0 && (
        <p className="draft-globs-in-story">
          Also matches {inStory.length === 1 ? "a file" : `${inStory.length} files`} the
          story reaches, so {inStory.length === 1 ? "it is" : "they are"} not{" "}
          {inStory.length === 1 ? "a draft" : "drafts"}:{" "}
          {inStory.map((path, i) => (
            <span key={path}>
              {i > 0 && ", "}
              <code>{path}</code>
            </span>
          ))}
        </p>
      )}
    </li>
  );
}
