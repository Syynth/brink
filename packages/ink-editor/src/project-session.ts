/**
 * ProjectSession — bridges a FileProvider with an EditorSession.
 *
 * Handles multi-file loading, INCLUDE resolution, file creation, provider
 * write-back, and project compilation (cached by the session's mutation
 * generation, so several live views can each ask for "the current compile"
 * without recompiling an unchanged project).
 *
 * It also owns the {@link FileChangeHub} — the single seam every content
 * mutation reports through (issues #154/#137). The CM6 edit flush calls
 * `notifyFileChanged`; bulk edit paths (binder structural ops, search
 * replace) call `applyEdit`; `addFile` records creations. All of them feed
 * both the provider write-back and the host egress callback.
 *
 * Per-view document state (wasm document handles, CM6 states, mirroring)
 * lives in DocumentSessions — this class owns only project-level concerns.
 */

import type { FileProvider } from "./provider.js";
import { EditorSessionHandle, getDiagnosticRegistry } from "@brink-lang/web";
import type { CompileResult, RenameDiagnostic } from "@brink/wasm-types";
import { FileChangeHub, type FileChange, type FileConflict } from "./file-change-hub.js";
import { scheduleIdleWork, cancelIdleWork, type IdleHandle } from "./idle-schedule.js";
import { withPerfTiming } from "./perf/wasm-proxy.js";
import { perfSpan } from "./perf/probe.js";
import { LocalTransport } from "./worker/local-transport.js";
import { SessionClient } from "./worker/session-client.js";
import { WorkerTransport, createSessionWorker } from "./worker/worker-transport.js";

// The config filename `discoverProjectConfig`'s walk-up looks for (mirrors
// `brink_project_config::CONFIG_FILE_NAME` — see crates/internal/brink-project-config/src/lib.rs).
const PROJECT_CONFIG_FILENAME = "brink.toml";

/** True when `path`'s basename is the project-config filename, wherever in
 *  the tree it sits — the trigger for re-running `discoverProjectConfig`. */
function isProjectConfigPath(path: string): boolean {
  const slash = path.lastIndexOf("/");
  const base = slash >= 0 ? path.slice(slash + 1) : path;
  return base === PROJECT_CONFIG_FILENAME;
}

/**
 * The result of {@link ProjectSession.renameFile} (issue #2918): the moved
 * file's INCLUDE-referrer paths, plus the breakage-gate verdict the wasm
 * `rename_file` op already computes (`StructuralResult.safe` /
 * `.introduced_diagnostics`, #316) — surfaced here rather than discarded, so
 * a caller can report a move that broke a reference instead of applying it
 * silently. `safe`/`introducedDiagnostics` describe the edits that were
 * ACTUALLY APPLIED (this method throws before applying anything on a
 * refused op — see the method's own doc), not a preflight the caller could
 * still cancel.
 */
export interface RenameFileResult {
  /** Paths whose `INCLUDE`s were rewritten — same as the pre-#2918 return
   *  value, kept for callers that only care about refreshing views. */
  referrers: string[];
  /** True when the move introduced no new diagnostics. */
  safe: boolean;
  /** Diagnostics the move introduced (breaking a reference). Empty ⇒ `safe`. */
  introducedDiagnostics: RenameDiagnostic[];
}

/**
 * The result of {@link ProjectSession.renameDir} (issue #2918) — the
 * directory analog of {@link RenameFileResult}: every moved `{oldPath,
 * newPath}` pair, the outside referrers whose `INCLUDE`s were rewritten, and
 * the same breakage-gate verdict `DirMoveResult.safe` /
 * `.introduced_diagnostics` already carries.
 */
export interface RenameDirResult {
  moved: Array<{ oldPath: string; newPath: string }>;
  referrers: string[];
  safe: boolean;
  introducedDiagnostics: RenameDiagnostic[];
}

export interface ProjectSessionOptions {
  provider: FileProvider;
  /**
   * The project's entry file, used to seed `brink.toml` discovery (its
   * walk-up starts at this path's directory) and as the compile/initial-tab
   * entry UNTIL/UNLESS discovery finds a `brink.toml` naming a valid
   * `[project] entry` (issue #2331, ruled 2026-08-07 "`[project] entry`
   * beats `mountStudio`'s `entryFile`") — see {@link ProjectSession.getEntryFile}.
   * This argument is the fallback for a configless project; it is never
   * consulted again once a config-named entry supersedes it.
   */
  entryFile: string;
  /**
   * Whether `entryFile` is a HUMAN'S EXPLICIT CHOICE rather than a host
   * default (the file-anchored project open model, ruled 2026-08-23 —
   * `docs/decision-log.md` "A project is anchored on a FILE"). When true, a
   * discovered `brink.toml`'s `[project] entry` never supersedes
   * `entryFile`: the #2331 precedence ruling ("`[project] entry` beats
   * `mountStudio`'s `entryFile`") stands for its own case — a
   * *host-supplied default* still loses to authored config — but a person
   * opening a specific story file IS choosing the entry, and that choice
   * wins. Discovery itself still runs (lints, conventions, warnings all
   * apply); only the entry supersession is disabled. Default `false`, the
   * pre-2026-08-23 behavior.
   */
  entryIsExplicit?: boolean;
  /** Re-use an existing session, or a new one is created. */
  session?: EditorSessionHandle;
  /**
   * Run the project-level query road (compile, outline, story graph,
   * closure — everything doc-independent) in a Web Worker (W4 of
   * `docs/editor-worker-spec.md`). The worker owns its own wasm session,
   * kept current by an ordered file/config mutation stream flushed before
   * every worker query; doc-scoped queries stay on the in-process road
   * until the W5 flip. Fully feature-detected: environments without
   * `Worker` (or where the worker fails to boot) silently keep the
   * in-process road. Default false.
   */
  workerSession?: boolean;
  /** Override the session-worker factory (tests; hosts whose bundler
   *  cannot process the `new URL` worker pattern supply their own).
   *  Returning null disables the worker road. */
  workerFactory?: () => import("./worker/worker-transport.js").WorkerLike | null;
  /** Called when an external file change is detected. */
  onExternalFileChange?: (path: string, content: string | null) => void;
  /**
   * Called when an external change collides with an unsaved studio buffer
   * (issue #320). The studio keeps the dirty buffer (the SAFE DEFAULT) and
   * flags the path conflicted; this hook lets a merge/diff surface (Track V)
   * reconcile the host's on-disk content with the kept buffer.
   */
  onFileConflict?: (conflict: FileConflict) => void;
  /**
   * Host egress callback (issue #154): receives debounced, batched change
   * notifications for every session-content mutation. See FileChangeHub.
   */
  onFilesChanged?: (changes: FileChange[]) => void;
  /** Trailing debounce for `onFilesChanged` batches (default 500 ms). */
  changeDebounceMs?: number;
  /**
   * Whether `onFilesChanged` delivery counts as persistence (default
   * `true`, the write-through contract). Overlay hosts — whose egress
   * handler feeds a **backup ring** rather than canonical storage (the
   * celeris file model; brink-desktop D2) — set `false`: batches still
   * deliver, but dirty means "diverges from the last canonical save" and
   * only `markFilesSaved`/`markAllSaved` clears it. See
   * {@link FileChangeHubOptions.deliveryPersists}.
   */
  egressPersists?: boolean;
  /**
   * Unrecognized-key/lint-code warnings from the most recent `brink.toml`
   * discovery/apply (issue #2324) — forwarded verbatim from
   * `EditorSessionHandle.discoverProjectConfig`'s return value. Fires once
   * after `initialize()` loads the project's files (even with an empty
   * array — a host that wants to clear a previous warning list can rely on
   * that), and again every time a `brink.toml` in the session is created,
   * edited, renamed into/out of, or externally rewritten. Never fires for a
   * discovery error (malformed TOML / an invalid recognized-key value) — see
   * {@link onProjectConfigError} instead.
   */
  onProjectConfigWarnings?: (warnings: string[]) => void;
  /**
   * A `brink.toml` discovery/apply error (issue #2324): `discoverProjectConfig`
   * throws on malformed TOML or a recognized key with an invalid value (e.g.
   * `dialect = "brnik"`). Without this callback such an error would otherwise
   * propagate out of whichever call triggered discovery — `initialize()`,
   * `notifyFileChanged`/`applyEdit`, `addFile`, `deleteFile`, `renameFile`, or
   * the external-change handler — and a mid-edit typo in the one file this
   * feature exists to make effective would take the whole session down with
   * it. `applyProjectConfig` catches the throw at its single call site and
   * reports it here instead; the file's *previous* successfully-applied
   * config (if any) stays in effect until a valid edit re-discovers it.
   */
  onProjectConfigError?: (message: string) => void;
}

