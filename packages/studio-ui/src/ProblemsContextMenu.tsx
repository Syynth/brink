/**
 * Right-click a Problems row to silence that diagnostic (#3148).
 *
 * Three scopes, narrowest first, because that is the order an author should
 * reach for them — and each is a directive the compiler already has:
 *
 * - **this line** — `// brink-disable Exxx` above it
 * - **this file** — `// brink-disable-file <code>` at the top, or
 *   `// brink-disable-file-all` for the blanket gesture
 * - **this project** — `[lints] Exxx = "allow"` in `brink.toml`
 *
 * Plus "Configure…", which opens Settings rather than deciding for you —
 * suppressing is one of five levels, and the other four (`deny`, `warn`,
 * `info`, `hint`) belong on a surface that can show them.
 *
 * A code the compiler will not let you suppress gets no suppression items
 * at all. `Error`-tier diagnostics are refused by every channel, and a menu
 * that offers a gesture the compiler discards is worse than one that
 * doesn't offer it.
 */

import { useMemo, useRef } from "react";
import type { Diagnostic } from "@brink/wasm-types";
import { setTomlString } from "@brink/studio-store";
import { useContextMenuDismiss } from "./BinderContextMenu.js";
import { isConfigPath } from "./ConfigFormPanel.js";
import { isProseDiagnostic } from "@brink/studio-store";
import {
  isSuppressible,
  suppressAllInFile,
  suppressInFile,
  suppressOnLine,
} from "./suppressDiagnostic.js";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { SETTINGS_SECTION_IDS } from "./settingsSectionIds.js";

export interface ProblemsMenuTarget {
  x: number;
  y: number;
  diagnostic: Diagnostic;
}

export function ProblemsContextMenu({
  target,
  onClose,
}: {
  target: ProblemsMenuTarget;
  onClose: () => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  useContextMenuDismiss(menuRef, onClose);

  const storeApi = useStudioStoreApi();
  const outline = useStudioStore((s) => s.outline);
  const setSettingsSection = useStudioStore((s) => s.setSettingsSection);

  const code = target.diagnostic.code;
  const path = target.diagnostic.file;

  const configPath = useMemo(
    () => outline.find((f) => !f.mounted && isConfigPath(f.path))?.path ?? null,
    [outline],
  );

  /** The code's DEFAULT severity, from the compiler's registry (#3169). */
  const defaultSeverity = useMemo(() => {
    const project = storeApi.getState()._project as
      | { getDiagnosticRegistry?: () => { code: string; default_severity: string }[] }
      | null;
    if (code === undefined || typeof project?.getDiagnosticRegistry !== "function") {
      return undefined;
    }
    return project.getDiagnosticRegistry().find((r) => r.code === code)?.default_severity;
  }, [storeApi, code, outline]);

  const edit = (file: string, next: (source: string) => string): void => {
    const project = storeApi.getState()._project;
    if (project === null) return;
    const current = project.getSession().getFileSource(file);
    if (current === null) return;
    const updated = next(current);
    if (updated === current) return;
    project.applyEdit(file, updated);
    const docs = storeApi.getState()._documents;
    docs?.refreshExternal(file);
    docs?.triggerCompile();
  };

  const items: { label: string; run: () => void; disabled?: boolean }[] = [];

  if (code !== undefined && path !== undefined && isSuppressible(defaultSeverity)) {
    items.push({
      label: `Suppress ${code} on this line`,
      run: () => edit(path, (src) => suppressOnLine(src, target.diagnostic.start, code)),
    });
    items.push({
      label: `Suppress ${code} in this file`,
      run: () => edit(path, (src) => suppressInFile(src, code)),
    });
    items.push({
      // Offered separately, and worded for what it does. One item that
      // claimed to suppress a code while silencing the file (#3259) is the
      // reason this is two gestures rather than one.
      label: "Suppress all diagnostics in this file",
      run: () => edit(path, suppressAllInFile),
    });
    items.push({
      label: `Suppress ${code} in this project`,
      // Disabled rather than hidden when there is no `brink.toml`: the
      // gesture exists, the project just has nowhere to record it, and
      // hiding it would read as "this code cannot be suppressed".
      disabled: configPath === null,
      run: () => {
        if (configPath === null) return;
        edit(configPath, (src) => setTomlString(src, "lints", code, "allow"));
      },
    });
  }

  // A prose finding has no compiler code and nothing in `[lints]` to
  // configure — "Configure prose:Spelling…" would open the Diagnostics
  // section and offer nothing about it. Its settings live under Prose.
  if (isProseDiagnostic(target.diagnostic)) {
    items.push({
      label: "Prose settings…",
      run: () => setSettingsSection(SETTINGS_SECTION_IDS.prose),
    });
  } else {
    items.push({
      label: code === undefined ? "Open diagnostics settings" : `Configure ${code}…`,
      // Names the DIAGNOSTICS section rather than opening `brink.toml`: the
      // config file's own door lands on Project, which is the wrong place to
      // arrive from "configure this diagnostic". Every door into Settings
      // names the section it means.
      run: () => setSettingsSection(SETTINGS_SECTION_IDS.diagnostics),
    });
  }

  return (
    <div
      ref={menuRef}
      className="brink-context-menu"
      style={{ left: target.x, top: target.y }}
      role="menu"
    >
      {items.map((item) => (
        <div
          key={item.label}
          className={"brink-context-menu-item" + (item.disabled === true ? " disabled" : "")}
          role="menuitem"
          aria-disabled={item.disabled === true}
          onClick={() => {
            if (item.disabled === true) return;
            item.run();
            onClose();
          }}
        >
          {item.label}
        </div>
      ))}
    </div>
  );
}
