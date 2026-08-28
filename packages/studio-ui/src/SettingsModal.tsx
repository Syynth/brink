/**
 * Settings as a modal with a section rail (#3174, design round 2026-08-27).
 *
 * Ruled 2026-08-27: Settings is consult-and-adjust, not a place you work,
 * so taking over the editor area cost you the file you were looking at for
 * something you leave in seconds. The takeover was right while Settings was
 * small; the `brink.toml` interface made it a substantial surface with its
 * own navigation, which is what a modal is for.
 *
 * The layout follows Zed's: a searchable rail on the left, ONE section at a
 * time on the right. That is the part that scales — the previous single
 * scrolling page put the project's lint table and the theme picker in the
 * same column, and the only way to find anything was to scroll past
 * everything else.
 *
 * **Sections are registered, not laid out.** A section is an id, a title,
 * an icon and a body; this file knows how to draw a rail and a pane, and
 * nothing else. Adding a settings surface means adding an entry to
 * `SECTIONS`, never editing the shell — which is what keeps the page from
 * drifting behind what is actually configurable.
 */

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { registerDismissible } from "@brink/studio-shell";
import { useStudioStore } from "./StoreContext.js";

/**
 * Which settings a section belongs to — and, crucially, WHERE THEY LIVE.
 *
 * `project` writes `brink.toml`: versioned, shared with everyone who opens
 * the project. `app` writes this machine's storage: yours alone, and it
 * follows you between projects.
 *
 * That is a real distinction an author needs to be able to see, which is
 * why it is a switch rather than a mixed list. Before it existed,
 * Diagnostics held both — the `[lints]` table (project) and the
 * external-function check (a studio preference) — with only a hint to say
 * so.
 */
export type SettingsScope = "app" | "project";

export interface SettingsSection {
  id: string;
  scope: SettingsScope;
  title: string;
  /** Matched by search alongside the title — what the section is *about*. */
  keywords: string;
  icon: ReactNode;
  body: ReactNode;
}

const icon = (d: string): ReactNode => (
  <svg
    width="13"
    height="13"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden
  >
    <path d={d} />
  </svg>
);

export const SETTINGS_ICONS = {
  project: icon("M12 3c1 2.5 3.5 5.5 5.5 8a7 7 0 1 1-11 0C8.5 8.5 11 5.5 12 3z"),
  diagnostics: icon("M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"),
  formatting: icon("M4 7h16M8 12h12M8 17h9"),
  prose: icon("M4 6h16M4 11h13M4 16h9M17.5 15.5l2 2M19.5 13.5l2 2"),
  editor: icon("M4 7h16M4 12h10M4 17h13"),
  appearance: icon("M12 3a9 9 0 1 0 0 18 2 2 0 0 0 0-4 2 2 0 0 1 2-2h3a4 4 0 0 0 4-4 8 8 0 0 0-9-8z"),
  keymap: icon("M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10"),
} as const;

const SCOPE_LABEL: Record<SettingsScope, string> = {
  app: "App",
  project: "Project",
};

const SCOPE_HINT: Record<SettingsScope, string> = {
  app: "Yours, on this machine — they follow you between projects.",
  project: "Written to brink.toml — shared with everyone who opens this project.",
};

export function SettingsModal({ sections }: { sections: SettingsSection[] }) {
  const open = useStudioStore((s) => s.settingsSection);
  const setSection = useStudioStore((s) => s.setSettingsSection);
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<SettingsScope>("project");
  const dialogRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const isOpen = open !== null;

  // Escape and outside-pointerdown, through the same registry every other
  // transient surface uses — so Settings closes on the same gesture as the
  // palette and the context menus rather than inventing its own.
  useEffect(() => {
    if (!isOpen) return;
    return registerDismissible(() => setSection(null));
  }, [isOpen, setSection]);

  useEffect(() => {
    if (isOpen) searchRef.current?.focus();
  }, [isOpen]);

  // The rail filters; it does not jump you somewhere. A search that moved
  // the selection would lose the section you were reading the moment you
  // typed.
  // Search reaches across BOTH scopes, deliberately: an author looking for
  // "theme" should not have to know it is an app setting first. Matches
  // from the other scope carry their scope as a label.
  const needle = query.trim().toLowerCase();
  const matches = useMemo(
    () =>
      needle === ""
        ? sections
        : sections.filter(
            (s) =>
              s.title.toLowerCase().includes(needle) ||
              s.keywords.toLowerCase().includes(needle),
          ),
    [sections, needle],
  );

  if (!isOpen) return null;

  // An id that no longer exists (a section removed while open, or a caller
  // naming one that isn't registered) falls back rather than rendering an
  // empty pane.
  const active = sections.find((s) => s.id === open) ?? sections[0];
  if (active === undefined) return null;

  // The SECTION decides the scope, not the switch: every door names a
  // section, and one that named a project section while the switch sat on
  // "App" would show a rail the active section is not in.
  const shownScope = active.scope;

  return (
    <div
      className="brink-settings-backdrop"
      // The backdrop is not the dismiss surface — `registerDismissible`
      // already handles outside-pointerdown, and a click handler here would
      // also fire for a drag that STARTED inside the dialog and ended out.
      role="presentation"
    >
      <div
        ref={dialogRef}
        className="brink-settings-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
      >
        <div className="brink-settings-rail">
          {/* Scope first: it says WHERE a setting is written, which changes
              what changing one means. Project settings are versioned and
              shared; app settings are yours on this machine. */}
          <div className="brink-settings-scopes" role="tablist" aria-label="Settings scope">
            {(["project", "app"] as const).map((sc) => (
              <button
                key={sc}
                type="button"
                role="tab"
                aria-selected={sc === shownScope}
                className={"brink-settings-scope" + (sc === shownScope ? " active" : "")}
                title={SCOPE_HINT[sc]}
                onClick={() => {
                  setScope(sc);
                  // Land on that scope's first section — switching scope is
                  // a navigation, and leaving the old section showing would
                  // make the switch look broken.
                  const first = sections.find((s) => s.scope === sc);
                  if (first !== undefined) setSection(first.id);
                }}
              >
                {SCOPE_LABEL[sc]}
              </button>
            ))}
          </div>
          <input
            ref={searchRef}
            type="search"
            className="brink-settings-search"
            placeholder="Search settings"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <nav className="brink-settings-nav">
            {matches
              // Unsearched, the rail is the current scope. Searching reaches
              // across BOTH — an author looking for "theme" should not have
              // to know it is an app setting first — and a result from the
              // other scope says so.
              .filter((s) => needle !== "" || s.scope === shownScope)
              .map((s) => (
                <button
                  key={s.id}
                  type="button"
                  className={"brink-settings-nav-item" + (s.id === active.id ? " active" : "")}
                  aria-current={s.id === active.id}
                  onClick={() => setSection(s.id)}
                >
                  {s.icon}
                  <span>{s.title}</span>
                  {needle !== "" && s.scope !== shownScope && (
                    <span className="brink-settings-nav-scope">{SCOPE_LABEL[s.scope]}</span>
                  )}
                </button>
              ))}
            {matches.length === 0 && (
              <p className="brink-settings-nav-empty">No section matches “{query}”.</p>
            )}
          </nav>
        </div>

        <div className="brink-settings-pane">
          <header className="brink-settings-head">
            <h2>{active.title}</h2>
            <button
              type="button"
              className="brink-settings-close"
              aria-label="Close settings"
              onClick={() => setSection(null)}
            >
              ×
            </button>
          </header>
          <div className="brink-settings-body">{active.body}</div>
        </div>
      </div>
    </div>
  );
}