/** Config-surface methods mirrored to the worker session (W4): every
 *  mutation that changes analysis/compile-relevant state without going
 *  through file content. Ordered and replayed via the config log. */
const WORKER_CONFIG_METHODS = new Set([
  "setDialect",
  "clearDialect",
  "setHostManifest",
  "clearHostManifest",
  "setHostValues",
  "clearHostValues",
  "setExternalCheck",
  "setSemanticTypeCheck",
  "setLanguageDialect",
  "setTypePolicy",
  "setLintOverrides",
  "setDenyWarningsOverride",
  "clearDenyWarningsOverride",
  "setFoldRunsEnabled",
  // #3229: MUST be mirrored. `compileProjectAsync` — the road the studio
  // actually compiles on (`document-sessions.ts`) — routes through
  // `projectQuery`, which runs on the WORKER replica whenever one is live.
  // A debug toggle set only on the main session would leave the worker
  // compiling without the `DebugInfo` section, so the flag would appear to
  // work and change nothing in the real studio: the exact failure shape
  // #3229 exists to fix, one layer over.
  "setDebugInfoEnabled",
  "applyProjectConfig",
  "discoverProjectConfig",
]);

/** File-content methods mirrored to the worker session (W4). */
const WORKER_FILE_METHODS = new Set(["updateFile", "removeFile"]);

/** Session mutations forwarded verbatim to the worker replica (W5b):
 *  doc lifecycle (whose returned ids must match the main session's —
 *  checked via the host's mutationResult events; see
 *  `expectWorkerDocId`), view-context choreography, and the legacy
 *  active-file push. Ordered with everything else through the client's
 *  mutation stream. */
const WORKER_REPLAY_METHODS = new Set([
  "openDocument",
  "openFragment",
  "closeDocument",
  "setActiveFile",
  "setViewContext",
  "clearViewContext",
  "updateSource",
]);

/** Per-document edits forwarded as PROTOCOL edit/push messages (W5b) —
 *  versioned and acked, unlike the verbatim replays above. */
const WORKER_DOC_EDIT_METHODS = new Set(["updateDocument", "applyEditsDocument"]);

/**
 * Observe the session at its single choke point (the same reasoning as
 * `withPerfTiming`): every config or file mutation — from this class,
 * `DocumentSessions`, or the studio reaching through `getSession()` — is
 * recorded so the worker session (W4) can be brought current before a
 * worker query. Per-document pushes are covered separately by
 * `notifyFileChanged` (their content reaches the worker as whole-file
 * updates at flush time).
 */
function withWorkerMirror(
  session: EditorSessionHandle,
  hooks: {
    config(method: string, args: unknown[]): void;
    file(method: string, args: unknown[]): void;
    replay(method: string, args: unknown[], returned: unknown): void;
    docEdit(method: string, args: unknown[]): void;
  },
): EditorSessionHandle {
  return new Proxy(session, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver);
      if (typeof value !== "function" || typeof prop !== "string") return value;
      if (
        !WORKER_CONFIG_METHODS.has(prop) &&
        !WORKER_FILE_METHODS.has(prop) &&
        !WORKER_REPLAY_METHODS.has(prop) &&
        !WORKER_DOC_EDIT_METHODS.has(prop)
      ) {
        return value;
      }
      return (...args: unknown[]) => {
        const out = (value as (...a: unknown[]) => unknown).apply(target, args);
        if (WORKER_CONFIG_METHODS.has(prop)) hooks.config(prop, args);
        else if (WORKER_FILE_METHODS.has(prop)) hooks.file(prop, args);
        else if (WORKER_DOC_EDIT_METHODS.has(prop)) hooks.docEdit(prop, args);
        else hooks.replay(prop, args, out);
        return out;
      };
    },
  }) as EditorSessionHandle;
}

export class ProjectSession {
  private provider: FileProvider;
  /** @see getProseDictionary — `null` means "not computed since analysis moved". */
  private proseDictionaryCache: string[] | null = null;
  /**
   * The host's constructor-time `entryFile` argument — the fallback for a
   * configless project, and the seed for `discoverProjectConfig`'s walk-up.
   * Set once in the constructor and never mutated afterward (issue #2331
   * review finding): `applyProjectConfig` used to overwrite the equivalent
   * field with a config-named entry, which both defeated the "config wins
   * only while it says so" contract (deleting `entry` from `brink.toml` had
   * no way back to the host's own choice) and shifted the discovery seed
   * itself out from under the host on every subsequent re-discovery.
   */
  private readonly hostEntryFile: string;
  /**
   * Whether {@link hostEntryFile} was a human's explicit open rather than a
   * host default — see {@link ProjectSessionOptions.entryIsExplicit}. When
   * set, {@link getEntryFile} never lets {@link configuredEntry} supersede.
   */
  private readonly entryIsExplicit: boolean;
  /**
   * The most recently discovered `[project] entry`, when it resolves to a
   * real file in this session — `null` when no `brink.toml` was found, it
   * doesn't set `entry`, or the named entry doesn't resolve. Wholesale
   * replaced on every `applyProjectConfig` call (never merged with the
   * previous value), so removing `entry` from `brink.toml` genuinely clears
   * it and {@link getEntryFile} falls back to {@link hostEntryFile} again.
   */
  private configuredEntry: string | null = null;
  private session: EditorSessionHandle;
  private readonly changes: FileChangeHub;
  private onExternalFileChange?: (path: string, content: string | null) => void;
  private onFileConflict?: (conflict: FileConflict) => void;
  private onProjectConfigWarnings?: (warnings: string[]) => void;
  private onProjectConfigError?: (message: string) => void;
  private unsubscribeExternal?: () => void;
  private destroyed = false;
  private lastCompile: { generation: number; result: CompileResult } | null = null;
  /**
   * The async session facade (docs/editor-worker-spec.md, W2a) — today a
   * `LocalTransport` over the same in-process session, so async consumers
   * get the worker transport's exact semantics with zero behavior change;
   * W4 swaps the transport. Owned here so every consumer of this project
   * shares one client (one doc-version space, one scheduler).
   */
  private readonly client: SessionClient;
  /**
   * The one in-flight project compile (spec §6: compile coalesces to one
   * in flight). Client-side dedup — two views debouncing into
   * {@link compileProjectAsync} at the same generation share this promise
   * instead of queueing a second identical query.
   */
  private pendingCompile: { generation: number; promise: Promise<CompileResult> } | null =
    null;
  // ── Worker road (docs/editor-worker-spec.md §8; W4 flush model
  //    replaced by the W5b continuous replica) ──
  private readonly workerEnabled: boolean;
  private workerClient: SessionClient | null = null;
  private workerFailed = false;
  /** Doc ids the MAIN session returned for forwarded open calls, in
   *  order; the worker's mutationResult events must match them exactly
   *  or the replica is desynced and gets dropped (fallback in-process).
   *  Deterministic by construction — both sessions assign ids
   *  monotonically from the same replayed call sequence — so this queue
   *  is a tripwire, not a mapping. */
  private readonly workerExpectedDocIds: (number | null)[] = [];
  private readonly workerFactory?: () => import("./worker/worker-transport.js").WorkerLike | null;
  /**
   * Idle handles this class has scheduled via {@link deferGatedCall} —
   * `renameFile`'s yield below, and (issue #2794 review) `studio-ui`'s
   * `runGatedStructuralOp` for the symbol-menu `moveStitch`/`promoteStitch`/
   * `demoteKnot` ops — that have not yet settled, mapped to the `reject`
   * function that lets {@link destroy} abort them. The same freed-wasm
   * discipline this class already applies elsewhere (`destroy()`'s listener
   * detach, `FileChangeHub.getContent`'s `destroyed` check): one guard,
   * meant to cover every current and future gated call this class defers
   * via `scheduleIdleWork`, not a `renameFile`-specific patch. Before this
   * existed, an unmount landing inside the ≤300ms idle window left the
   * scheduled callback to fire anyway and go on to call `this.session.*` on
   * a handle `destroy()` had already freed.
   */
  private readonly pendingIdleWork = new Map<IdleHandle, (reason: Error) => void>();

