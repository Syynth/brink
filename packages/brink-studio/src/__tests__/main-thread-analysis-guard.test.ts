/**
 * Main-thread analysis boundary guard (docs/editor-worker-spec.md §4.2,
 * landed with W5c — re-sequenced from W3, where mid-migration its
 * allowlist would have been the whole tree).
 *
 * ## The invariant
 *
 * Since W5, the RECURRING editor paths (keystroke, deferred refresh,
 * diagnostics, panels) must never pull project analysis on the main
 * thread: analysis-flavored queries ride the `SessionClient` to the
 * worker replica, and synchronous rebuilds read worker-fed stashes.
 * The synchronous session survives on the main thread as (a) the
 * CONTENT store and mutation source that feeds the replica, and (b) the
 * complete in-process fallback road for environments without workers —
 * so analysis-flavored METHODS still exist on it, and this guard pins
 * WHERE they may be called.
 *
 * ## The mechanism
 *
 * A lexical scan (same family as `paint-path-call-enrolment`) over every
 * workspace `src/` file for `.<analysisMethod>(` call shapes. Every hit
 * must be in the allowlist below — each entry a deliberate, documented
 * survivor (fallback roads behind worker-fed stashes, or one-shot
 * command paths whose migration is tracked). A new call site anywhere
 * else fails this test and must either route through the client or be
 * enrolled here with a reason.
 *
 * Client-road usage is invisible to this scan by construction: queries
 * name methods as STRING ARGUMENTS (`query("getHirSpansDoc", …)`), never
 * as `.getHirSpansDoc(` member calls.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { describe, it, expect } from "vitest";
import { deriveScanRoots, parseWorkspacePackageGlobs } from "./workspace-roots.js";

/** Session methods that pull project ANALYSIS (symbol index, resolution,
 *  diagnostics, compile) — the set the main thread's recurring paths must
 *  not call. Content-level reads (`getFileSource`, `listFiles`,
 *  `getViewSourceDoc`, line contexts, segment manifests, classifier
 *  tokens) are deliberately NOT here: they are input/lowering-grade. */
const ANALYSIS_METHODS = [
  "compileProject",
  "getProjectOutline",
  "getStoryGraph",
  "getCompilationClosure",
  "getHirSpansDoc",
  "getSemanticTokensDoc",
  "getSegmentSemanticTokensDoc",
  "getCompletionsDoc",
  "getHoverDoc",
  "getSignatureHelpDoc",
  "getCodeActionsDoc",
  "getInlayHintsDoc",
  "getArgumentWidgetsDoc",
  "getColorHintsDoc",
  "getFoldingRangesDoc",
  "getFileSymbols",
  "getDocumentSymbols",
  "gotoDefinitionDoc",
] as const;

/**
 * The surviving call sites: `repo-relative file` → the methods it may
 * call, with the reason. Everything else is a violation.
 */
const ALLOWLIST: Record<string, { methods: string[]; reason: string }> = {
  "packages/ink-editor/src/document-handle.ts": {
    methods: [...ANALYSIS_METHODS],
    reason:
      "THE choke point: worker-fed stashes are read first (W5c); these are " +
      "the in-process fallback fetches (mocks, small documents, no-worker " +
      "environments) and the one-shot thin wrappers (goto/rename family) " +
      "whose worker migration is tracked as a follow-up issue.",
  },
  "packages/ink-editor/src/project-session.ts": {
    methods: ["compileProject"],
    reason:
      "The synchronous compile road's definition — generation-cached, no " +
      "production caller since W4 (triggerCompile/diagnostics/panels all " +
      "ride the async facade); kept as embedder API and fallback.",
  },
  "packages/studio-store/src/slices/search.ts": {
    methods: ["getSemanticTokensDoc"],
    reason:
      "Search-card highlighting: user-triggered, bounded by the rendered " +
      "cards, and served against a delta-fed session (incremental " +
      "re-analysis only). Worker migration tracked with the one-shot " +
      "family.",
  },
  "packages/ink-editor/src/document-sessions.ts": {
    methods: ["getFileSymbols"],
    reason:
      "resolveSymbolRange at symbol-tab mount: a one-shot with a built-in " +
      "degrade path (hint, then full-file). Post-W5b this costs " +
      "INCREMENTAL analysis only (the replica does not warm the main " +
      "session, but delta-fed inputs keep re-analysis bounded). Worker " +
      "migration tracked with the one-shot family.",
  },
};

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const workspaceYamlPath = join(repoRoot, "pnpm-workspace.yaml");
const SKIP_DIRS = new Set(["__tests__", "__mocks__", "__fixtures__", "dist", "node_modules"]);

const CALL_RE = new RegExp(`\\.(${ANALYSIS_METHODS.join("|")})\\(`);

function listSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    if (SKIP_DIRS.has(entry)) continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...listSourceFiles(full));
    else if (/\.tsx?$/.test(entry) && !/\.(test|spec)\.tsx?$/.test(entry)) out.push(full);
  }
  return out;
}

function violationsIn(file: string): string[] {
  const rel = file.slice(repoRoot.length + 1);
  const allowed = new Set(ALLOWLIST[rel]?.methods ?? []);
  const out: string[] = [];
  const lines = readFileSync(file, "utf8").split("\n");
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (line === undefined || line.trim().startsWith("//") || line.trim().startsWith("*")) {
      continue;
    }
    const hit = CALL_RE.exec(line);
    if (hit === null) continue;
    const method = hit[1];
    if (method !== undefined && !allowed.has(method)) {
      out.push(`${rel}:${i + 1} calls .${method}( on the main thread`);
    }
  }
  return out;
}

describe("main-thread analysis boundary (worker architecture W5)", () => {
  it("no analysis-flavored session call outside the allowlist", () => {
    const violations: string[] = [];
    for (const pkgDir of deriveScanRoots(
      parseWorkspacePackageGlobs(readFileSync(workspaceYamlPath, "utf8")),
      repoRoot,
    )) {
      const src = join(pkgDir, "src");
      try {
        if (!statSync(src).isDirectory()) continue;
      } catch {
        continue;
      }
      for (const file of listSourceFiles(src)) violations.push(...violationsIn(file));
    }
    expect(
      violations,
      "Analysis must ride the SessionClient to the worker (or read a " +
        "worker-fed stash). Route the call through docClient()/projectQuery, " +
        "or enroll the site in this guard's ALLOWLIST with a reason.",
    ).toEqual([]);
  });

  it("every allowlisted file still has at least one enrolled call (no rot)", () => {
    for (const [rel, entry] of Object.entries(ALLOWLIST)) {
      const text = readFileSync(join(repoRoot, rel), "utf8");
      const found = entry.methods.some((m) => text.includes(`.${m}(`));
      expect(found, `${rel} no longer calls any enrolled method — prune the entry`).toBe(true);
    }
  });
});