  constructor(options: ProjectSessionOptions) {
    this.provider = options.provider;
    this.hostEntryFile = options.entryFile;
    this.entryIsExplicit = options.entryIsExplicit ?? false;
    // The perf proxy times every wasm call from this one choke point (all
    // DocHandles and panel pulls share this instance); it is a pass-through
    // branch per call while the probe is disabled (the production state).
    this.workerEnabled = options.workerSession ?? false;
    this.workerFactory = options.workerFactory;
    this.session = withWorkerMirror(
      withPerfTiming(options.session ?? new EditorSessionHandle()),
      {
        // W5b: continuous forwarding — every mutation posts to the worker
        // replica the moment it happens (ordered by the client's mutation
        // stream), instead of the W4 query-time flush.
        config: (method, args) => this.forwardToWorker((c) => c.config(method, ...args)),
        file: (method, args) => this.forwardToWorker((c) => c.files(method, ...args)),
        docEdit: (method, args) =>
          this.forwardToWorker((c) => {
            const doc = args[0] as number;
            if (method === "applyEditsDocument") {
              c.applyEdits(doc, args[1] as { from: number; to: number; insert: string }[]);
            } else {
              c.pushSource(doc, String(args[1]));
            }
          }),
        replay: (method, args, returned) =>
          this.forwardToWorker((c) => {
            if (method === "openDocument" || method === "openFragment") {
              this.expectWorkerDocId(returned);
            }
            c.files(method, ...args);
          }),
      },
    );
    this.client = new SessionClient(new LocalTransport(this.session));
    // W5b: spawn the replica EAGERLY so it exists before any mutation —
    // the prime streams only pre-existing state (an injected session's
    // files; ordinarily just the untouched baseline) and every later
    // mutation forwards exactly once through the mirror hooks.
    if (this.workerEnabled) this.ensureWorkerClient();
    this.onExternalFileChange = options.onExternalFileChange;
    this.onFileConflict = options.onFileConflict;
    this.onProjectConfigWarnings = options.onProjectConfigWarnings;
    this.onProjectConfigError = options.onProjectConfigError;
    this.changes = new FileChangeHub({
      getContent: (path) => (this.destroyed ? null : this.session.getFileSource(path)),
      onFlush: options.onFilesChanged,
      onFileConflict: options.onFileConflict,
      debounceMs: options.changeDebounceMs,
      deliveryPersists: options.egressPersists,
    });
  }

  /**
   * Re-run `brink.toml` discovery (issue #2324) and forward any warnings.
   * Uses `discoverProjectConfig` (#1414), not `applyProjectConfig` (#1005):
   * it walks the session's own already-loaded documents from `entryFile`
   * up to the tree root, so this class — which already loads every
   * provider file (including `brink.toml`, an ordinary project file) into
   * the session — needs no host-specific directory-walk/read code of its
   * own to locate or read the text. `applyProjectConfig` would require
   * this class (or its caller) to separately fetch the file's text through
   * the `FileProvider`, duplicating work `initialize()` already did.
   *
   * Safe to call whenever `brink.toml` might have changed — a missing file
   * is not an error (`discoverProjectConfig` returns `[]`), and a
   * recognized-key/lint-code warning list is forwarded even when empty.
   *
   * `discoverProjectConfig` throws on malformed TOML or a recognized key
   * with an invalid value (issue #2324's review finding): every caller of
   * this method — `initialize()`, `notifyFileChanged`/`applyEdit`,
   * `addFile`, `deleteFile`, `renameFile`, and the external-change handler —
   * is a place a mid-edit typo in `brink.toml` could otherwise take down,
   * from a mount-time failure with no editor to fix the file in, to an
   * uncaught exception on every subsequent keystroke. Caught here, once, at
   * the single call site all of them share, and reported through
   * {@link ProjectSessionOptions.onProjectConfigError} instead of
   * rethrowing.
   *
   * Also owns `[project] entry` precedence (issue #2331, ruled 2026-08-07
   * "`[project] entry` beats `mountStudio`'s `entryFile`"): a discovered
   * `brink.toml` naming an `entry` that resolves to a real file in this
   * session supersedes {@link hostEntryFile} — the host's constructor-time
   * `entryFile` argument is only the fallback for a configless project (no
   * `brink.toml`, or one that doesn't set `entry`). {@link configuredEntry}
   * is wholesale-replaced (not merged) on every call, so a `brink.toml` edit
   * that removes `entry` genuinely clears the supersession. This is the one
   * place that reconciles the two, so every caller of `getEntryFile()`/
   * `compileProject()` — and `mountStudio`'s initial-tab open, which reads
   * `getEntryFile()` after `initialize()` — automatically sees whichever
   * one wins. A config-named entry that does NOT resolve to a real project
   * file never supersedes anything ({@link configuredEntry} is cleared to
   * `null`) and is reported through the same
   * {@link ProjectSessionOptions.onProjectConfigWarnings} channel as every
   * other `brink.toml` misconfiguration — no new warning channel for this
   * one case.
   */
  private applyProjectConfig(): void {
    let warnings: string[];
    try {
      warnings = this.session.discoverProjectConfig(this.hostEntryFile);
    } catch (err) {
      this.onProjectConfigError?.(err instanceof Error ? err.message : String(err));
      return;
    }
    const configuredEntry = this.sessionConfiguredEntry();
    if (configuredEntry !== null && this.session.getFileSource(configuredEntry) !== null) {
      this.configuredEntry = configuredEntry;
    } else {
      if (configuredEntry !== null) {
        warnings = [
          ...warnings,
          `project.entry \`${configuredEntry}\` in brink.toml does not resolve to a project file (ignored)`,
        ];
      }
      this.configuredEntry = null;
    }
    this.onProjectConfigWarnings?.(warnings);
  }

  /**
   * Feature-detected wrapper around `session.getConfiguredEntry()` (issue
   * #2331 review finding): `session` is a public injection seam
   * (`ProjectSessionOptions.session`), and a pre-#2331 stub/handle has no
   * such method — calling it unguarded would throw out of `initialize()`
   * for any host stub that predates this feature. Same pattern as
   * {@link sessionIsReadOnly}.
   */
  private sessionConfiguredEntry(): string | null {
    return typeof this.session.getConfiguredEntry === "function"
      ? this.session.getConfiguredEntry()
      : null;
  }

  /**
   * `[project] indent` from the applied `brink.toml` (#3149), or `null`
   * when the project set none — feature-detected like
   * {@link sessionConfiguredEntry}, so an older wasm build without the
   * accessor degrades to "the project said nothing" rather than throwing.
   *
   * The caller resolves `null` to `DEFAULT_INDENT`; this does not, so the
   * two cases stay distinguishable.
   */
  getConfiguredIndent(): number | null {
    return typeof this.session.getConfiguredIndent === "function"
      ? this.session.getConfiguredIndent()
      : null;
  }

  /**
   * Every diagnostic code the compiler knows (#3169) — the Settings
   * Diagnostics section's list.
   *
   * Reads the MODULE-level accessor rather than anything on `this.session`.
   * The registry is static compiler data: it depends on no session, cannot
   * go stale within a build, and is exported alongside `getTokenTypeNames`
   * for exactly that reason. Reaching for `this.session.…` here was a real
   * bug — the method does not exist there, so the section rendered an empty
   * registry and filed every configured code under "unknown to this
   * compiler".
   */
  getDiagnosticRegistry(): unknown[] {
    return getDiagnosticRegistry();
  }

  /**
   * The project's proper nouns for the prose dictionary (#3210).
   *
   * Cached per compile generation, not per call: the prose extension asks on
   * every debounce, and this walks every file's symbols and line contexts.
   * `deliverCompile` invalidates it — a name added in one file must reach the
   * checker in another, so the cache key is the project's analysis, not a
   * file's text.
   */
  getProseDictionary(): string[] {
    if (this.proseDictionaryCache === null) {
      try {
        this.proseDictionaryCache = [
          ...this.session.getProseDictionary(),
          ...this.session.getConfiguredProseDictionary(),
        ];
      } catch {
        // No analysis yet (first paint). An empty dictionary means the
        // checker flags invented names for one debounce, not that it breaks.
        return [];
      }
    }
    return this.proseDictionaryCache;
  }

  /** Drop the prose dictionary cache — called when analysis changes. */
  invalidateProseDictionary(): void {
    this.proseDictionaryCache = null;
  }

  /**
   * `[prose] dialect` from `brink.toml`, or `"american"` when unset.
   *
   * The default lives here rather than in the checker so both roads agree:
   * the session is what the editor asks, and the settings UI shows the same
   * fallback.
   */
  getProseDialect(): string {
    try {
      return this.session.getConfiguredProseDialect() ?? "american";
    } catch {
      return "american";
    }
  }

  /**
   * Whether `[prose] enable` allows prose checking. Defaults to ON when the
   * config says nothing — a project that has never heard of the setting
   * should get the feature, not silently miss it.
   */
  isProseEnabled(): boolean {
    try {
      return this.session.getConfiguredProseEnable() ?? true;
    } catch {
      return true;
    }
  }

  /** Load all files from provider and resolve INCLUDEs. */
  async initialize(): Promise<void> {
    const endInit = perfSpan("project.initialize");
    const endList = perfSpan("project.initialize.listFiles");
    const files = await this.provider.listFiles();
    endList(files.length);
    this.assertLive();
    for (const file of files) {
      const endRead = perfSpan("project.initialize.readFile");
      const content = await this.provider.readFile(file);
      endRead(content.length);
      this.assertLive();
      this.session.updateFile(file, content);
      // Host-loaded content is the clean baseline: the project starts with
      // zero dirty files, and a no-op edit flush never reaches the host.
      this.changes.setBaseline(file, content);
    }

    const endIncludes = perfSpan("project.initialize.resolveIncludes");
    await this.resolveIncludes();
    endIncludes();
    this.assertLive();

    // `brink.toml` (issue #2324): every project file is loaded above, so
    // discovery can run once, right here, before anything analyzes/compiles.
    this.applyProjectConfig();

    // Register external change callback if the provider supports it. Keep the
    // unsubscribe so destroy() can detach it — otherwise a later external change
    // would call into a freed wasm session (use-after-free).
    this.unsubscribeExternal = this.provider.onExternalChange?.((path, content) => {
      if (this.destroyed) return;

      // Guard against silent data loss (issue #320): if the host rewrites a
      // file the studio has an unsaved, divergent buffer for, overwriting the
      // wasm buffer + re-baselining would clobber the pending edit with no
      // recourse. Detect that BEFORE mutating anything.
      const conflict =
        content === null ? null : this.changes.detectExternalConflict(path, content);
      if (conflict !== null) {
        // SAFE DEFAULT: keep the editor buffer (no updateFile), do not
        // re-baseline (no applyExternal) — flag the path conflicted and hand
        // both versions to the host for reconciliation (Track V merge view).
        this.changes.markConflicted(path);
        this.onFileConflict?.(conflict);
        return;
      }

      if (content === null) {
        this.session.removeFile(path);
      } else {
        this.session.updateFile(path, content);
      }
      // No conflict (clean buffer, or buffer already equals disk): the host's
      // content is the new truth — re-baseline, supersede any pending
      // studio-side change for the path (no echo back to the host).
      this.changes.applyExternal(path, content);
      this.onExternalFileChange?.(path, content);
      // `brink.toml` rewritten from outside the studio (issue #2324): the
      // file just landed in the session via `updateFile` above — re-run
      // discovery so an external edit is not silently ignored either.
      if (isProjectConfigPath(path)) this.applyProjectConfig();
    });
    endInit(files.length);
  }

  /** Underlying wasm session. */
  getSession(): EditorSessionHandle {
    return this.session;
  }

  /**
   * Yield to the next idle slot ahead of a gated wasm call — a call whose
   * Rust op runs the full-project breakage/collision gate, the way {@link
   * renameFile}'s call to `this.session.renameFile` below does. Public
   * (issue #2794 review) so every gated call this class's callers defer
   * shares one guard instead of a per-site `scheduleIdleWork` sprinkle —
   * `studio-ui`'s `runGatedStructuralOp` (the symbol-menu `moveStitch`/
   * `promoteStitch`/`demoteKnot` ops) awaits this directly rather than
   * rolling its own bare `scheduleIdleWork` yield, which had none of the
   * protections below and could reach a freed `session` the same way
   * `renameFile` once could. Unlike a bare `scheduleIdleWork` await, this:
   *
   *  - Rejects immediately, without scheduling anything, if the session is
   *    already destroyed when called (a caller invoking a gated method after
   *    `destroy()` — a caller bug, but one that must not reach a freed
   *    handle either).
   *  - Tracks the idle handle in {@link pendingIdleWork} so {@link destroy}
   *    can `cancelIdleWork` it — otherwise the browser/timer callback fires
   *    into a session that has already freed its wasm handle.
   *  - Rejects (rather than leaving the promise to hang forever) if
   *    `destroy()` runs while this is still waiting: the caller's `await`
   *    throws, so the code that would call `this.session.*` after the yield
   *    never runs. This mirrors `applyRename`'s existing `try`/`finally` in
   *    `binder.ts`, which already restores its own local state on any
   *    rejection — a caller that swallows this rejection with no
   *    catch/finally still won't touch the freed session, since the
   *    rejection prevents its own continuation from ever executing.
   */
  deferGatedCall(): Promise<void> {
    if (this.destroyed) {
      return Promise.reject(
        new Error("ProjectSession destroyed before a deferred gated call ran"),
      );
    }
    return new Promise<void>((resolve, reject) => {
      const handle = scheduleIdleWork(() => {
        this.pendingIdleWork.delete(handle);
        resolve();
      });
      this.pendingIdleWork.set(handle, reject);
    });
  }

  /**
   * Guard against resuming `this.session.*`/`this.changes.*` work after
   * `destroy()` has already freed the wasm handle. {@link deferGatedCall}
   * only closes the destroy()-safety window around its own ≤300ms idle
   * yield (issue #2794/#2798) — it has nothing to say about the LARGER
   * window that opens right after, when a method `await`s the host
   * provider itself (Tauri IPC, unbounded, and not a handle `destroy()` can
   * cancel the way it cancels a pending idle handle via {@link
   * pendingIdleWork}). Every method here that resumes session state after
   * such an await calls this the instant its continuation runs, before
   * touching `this.session`/`this.changes` again — one seam every
   * post-host-IO-await touch goes through (issue #2802), generalizing past
   * the idle-yield-specific guard above rather than repeating a per-site
   * `if (this.destroyed)` check at each of `renameFile`, `deleteFile`,
   * `requestFile`, `resolveIncludes`, `initialize`, and `addFile`. Throws the same
   * error family {@link deferGatedCall} rejects with, so a caller catching
   * a destroy()-during-await race sees one shape regardless of which await
   * it landed in.
   */
  private assertLive(): void {
    if (this.destroyed) {
      throw new Error("ProjectSession destroyed while awaiting the host provider");
    }
  }

  /**
   * The project's entry file — for compilation, and (via `mountStudio`,
   * read after `initialize()`) the initial tab. This is the constructor's
   * `entryFile` option ({@link hostEntryFile}) UNLESS `applyProjectConfig`
   * found a `brink.toml` naming a valid `[project] entry` (issue #2331,
   * ruled 2026-08-07), which supersedes it; see that method's doc for the
   * full precedence rule. Never sticky past the config that set it — see
   * {@link configuredEntry}. The one carve-out is an EXPLICIT host entry
   * ({@link entryIsExplicit}, the file-anchored open model ruled
   * 2026-08-23): a human's explicit open is not a default, so config never
   * supersedes it.
   */
  getEntryFile(): string {
    if (this.entryIsExplicit) return this.hostEntryFile;
    return this.configuredEntry ?? this.hostEntryFile;
  }

  /** Create a new file and add it to the session (`file.new`). Recorded as
   *  a "created" change — the host learns about the file's existence. */
  async addFile(path: string, content: string = ""): Promise<void> {
    await this.provider.createFile(path, content);
    this.assertLive();
    this.session.updateFile(path, content);
    this.changes.record(path, "created");
    // A `brink.toml` created after mount (issue #2324) was previously
    // undiscoverable — the file wasn't there for `initialize()`'s discovery
    // call, and nothing re-ran it.
    if (isProjectConfigPath(path)) this.applyProjectConfig();
  }

  /** Remove a file from the wasm session (does not delete from provider). */
  closeFile(path: string): void {
    this.session.removeFile(path);
  }

  /** Whether the provider can delete files (drives the binder's delete UI). */
  canDeleteFiles(): boolean {
    return this.provider.deleteFile !== undefined;
  }

  /**
   * Delete a file: remove it from the provider and the wasm session, and
   * record a "deleted" change so the host's mirror drops it too. Unlike
   * {@link closeFile} (session-only eviction), this is a real deletion. The
   * caller is responsible for snapshotting content first if undo is wanted
   * and for closing any open views (see the store's `deleteFile`).
   *
   * Refuses (no provider write, no session mutation) when `path` currently
   * resolves to a mounted stdlib copy (issue #2306/#2343): the Binder's
   * Library section offers no delete affordance, but `list_files` now
   * lists mounted files (the exact route this guard closes — a caller
   * reaching a mounted path outside the Binder's own gating must not
   * delete the mount, and definitely must not have the provider write a
   * "deletion" of a file it never wrote in the first place). Returns `false`
   * on refusal rather than throwing, matching {@link applyEdit}'s and
   * `EditorSession::remove_file`'s sibling contract — the store's
   * `deleteFilesWithUndo` awaits this with no try/catch, so a throw here
   * would leave a tab already closed by the caller with nothing telling the
   * user why the delete silently vanished (issue #2343 review finding).
   */
  async deleteFile(path: string): Promise<boolean> {
    if (this.sessionIsReadOnly(path)) {
      return false;
    }
    await this.provider.deleteFile?.(path);
    this.assertLive();
    this.session.removeFile(path);
    this.changes.record(path, "deleted");
    // A deleted `brink.toml` (issue #2324) may uncover an ancestor
    // `brink.toml` discovery previously stopped short of (or find none,
    // which is not an error — see `applyProjectConfig`'s doc comment).
    if (isProjectConfigPath(path)) this.applyProjectConfig();
    return true;
  }

  /** Whether files can be renamed/moved. True when the provider has an atomic
   *  rename, or can delete (so the create+delete fallback can drop the old
   *  file). Drives the binder's rename/move affordances. */
  canRenameFiles(): boolean {
    return this.provider.renameFile !== undefined || this.provider.deleteFile !== undefined;
  }

  /**
   * Rename/move a file, rewriting `INCLUDE` references. The session's rename op
   * (pure) computes the moved content + the referencing files' edits; this
   * applies them: writes the content under `newPath`, drops `oldPath`, and
   * rewrites referrers — recording created/deleted/modified so the host mirror
   * follows. Returns the referrer paths whose `INCLUDE`s were rewritten (so the
   * caller can refresh their views). Throws if the op fails (unknown source, or
   * `newPath` taken).
   *
   * Off the paint path (#2776, generalizing #2767's `runGatedStructuralOp`
   * remedy — spec §7.7.4): `rename_file` runs the same op-agnostic breakage
   * gate as `moveStitch`/`promoteStitch`/`demoteKnot` (`gate_with_source`,
   * `crates/internal/brink-ide/src/file_rename.rs`) — an overlay re-analysis
   * of the whole project — so the wasm call below is deferred to the next
   * idle slot via `scheduleIdleWork` rather than run inline. This method
   * stays async either way, so every existing caller gets the deferral for
   * free; the synchronous busy-state commit a caller needs to paint BEFORE
   * this yields lives one layer up, in the caller that has store access
   * (`applyRename`, `packages/studio-store/src/slices/binder.ts`) — this
   * class has no UI-state concept of its own to commit one.
   *
   * Returns a {@link RenameFileResult}, not a bare referrer array (issue
   * #2918): the wasm op's `safe`/`introduced_diagnostics` breakage-gate
   * verdict used to be discarded here, so a move that broke a reference
   * applied with no way for any caller to know. The move still applies
   * either way (this is the notification FLOOR #2918 shipped, not a
   * preflight gate — see that issue for why) — callers decide how to report
   * an unsafe move; they can no longer fail to know about one.
   */
  async renameFile(oldPath: string, newPath: string): Promise<RenameFileResult> {
    if (oldPath === newPath) return { referrers: [], safe: true, introducedDiagnostics: [] };
    await this.deferGatedCall();
    type RenameFileOp = ReturnType<EditorSessionHandle["renameFile"]>;
    // PAINT-PATH-DEFERRED rename-file: gated (structural_result::gate_with_source
    // via crates/internal/brink-ide/src/file_rename.rs) — deferred by the
    // deferGatedCall yield immediately above (#2776; destroy()-safe since
    // #2794 — see that method's doc comment) and run through the async
    // session facade at interactive priority (W2e).
    const result = await this.structuralQuery<RenameFileOp>("renameFile", [oldPath, newPath]);
    if (!result.ok) {
      throw new Error(result.error ?? `cannot rename ${oldPath}`);
    }
    const newSource = result.new_source ?? this.session.getFileSource(oldPath) ?? "";

    // Session: add the moved file under its new key, drop the old one.
    this.session.updateFile(newPath, newSource);
    this.session.removeFile(oldPath);

    // Cross-file INCLUDE rewrites — through the shared apply-edits seam.
    const referrers: string[] = [];
    for (const edit of result.cross_file_edits) {
      this.applyEdit(edit.path, edit.new_source);
      referrers.push(edit.path);
    }

    // Provider: atomic rename, or create-new + delete-old fallback. Both
    // branches hand over `newSource` — an atomic rename moves the file's
    // PRE-rewrite bytes, so a host that persisted only those would keep
    // stale outbound `INCLUDE` paths for any move that crossed a directory
    // boundary (#2425), while the fallback branch already wrote the
    // rewritten source through `createFile`.
    if (this.provider.renameFile) {
      await this.provider.renameFile(oldPath, newPath, newSource);
    } else {
      await this.provider.createFile(newPath, newSource);
      await this.provider.deleteFile?.(oldPath);
    }
    this.assertLive();

    // Host egress for the moved file itself.
    this.changes.record(newPath, "created");
    this.changes.record(oldPath, "deleted");

    // `brink.toml` moved into or out of the tree (issue #2324): the
    // ancestor `brink.toml` discovery finds by walk-up depends on exact
    // paths, so either direction can change what's discovered.
    if (isProjectConfigPath(oldPath) || isProjectConfigPath(newPath)) {
      this.applyProjectConfig();
    }

    return { referrers, safe: result.safe, introducedDiagnostics: result.introduced_diagnostics };
  }

  /**
   * Rename/move a directory (#314, wired in #2587), rewriting every affected
   * `INCLUDE` against ONE atomic pre-move snapshot — moved files' outbound
   * includes, inbound includes from files outside the folder, and
   * intra-folder sibling includes are all rewritten together, unlike a
   * per-file {@link renameFile} loop (which computes each file's cross-file
   * edits independently against whatever has already moved, so a
   * same-basename directory move — every moved file keeps its own basename,
   * only the prefix changes — can leave an outside referrer's `INCLUDE`
   * pointing at the old, now-nonexistent path; see issue #2587).
   *
   * The session op (pure) computes every moved file's content plus outside
   * referrers' edits from that one snapshot; this applies them the same way
   * {@link renameFile} applies a single-file result — a provider write is
   * inherently per-file, so #314's atomicity guarantee lives in the EDIT
   * COMPUTATION above, not in these writes. All-or-nothing: the op itself
   * refuses (no partial move) on a destination collision or an empty
   * folder, so either every file in `oldPrefix` moves consistently or none
   * do — a caller wanting partial-move-with-skips would reintroduce exactly
   * the inconsistency #314 exists to prevent.
   *
   * Returns the `{oldPath, newPath}` pairs actually moved (so the caller can
   * re-key open tabs and build an undo entry) plus the outside referrer
   * paths whose `INCLUDE`s were rewritten (so the caller can refresh their
   * views). Throws if the op fails (empty folder, a destination collision,
   * or — a TS-side fence neither the Rust op nor the wasm binding has, see
   * below — any moved file resolving to a mounted stdlib copy) — same
   * contract as {@link renameFile}.
   *
   * Off the paint path (#2587, same remedy as {@link renameFile} — #2776,
   * spec §7.7.4): `rename_dir` runs the identical `gate_with_source`
   * breakage gate as `rename_file`
   * (`crates/internal/brink-ide/src/dir_rename.rs`), so the wasm call below
   * is deferred to the next idle slot via {@link deferGatedCall} rather than
   * run inline. This method stays async either way, so the synchronous
   * busy-state commit a caller needs to paint BEFORE this yields lives one
   * layer up, in the caller with store access (`renameFolder`,
   * `packages/studio-store/src/slices/binder.ts`) — same split as
   * `renameFile`/`applyRename`.
   *
   * Returns a {@link RenameDirResult}, carrying the same `safe`/
   * `introducedDiagnostics` breakage-gate verdict {@link renameFile} now
   * does (issue #2918) — `DirMoveResult.safe`/`.introduced_diagnostics`
   * were computed correctly by the wasm op all along but discarded here,
   * so a folder move that broke an outside reference applied silently. Same
   * floor-not-gate contract as `renameFile`: the move still applies.
   */
  async renameDir(oldPrefix: string, newPrefix: string): Promise<RenameDirResult> {
    if (oldPrefix === newPrefix) {
      return { moved: [], referrers: [], safe: true, introducedDiagnostics: [] };
    }
    await this.deferGatedCall();
    type RenameDirOp = ReturnType<EditorSessionHandle["renameDir"]>;
    // PAINT-PATH-DEFERRED rename-dir: gated (structural_result::gate_with_source
    // via crates/internal/brink-ide/src/dir_rename.rs) — deferred by the
    // deferGatedCall yield immediately above (#2587, mirroring #2776's
    // rename-file remedy; destroy()-safe since #2794 — see that method's
    // doc comment).
    const result = await this.structuralQuery<RenameDirOp>("renameDir", [oldPrefix, newPrefix]);
    if (!result.ok) {
      throw new Error(result.error ?? `cannot rename directory ${oldPrefix}`);
    }

    // Read-only fence (issue #2306/#2343, #2916 review finding): `rename_dir`
    // discovers its own file set from the db — every file under `oldPrefix`
    // — rather than from a caller-filtered list, and (unlike `rename_file`)
    // NEITHER the real Rust op nor the wasm binding checks whether any of
    // them is a mounted stdlib copy. A project with its own `std/` folder
    // could otherwise have a folder move sweep a mounted copy along with it
    // — forking the read-only library into the project and making the host
    // provider create/delete a file it never wrote, the exact hazard
    // `renameFile`/`deleteFile`/`applyEdit` already fence elsewhere. Refuse
    // the WHOLE move (same all-or-nothing contract as every other refusal
    // this op can return) before any mutation, rather than silently
    // skipping the mounted file mid-move.
    if (result.moved_files.some((mf) => this.sessionIsReadOnly(mf.old_path))) {
      throw new Error(
        `cannot rename directory ${oldPrefix}: contains a read-only file`,
      );
    }

    // Every path this move leaves occupied at the end — i.e. every entry's
    // `new_path`. Used below to tell a genuinely stale `old_path` (nothing
    // else needs it) apart from an `old_path` that is ALSO another entry's
    // destination (issue #2916 review, "apply-order clobber when the
    // destination nests inside the source"): moving folder `a` to
    // `a/nested` with files `a/a.ink` + `a/nested/a.ink` moves the former to
    // `a/nested/a.ink` and the latter to `a/nested/nested/a.ink` — so
    // `a/nested/a.ink` is simultaneously the FIRST entry's destination and
    // the SECOND entry's source. Writing+removing per entry in `old_path`
    // order would have the second entry's removal of `a/nested/a.ink` wipe
    // out the correct content the first entry had just written there.
    const newPaths = new Set(result.moved_files.map((mf) => mf.new_path));
    const staleOldPaths = result.moved_files.filter((mf) => !newPaths.has(mf.old_path));

    // Session: write every moved file's new content FIRST — all
    // destinations — before removing any old path, so a destination that is
    // itself another entry's (about-to-be-removed) source is never wiped
    // out by that entry's removal.
    for (const mf of result.moved_files) {
      this.session.updateFile(mf.new_path, mf.new_source);
    }
    for (const mf of staleOldPaths) {
      this.session.removeFile(mf.old_path);
    }

    // Cross-file INCLUDE rewrites — outside referrers, through the shared
    // apply-edits seam.
    const referrers: string[] = [];
    for (const edit of result.cross_file_edits) {
      this.applyEdit(edit.path, edit.new_source);
      referrers.push(edit.path);
    }

    // Provider: same write-everything-first, then-remove-only-what's-stale
    // order as the session pass above. Deliberately `createFile`, not
    // `provider.renameFile`: an atomic per-file rename COMBINES the write
    // and the old-path removal into one call, which is exactly the unsafe
    // shape this fix removes — a "stale" old path for one entry can be
    // another entry's live destination that must survive the move.
    for (const mf of result.moved_files) {
      await this.provider.createFile(mf.new_path, mf.new_source);
      this.assertLive();
      this.changes.record(mf.new_path, "created");
    }
    for (const mf of staleOldPaths) {
      await this.provider.deleteFile?.(mf.old_path);
      this.assertLive();
      this.changes.record(mf.old_path, "deleted");
    }

    // `brink.toml` moved into or out of the tree (issue #2324).
    if (
      result.moved_files.some(
        (mf) => isProjectConfigPath(mf.old_path) || isProjectConfigPath(mf.new_path),
      )
    ) {
      this.applyProjectConfig();
    }

    return {
      moved: result.moved_files.map((mf) => ({ oldPath: mf.old_path, newPath: mf.new_path })),
      referrers,
      safe: result.safe,
      introducedDiagnostics: result.introduced_diagnostics,
    };
  }

  /**
   * Compile the project from its entry file. Cached against the session's
   * mutation generation: with several live views each compiling on their own
   * debounce, only the first compile after a change does real work.
   */
  compileProject(): CompileResult {
    const generation = this.session.generation;
    if (this.lastCompile !== null && this.lastCompile.generation === generation) {
      // Zero-duration counter: hit count vs `project.compileProject` count
      // reads as the generation cache's effectiveness in a report.
      perfSpan("project.compileProject.cacheHit")();
      return this.lastCompile.result;
    }
    const end = perfSpan("project.compileProject");
    const result = this.session.compileProject(this.getEntryFile());
    end();
    this.lastCompile = { generation, result };
    return result;
  }

  /**
   * {@link compileProject} through the async session facade (W2a) — the
   * road the diagnostics extension rides so the compile stops occupying
   * the caller's turn. Same generation cache; additionally dedups
   * concurrent callers onto one in-flight query.
   *
   * The cache entry is keyed by the generation captured at *issue* time.
   * If a mutation lands between issue and execution, the entry is keyed
   * one generation early — a caller at the newer generation then misses
   * the cache and compiles again, so the mis-key costs one extra compile,
   * never a stale hit (a hit requires an exact generation match).
   */
  compileProjectAsync(): Promise<CompileResult> {
    const generation = this.session.generation;
    if (this.lastCompile !== null && this.lastCompile.generation === generation) {
      perfSpan("project.compileProject.cacheHit")();
      return Promise.resolve(this.lastCompile.result);
    }
    if (this.pendingCompile !== null && this.pendingCompile.generation === generation) {
      return this.pendingCompile.promise;
    }
    const end = perfSpan("project.compileProject");
    const promise = this.projectQuery<CompileResult>("compileProject", [
      this.getEntryFile(),
    ]).then(
      (value) => {
        end();
        if (this.pendingCompile?.promise === promise) this.pendingCompile = null;
        this.lastCompile = { generation, result: value };
        return value;
      },
      (error: unknown) => {
        end();
        if (this.pendingCompile?.promise === promise) this.pendingCompile = null;
        throw error;
      },
    );
    this.pendingCompile = { generation, promise };
    return promise;
  }

  /** The async session facade (docs/editor-worker-spec.md §5.2) — the
   *  surface later migration waves move onto. One client per project. */
  sessionClient(): SessionClient {
    return this.client;
  }

  /**
   * One structural compute through the async facade (W2e): interactive
   * priority — ordered after queued mutations, never coalesced or
   * dropped. The wasm structural ops are COMPUTE-ONLY
   * (`structural_result::gate_with_source`): they return new sources + a
   * breakage report and mutate nothing — application happens through the
   * ordinary `updateFile`/`applyEdit` mutations that follow — so query
   * semantics are exactly right for them. Rejects with
   * `QueryDroppedError("cancelled")` if the session is destroyed while
   * the compute is queued.
   */
  structuralQuery<T>(method: string, args: readonly unknown[]): Promise<T> {
    return this.docClient()
      .query<T>(method, [...args], { priority: "interactive" })
      .promise.then((r) => r.value);
  }

  /**
   * One project-level (doc-independent) pull — compile, outline, story
   * graph, closure. Runs on the Web Worker road when enabled and healthy:
   * the worker session is a continuously-forwarded replica (W5b), and
   * the scheduler guarantees queued mutations apply before the query.
   * Everywhere else (no `workerSession`, no `Worker`, boot/crash
   * failure) this is the in-process client — identical semantics,
   * main-thread execution.
   */
  projectQuery<T>(
    method: string,
    args: readonly unknown[],
    options: { coalesceKey?: string } = {},
  ): Promise<T> {
    const client = this.ensureWorkerClient() ?? this.client;
    return client
      .query<T>(method, [...args], {
        priority: "background",
        ...(options.coalesceKey !== undefined ? { coalesceKey: options.coalesceKey } : {}),
      })
      .promise.then((r) => r.value);
  }

  /** The client doc-scoped queries ride (W5b): the worker replica when
   *  live, else the in-process client. Both see the same state — the
   *  main session stays fully written until the W5c delete. */
  docClient(): SessionClient {
    return this.ensureWorkerClient() ?? this.client;
  }

  /** Whether queries currently ride a live worker replica — false in
   *  worker-less environments and after a crash-fallback. Consumers that
   *  address the HOST REALM itself (the perf HUD's `hostPerfReport`) use
   *  this to avoid asking the in-process road, where the answer would
   *  just mirror the main realm's own state. */
  workerActive(): boolean {
    return this.ensureWorkerClient() !== null;
  }

  /** Post one mutation to the worker replica, creating it on first use.
   *  A closed/crashed worker drops the forward silently — the replica is
   *  already marked failed and every query road falls back in-process. */
  private forwardToWorker(post: (client: SessionClient) => void): void {
    const client = this.ensureWorkerClient();
    if (client === null) return;
    try {
      post(client);
    } catch {
      this.dropWorker();
    }
  }

  /** Record the main session's returned doc id for a forwarded open; the
   *  worker's mutationResult event must echo the same id in order. */
  private expectWorkerDocId(returned: unknown): void {
    this.workerExpectedDocIds.push(typeof returned === "number" ? returned : null);
  }

  private ensureWorkerClient(): SessionClient | null {
    if (!this.workerEnabled || this.workerFailed || this.destroyed) return null;
    if (this.workerClient !== null) return this.workerClient;
    const worker = (this.workerFactory ?? createSessionWorker)();
    if (worker === null) {
      this.workerFailed = true;
      return null;
    }
    const transport = new WorkerTransport(worker, { onCrash: () => this.dropWorker() });
    const client = new SessionClient(transport);
    client.onEvent((event) => {
      const e = event as { type?: string; method?: string; value?: unknown } | null;
      if (e?.type === "bootError") {
        this.dropWorker();
        return;
      }
      // Doc-id determinism tripwire (W5b): a forwarded open must mint the
      // SAME id on the replica. A mismatch means the sessions diverged —
      // drop the worker rather than serve queries against the wrong doc.
      if (
        e?.type === "mutationResult" &&
        (e.method === "openDocument" || e.method === "openFragment")
      ) {
        const expected = this.workerExpectedDocIds.shift();
        if (expected !== (typeof e.value === "number" ? e.value : null)) this.dropWorker();
      }
    });
    this.workerClient = client;
    // Prime the replica with any state that predates it (an injected
    // session with prior files; ordinarily just nothing — the client is
    // created by the FIRST forwarded mutation, so everything later
    // arrives through the continuous stream). Mounted stdlib is skipped:
    // the replica's own session mounts its own copy.
    for (const file of this.session.listFiles()) {
      if (file.mounted) continue;
      const content = this.session.getFileSource(file.path);
      if (content !== null) client.files("updateFile", file.path, content);
    }
    return client;
  }

  /** Worker crashed or failed to boot: reject everything in flight (the
   *  rejected consumers retry on their own cadence and land on the
   *  in-process road) and never try the worker again this session. */
  private dropWorker(): void {
    this.workerFailed = true;
    const client = this.workerClient;
    this.workerClient = null;
    client?.close();
  }


  /**
   * Report that `path`'s session content changed: provider write-back plus
   * a "modified" record on the change hub (host egress). Every mutation
   * path lands here — the CM6 edit flush calls it directly; bulk edits go
   * through {@link applyEdit}. No-op changes (content equal to the host
   * baseline) are dropped by the hub.
   */
  /** See the feature-detection note at the first call site. */
  private sessionIsReadOnly(path: string): boolean {
    return typeof this.session.isReadOnly === "function" && this.session.isReadOnly(path);
  }

  /**
   * Whether `path` currently resolves to a mounted stdlib copy (issue
   * #2306/#2343) — the public wrapper `DocumentSessions` reads to put a
   * mounted file's CM6 view into `EditorState.readOnly` (`document-sessions.ts`
   * `slotExtensions`), so a keystroke over the Binder's Library section
   * genuinely can't type rather than silently no-oping at the wasm layer.
   * Same feature-detected fallback as {@link sessionIsReadOnly}: `false` for
   * an injected session/stub that predates #2306.
   */
  isReadOnly(path: string): boolean {
    return this.sessionIsReadOnly(path);
  }

  notifyFileChanged(path: string): void {
    // Session-level read-only enforcement (issue #2306, ruled 2026-08-06
    // "Mounted stdlib presents as a read-only library node"): a still-
    // mounted path has no host baseline to diff against, so egressing it
    // here would persist the library's content into the host provider
    // (`InMemoryFileProvider.onFileChanged`) and record a false "modified"
    // change — forking the mount into the user's project with no actual
    // edit having been legitimately applied. The legitimate shadow path
    // (a real file replacing a mount) calls `session.updateFile` first,
    // which un-mounts the id before this method is ever reached for it.
    //
    // Feature-detected: `session` is a public injection seam
    // (`ProjectSessionOptions.session`) and pre-#2306 stubs/handles have
    // no `isReadOnly` — absent means "nothing is read-only", which is
    // exactly their world (only the real wasm handle mounts a stdlib).
    if (this.sessionIsReadOnly(path)) return;
    const end = perfSpan("project.notifyFileChanged");
    const source = this.session.getFileSource(path);
    if (source !== null) {
      this.provider.onFileChanged?.(path, source);
    }
    this.changes.record(path, "modified");
    end(source?.length);
    // `brink.toml` edited in the studio (issue #2324) — CM6 edits (this is
    // the direct caller) and every bulk-edit path (through {@link applyEdit},
    // which calls this) both land here. The session's content for `path` is
    // already live by this point, so discovery picks up the new text.
    if (isProjectConfigPath(path)) this.applyProjectConfig();
  }

  /**
   * The shared apply-edits helper (issue #137): rewrite a file's session
   * content AND report it. Bulk edit paths (binder structural ops, search
   * replace, binder undo) MUST use this instead of raw `updateFile` so the
   * provider write-back and the host egress callback always see them.
   *
   * Session-level read-only enforcement (issue #2306, ruled 2026-08-06
   * "Mounted stdlib presents as a read-only library node"): refuses (no
   * write, no notify) when `path` currently resolves to a mounted stdlib
   * copy — the by-id route named in that ruling (project-wide search/
   * replace, or any future bulk caller not gated by `listFiles`) must not
   * be able to silently fork the library into the project. Returns whether
   * the edit actually applied, so a caller can surface the refusal instead
   * of assuming success.
   *
   * Deliberately NOT applied to `initialize()`/`addFile()`/the external-
   * change handler above, which call `session.updateFile` directly: those
   * are the host seeding real project content, including the legal case of
   * a real file deliberately shadowing a mounted stdlib key (see
   * `EditorSession::new`'s doc in `crates/brink-web/src/editor/mod.rs`) —
   * that must keep winning by construction-time ordering, not be rejected
   * because the id is still (momentarily) mounted at call time.
   */
  applyEdit(path: string, newSource: string): boolean {
    if (this.sessionIsReadOnly(path)) return false;
    this.session.updateFile(path, newSource);
    // `notifyFileChanged` re-applies `brink.toml` for us — see its own
    // comment. Do NOT add a second `applyProjectConfig()` call here: it
    // applies the config twice per edit, which
    // `project-config-application.test.ts` catches by counting warning
    // batches.
    this.notifyFileChanged(path);
    return true;
  }

  // ── Host egress (issue #154) ─────────────────────────────────────

  /** Deliver pending change notifications to the host now (save commands,
   *  unmount) instead of waiting for the debounce. */
  flushFileChanges(): FileChange[] {
    return this.changes.flush();
  }

  /** Re-baseline `paths` to their current content (explicit save).
   *
   *  ⚠ Callers must read the content that CONFIRMS what the write persisted
   *  and call this in ONE synchronous step — no `await` between them, or an
   *  edit landing in that window is retired without ever having been
   *  written (docs/embedder-api.md "Dirty state", "Confirm and retire in
   *  ONE synchronous step"; pinned for every save path by
   *  packages/brink-studio/src/__tests__/save-retire-invariant.test.ts). */
  markFilesSaved(paths: Iterable<string>): void {
    this.changes.markSaved(paths);
  }

  /** Re-baseline every session file (file.saveAll). Excludes mounted stdlib
   *  files (issue #2306/#2343): the Library section has no save affordance
   *  and a mounted path never gets a dirty baseline in the first place
   *  (`notifyFileChanged`/`applyEdit` refuse it), but `listFiles()` now
   *  lists it alongside real files (#2343's flag flip) — filtering here
   *  keeps this method's own contract ("re-baseline every session file")
   *  from silently growing to include files that were never dirty.
   *
   *  ⚠ Unconditional re-baseline — no confirming read at all, so it is MORE
   *  dangerous than `markFilesSaved` if a future caller ever reaches it
   *  after an `await`. Only safe today because its one caller
   *  (`file-commands.ts`'s no-host-save branch) is fully synchronous — there
   *  is no write to await, so nothing can move on first. A new async save
   *  path must not call this directly; it needs the same confirm-then-retire
   *  discipline as `markFilesSaved` (docs/embedder-api.md "Dirty state",
   *  "Confirm and retire in ONE synchronous step"; pinned for every save
   *  path by
   *  packages/brink-studio/src/__tests__/save-retire-invariant.test.ts). */
  markAllSaved(): void {
    this.changes.markSaved(
      this.session
        .listFiles()
        .filter((f) => !f.mounted)
        .map((f) => f.path),
    );
  }

  /** Snapshot of every session file's current content, by path (sorted). */
  getFiles(): Record<string, string> {
    const files: Record<string, string> = {};
    const paths = this.session
      .listFiles()
      .map((f) => f.path)
      .sort();
    for (const path of paths) {
      const source = this.session.getFileSource(path);
      if (source !== null) files[path] = source;
    }
    return files;
  }

  /** Paths whose content diverges from the last-saved/notified baseline. */
  dirtyPaths(): string[] {
    return this.changes.dirtyPaths();
  }

  /** Paths whose dirty buffer collided with an external change and was kept,
   *  not yet reconciled (issue #320). */
  conflictedPaths(): string[] {
    return this.changes.conflictedPaths();
  }

  /** Whether `path` has a kept-but-unreconciled external conflict (#320). */
  hasConflict(path: string): boolean {
    return this.changes.isConflicted(path);
  }

  /** Whether `path` was deleted externally while a kept editor buffer for it
   *  survives, not yet recreated by a save or an external re-creation (issue
   *  #2371, "External deletion of an open file: keep the view, mark
   *  orphaned"). */
  isOrphaned(path: string): boolean {
    return this.changes.isOrphaned(path);
  }

  /** Sorted paths flagged orphaned (issue #2371) — for tab badging. */
  orphanedPaths(): string[] {
    return this.changes.orphanedPaths();
  }

  /**
   * Recreate `path` in the wasm session from a kept editor buffer after an
   * external deletion (issue #2371) — `DocumentSessions.markOrphaned`'s only
   * call site, and the point at which a kept buffer is first confirmed to
   * survive. Unlike {@link applyEdit}, this deliberately does NOT notify the
   * provider yet, and does NOT go through `record()`/`notifyFileChanged`:
   * `changes.noteOrphanRecreated` flags the path orphaned (no earlier call
   * site knows a buffer exists) and marks it dirty (no baseline — the
   * existing `FileChangeHub` rule) so the badge, dirty indicator, and IDE
   * queries are all correct immediately, WITHOUT enqueuing a pending change
   * or arming the flush debounce — a debounced delivery here would
   * "save" the recreated buffer on a timer under a write-through contract,
   * not on an actual ⌘S. `provider.onFileChanged` — the step that actually
   * stages/persists content, depending on the provider — fires only from the
   * next real `notifyFileChanged`, which a save always triggers
   * (`DocumentSessions.flushSlot` calls it unconditionally, whether or not
   * the buffer was edited since the deletion). That keeps "⌘S recreates the
   * file" literally true even for a provider whose `onFileChanged` IS its
   * persistence (`InMemoryFileProvider`'s playground contract) — recreating
   * eagerly here would resurrect the file the moment the deletion is
   * detected, before any save.
   */
  recreateOrphaned(path: string, content: string): void {
    if (this.sessionIsReadOnly(path)) return;
    this.session.updateFile(path, content);
    this.changes.noteOrphanRecreated(path);
  }

  /**
   * Resolve an external conflict (issue #320, Track V) by taking the host's
   * on-disk content: overwrite the session buffer with `disk`, re-baseline to
   * it, and clear the conflict flag (the path goes clean). This is the
   * "Use disk" merge action — the studio's dirty edit is discarded in favor
   * of what landed on disk.
   */
  resolveConflictUseDisk(path: string, disk: string): void {
    this.session.updateFile(path, disk);
    // applyExternal re-baselines to `disk` and clears the conflict flag.
    this.changes.applyExternal(path, disk);
  }

  /**
   * Resolve an external conflict (issue #320, Track V) by KEEPING the studio
   * buffer: clear the conflict flag without touching the buffer or baseline.
   * The path stays dirty — the kept edit still diverges from disk and is
   * re-delivered on the next flush/save. This is the "Keep mine" merge action.
   */
  resolveConflictKeepMine(path: string): void {
    this.changes.clearConflict(path);
  }

  /**
   * Resolve an external conflict (issue #320, Track V) with a hand-merged
   * result: write `merged` through the shared apply-edits seam (so the host
   * egress + provider write-back see it) and clear the conflict flag. The
   * merged text becomes the new dirty buffer over the still-unchanged
   * baseline, so the user can save it normally.
   */
  resolveConflictMerged(path: string, merged: string): void {
    this.applyEdit(path, merged);
    this.changes.clearConflict(path);
  }

  /** Observe the dirty-file count (drives the public-state summary). */
  setDirtyListener(listener: ((dirtyCount: number) => void) | undefined): void {
    this.changes.setDirtyListener(listener);
  }

  /**
   * Re-resolve INCLUDEs across all loaded files, loading missing files from
   * the provider — the next compile picks up newly discovered files.
   */
  async refreshIncludes(): Promise<void> {
    const end = perfSpan("project.refreshIncludes");
    await this.resolveIncludes();
    end();
  }

  /** Request a canonical save via the provider (optionally narrowed to
   *  `paths` — see {@link FileProvider.requestSave}). Rejections propagate:
   *  the save commands rely on that to keep files dirty when the host's
   *  write fails. */
  async save(paths?: string[]): Promise<void> {
    await this.provider.requestSave?.(paths);
  }

  /** Whether the provider implements a host-side canonical save. The save
   *  commands branch on this: with a host save they await it and only
   *  re-baseline on success; without one (the standalone playground) the
   *  flush-and-re-baseline path runs synchronously as it always has. */
  hasHostSave(): boolean {
    return this.provider.requestSave !== undefined;
  }

  /**
   * Read `path` straight from the provider, bypassing session state
   * entirely — the provider's own account of what is actually persisted
   * (disk, for a host-save provider). Existing {@link FileProvider.readFile}
   * plumbing; this just exposes it past `ProjectSession`.
   *
   * The save commands use this to confirm what a host write actually wrote
   * when a path's content no longer matches the snapshot taken before the
   * save started (issue #2435): with `requestSave` calls serialized
   * (`TauriFileProvider`, #2403), a write queued behind another in-flight
   * one can legitimately pick up a later edit by the time it actually runs
   * and persist content newer than that snapshot — a case this lets the
   * caller tell apart from a genuine mid-write divergence (issue #2426)
   * without weakening that guard: a divergence still fails this check,
   * since disk keeps the pre-race content the write actually persisted.
   * Rejects like {@link FileProvider.readFile} itself (e.g. a vanished path).
   *
   * This confirmation is only meaningful if the underlying
   * {@link FileProvider.readFile} reports PERSISTED content, never content a
   * `requestSave` merely staged — a provider whose `readFile` mirrors
   * in-flight edits (see that method's doc) makes every call here vacuously
   * match, silently turning the #2426 guard into a no-op.
   */
  async readProviderFile(path: string): Promise<string> {
    return this.provider.readFile(path);
  }

  /** Ask the provider for a file not yet in the session; loads it if found. */
  async requestFile(path: string): Promise<string | null> {
    const existing = this.session.getFileSource(path);
    if (existing !== null) return existing;
    const content = await this.provider.requestFile(path);
    this.assertLive();
    if (content !== null) {
      this.session.updateFile(path, content);
      this.changes.setBaseline(path, content);
    }
    return content;
  }

  /** Tear down. Detaches the external-change listener before freeing the
   *  session so a late callback can't touch freed wasm memory. Pending
   *  change notifications must be flushed by the caller BEFORE destroy
   *  (mountStudio's unmount does) — destroy only cancels.
   *
   *  Also aborts every gated call still waiting on its {@link
   *  deferGatedCall} yield (issue #2794): each pending idle handle is
   *  cancelled (so the browser/timer callback never fires against this
   *  now-freed session) and its caller's `await` is rejected (so the code
   *  that would call `this.session.*` after the yield never runs) — before
   *  `this.session.free()` below. */
  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.changes.dispose();
    this.unsubscribeExternal?.();
    this.unsubscribeExternal = undefined;
    for (const [handle, reject] of this.pendingIdleWork) {
      cancelIdleWork(handle);
      reject(new Error("ProjectSession destroyed while a gated call was deferred"));
    }
    this.pendingIdleWork.clear();
    // Rejects every in-flight client query (as cancelled) BEFORE the wasm
    // handle is freed — same freed-wasm discipline as the idle-work sweep
    // above: an async landing must never dispatch into a freed session.
    this.client.close();
    this.workerClient?.close();
    this.workerClient = null;
    this.session.free();
  }

  /** Resolve INCLUDEs across all loaded files, loading missing files from the provider. */
  private async resolveIncludes(): Promise<void> {
    const visited = new Set<string>();
    const queue = this.session.listFiles().map((f) => f.path);

    while (queue.length > 0) {
      const current = queue.shift()!;
      if (visited.has(current)) continue;
      visited.add(current);

      const includes = this.session.getFileIncludes(current);
      for (const inc of includes) {
        if (inc.loaded) {
          // Already in session — but still need to check its includes
          if (!visited.has(inc.resolved)) {
            queue.push(inc.resolved);
          }
          continue;
        }

        const content = await this.provider.requestFile(inc.resolved);
        this.assertLive();
        if (content !== null) {
          this.session.updateFile(inc.resolved, content);
          // Provider-supplied content = host-synced = clean baseline.
          this.changes.setBaseline(inc.resolved, content);
          queue.push(inc.resolved);
        }
      }
    }
  }
}
