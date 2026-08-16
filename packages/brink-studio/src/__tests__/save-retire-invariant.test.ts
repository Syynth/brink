/**
 * The confirm→retire invariant, pinned across EVERY save path (issue #2455,
 * docs/embedder-api.md "Dirty state").
 *
 * The rule: the read that CONFIRMS what a write persisted and the
 * `markFilesSaved` call that RETIRES those paths must be one synchronous
 * step — no `await` between them. A snapshot carried across an `await` is
 * stale by the time it is acted on: an edit landing in that window was
 * never written, so retiring it discards the author's work with no warning
 * at all. That is strictly worse than the false-positive warning the
 * disk-confirmation check (#2435) exists to avoid.
 *
 * The rule was learned three times — `OverlayPersistence.saveDirty`
 * (#2417), `file.save`/`file.saveAll` (#2426), and again inside PR #2447,
 * whose first draft re-read a pre-`await` snapshot after the
 * `readProviderFile` round trip and was caught only by review. Each site
 * has its own race test for its own specific window; none of them states
 * the general rule, so a NEW save path, or a new `await` slipped into an
 * existing one, would reintroduce the loss silently.
 *
 * This suite is that general pin. It is deliberately implementation-blind:
 * it never names which `await` is the dangerous one. Instead it drives each
 * save path through a shared fake world that instruments the two ends of
 * the window — every `getFiles()` read and every `markFilesSaved()` call —
 * and then:
 *
 *  1. **Calibrates.** Runs the path undisturbed, counts its `getFiles()`
 *     calls, and asserts it really did retire the paths it should (a
 *     scenario that retires nothing would make the sweep vacuous).
 *  2. **Sweeps.** Re-runs it once per read, landing ONE adversarial edit as
 *     a microtask queued the instant that read returns. If the path retires
 *     synchronously after its confirming read the edit lands harmlessly
 *     afterwards; if ANY `await` sits between them, the edit lands inside
 *     the window instead.
 *  3. **Asserts the safety property**, checked inside `markFilesSaved`
 *     itself: a path may only be retired while its current buffer content
 *     is what the provider actually has on disk. The adversarial edit is
 *     never persisted, so retiring it is always a violation — whichever
 *     `await` let it in, in whichever save path.
 *
 * The sweep also asserts that at least one injection was actually declined
 * per path: if every adversarial edit landed harmlessly outside a decision
 * window, the run would prove nothing about the guards.
 *
 * This file is itself the incident that motivated #2516: an earlier draft
 * of `save-path-enrolment.test.ts` imported `SAVE_PATHS` straight from this
 * file, re-registering every test above under the enrolment suite's name.
 * `save-paths.ts` is the plain-module fix; the recurrence guard is
 * `packages/brink-studio/src/__tests__/no-test-file-imports.test.ts`.
 */

import { describe, it, expect } from "vitest";
import { OverlayPersistence, type CanonicalStore, type PersistenceSession } from "@brink-lang/editor";
import { CommandRegistry, type NotificationInput } from "@brink/studio-shell";
import type { DocumentSessions, ProjectSession } from "@brink/studio-store";
import {
  registerFileCommands,
  FILE_SAVE_COMMAND_ID,
  FILE_SAVE_ALL_COMMAND_ID,
} from "../file-commands.js";
import { SAVE_PATH_IDS, type SavePathId } from "./save-paths.js";

const TARGET = "a.ink";
const OTHER = "b.ink";
/** Content no write ever persists — retiring it is always data loss. */
const ADVERSARIAL = "// edit that landed mid-confirmation\n";

/** One adversarial edit, queued the moment a chosen `getFiles()` returns. */
interface Adversary {
  /** 1-based index of the `getFiles()` call to queue the edit behind. */
  afterRead: number;
  path: string;
}

/**
 * A fake session + provider pair sharing one "disk", instrumented at both
 * ends of the confirm→retire window. Structural, so every save path can be
 * driven through it without the wasm session or a real provider.
 */
class SaveWorld {
  /** Editor buffers — what the session holds right now. */
  private readonly buffer = new Map<string, string>();
  /** What the provider has actually PERSISTED. */
  private readonly disk = new Map<string, string>();
  /** Last re-baselined content; dirty means buffer !== baseline. */
  private readonly baseline = new Map<string, string>();
  /**
   * Edits an in-flight write legitimately catches up to and persists: with
   * `requestSave` calls serialized (`TauriFileProvider`, #2403) a write
   * queued behind another runs late enough to pick up a later edit and
   * write content newer than the caller's pre-save snapshot (#2435). Needed
   * so the disk-confirmation branch is actually REACHED — a scenario that
   * never gets past the trivial "content never moved" check would sweep
   * only half of each command.
   */
  private readonly catchUp: Array<[string, string]> = [];
  private readonly adversary: Adversary | null;

  /** `getFiles()` calls so far — the sweep's coordinate system. */
  reads = 0;
  /** Whether the adversarial edit actually landed (guards a silent no-op). */
  landed = false;
  /** Paths passed to `markFilesSaved`/`markAllSaved`, in order. */
  readonly retired: string[] = [];
  /** Retirements of content the provider never persisted. */
  readonly violations: string[] = [];
  readonly notifications: NotificationInput[] = [];

  constructor(files: Record<string, string>, adversary: Adversary | null) {
    for (const [path, content] of Object.entries(files)) {
      this.buffer.set(path, content);
      this.disk.set(path, content);
      this.baseline.set(path, content);
    }
    this.adversary = adversary;
  }

  // ── Scenario setup ───────────────────────────────────────────────

  /** An unsaved editor edit: buffer moves, disk does not. */
  edit(path: string, content: string): void {
    this.buffer.set(path, content);
  }

  /** Stage a legitimate mid-flight catch-up (see {@link catchUp}). */
  persistDuringSave(path: string, content: string): void {
    this.catchUp.push([path, content]);
  }

  // ── Instrumented session surface ─────────────────────────────────

  /** Sorted content snapshot — `ProjectSession.getFiles`'s shape. */
  getFiles(): Record<string, string> {
    this.reads += 1;
    if (this.adversary !== null && this.adversary.afterRead === this.reads) {
      // A microtask, not a synchronous edit: it lands at the caller's very
      // next `await` — inside the window if one exists there, harmlessly
      // after the retirement if it does not.
      queueMicrotask(() => {
        this.buffer.set(this.adversary?.path ?? TARGET, ADVERSARIAL);
        this.landed = true;
      });
    }
    const snapshot: Record<string, string> = {};
    for (const path of [...this.buffer.keys()].sort()) {
      snapshot[path] = this.buffer.get(path) ?? "";
    }
    return snapshot;
  }

  dirtyPaths(): string[] {
    return [...this.buffer.keys()]
      .filter((path) => this.buffer.get(path) !== this.baseline.get(path))
      .sort();
  }

  /** The retire end of the window — where the safety property is checked. */
  markFilesSaved(paths: Iterable<string>): void {
    for (const path of paths) {
      this.retired.push(path);
      const content = this.buffer.get(path);
      if (content !== this.disk.get(path)) {
        this.violations.push(
          `${path} retired holding ${JSON.stringify(content)}, but the provider persisted ${JSON.stringify(this.disk.get(path))}`,
        );
      }
      if (content !== undefined) this.baseline.set(path, content);
    }
  }

  markAllSaved(): void {
    this.markFilesSaved([...this.buffer.keys()].sort());
  }

  // ── Instrumented provider surface ────────────────────────────────

  /** One async hop — a host IPC round trip, in microtask terms. */
  private async hop(): Promise<void> {
    await Promise.resolve();
  }

  /**
   * `ProjectSession.save` — the host's canonical write. What it persists is
   * snapshotted at CALL time (like `TauriFileProvider.writeStaged`'s own
   * `pending` snapshot before `invoke("write_file")`), so an edit landing
   * while it is in flight is never retroactively written.
   */
  async save(paths?: string[]): Promise<void> {
    const wanted = paths ?? this.dirtyPaths();
    const staged = wanted.map((path): [string, string] => [path, this.buffer.get(path) ?? ""]);
    await this.hop();
    for (const [path, content] of staged) this.disk.set(path, content);
    for (const [path, content] of this.catchUp) {
      this.buffer.set(path, content);
      this.disk.set(path, content);
    }
    this.catchUp.length = 0;
  }

  /** `ProjectSession.readProviderFile` — the disk-confirmation read. */
  async readProviderFile(path: string): Promise<string> {
    await this.hop();
    const content = this.disk.get(path);
    if (content === undefined) throw new Error(`no such file: ${path}`);
    return content;
  }

  // ── Adapters for the save paths under test ───────────────────────

  persistenceSession(): PersistenceSession {
    return {
      dirtyPaths: () => this.dirtyPaths(),
      getFiles: () => this.getFiles(),
      markFilesSaved: (paths) => {
        this.markFilesSaved(paths);
      },
    };
  }

  canonicalStore(): CanonicalStore {
    return {
      write: async (path, content) => {
        await this.hop();
        this.disk.set(path, content);
      },
    };
  }

  /** The slice of `ProjectSession` the save commands actually call. */
  projectSession(): ProjectSession {
    return {
      flushFileChanges: () => [],
      dirtyPaths: () => this.dirtyPaths(),
      getFiles: () => this.getFiles(),
      hasHostSave: () => true,
      save: (paths?: string[]) => this.save(paths),
      readProviderFile: (path: string) => this.readProviderFile(path),
      markFilesSaved: (paths: Iterable<string>) => {
        this.markFilesSaved(paths);
      },
      markAllSaved: () => {
        this.markAllSaved();
      },
    } as unknown as ProjectSession;
  }

  /** The slice of `DocumentSessions` the save commands actually call. */
  documentSessions(focused: string | null): DocumentSessions {
    return {
      flushFocused: () => focused,
      flushAll: () => undefined,
    } as unknown as DocumentSessions;
  }

  notify(n: NotificationInput): void {
    this.notifications.push(n);
  }
}

/** Drain the promise chain a save command fires and forgets. */
async function settle(): Promise<void> {
  for (let i = 0; i < 20; i += 1) await Promise.resolve();
}

function dispatchCommand(world: SaveWorld, id: string, focused: string | null): void {
  const commands = new CommandRegistry();
  const dispose = registerFileCommands(commands, {
    project: world.projectSession(),
    documents: world.documentSessions(focused),
    notify: (n) => {
      world.notify(n);
    },
  });
  commands.dispatch(id);
  dispose();
}

/**
 * One save path under test. `scenario` sets up a run that legitimately
 * retires `retires` — the sweep then attacks that run read by read.
 */
interface SavePath {
  /**
   * Typed against `save-paths.ts` rather than `string`, so a driver naming an
   * id the enrolment guard's registry doesn't have is a typecheck failure
   * before it is a test failure (#2480).
   */
  id: SavePathId;
  files: Record<string, string>;
  scenario: (world: SaveWorld) => void;
  run: (world: SaveWorld) => Promise<void>;
  /** Paths the undisturbed run must retire, sorted. */
  retires: string[];
}

const SAVE_PATHS: SavePath[] = [
  {
    // packages/ink-editor/src/persistence.ts — saveDirty (#2417).
    id: "OverlayPersistence.saveAll",
    files: { [TARGET]: "v0", [OTHER]: "v0" },
    scenario: (world) => {
      world.edit(TARGET, "a1");
      world.edit(OTHER, "b1");
    },
    run: async (world) => {
      const persistence = new OverlayPersistence({
        session: world.persistenceSession(),
        canonical: world.canonicalStore(),
      });
      await persistence.saveAll();
      persistence.dispose();
    },
    retires: [TARGET, OTHER],
  },
  {
    // Same site, entered through the subset door the single-file command
    // uses — a distinct caller of the same confirm→retire step.
    id: "OverlayPersistence.save",
    files: { [TARGET]: "v0", [OTHER]: "v0" },
    scenario: (world) => {
      world.edit(TARGET, "a1");
      world.edit(OTHER, "b1");
    },
    run: async (world) => {
      const persistence = new OverlayPersistence({
        session: world.persistenceSession(),
        canonical: world.canonicalStore(),
      });
      await persistence.save([TARGET]);
      persistence.dispose();
    },
    retires: [TARGET],
  },
  {
    // packages/brink-studio/src/file-commands.ts — file.save (#2426/#2447).
    id: "file.save",
    files: { [TARGET]: "v0" },
    scenario: (world) => {
      world.edit(TARGET, "a1");
      // The write catches up to a later edit and persists it: the command
      // reaches its disk-confirmation branch rather than stopping at the
      // trivial "content never moved" check.
      world.persistDuringSave(TARGET, "a2");
    },
    run: async (world) => {
      dispatchCommand(world, FILE_SAVE_COMMAND_ID, TARGET);
      await settle();
    },
    retires: [TARGET],
  },
  {
    // packages/brink-studio/src/file-commands.ts — file.save's OTHER retire
    // door: the trivially-settled branch (`current === before`), taken when
    // the write persists exactly what was staged with no mid-flight
    // catch-up. Every other driver forces a divergence so the command falls
    // through to the disk-confirmation branch instead — this scenario is
    // the one that actually reaches the settled door, so the sweep covers
    // both of `file.save`'s retire sites, not just one.
    id: "file.save (settled)",
    files: { [TARGET]: "v0" },
    scenario: (world) => {
      world.edit(TARGET, "a1");
    },
    run: async (world) => {
      dispatchCommand(world, FILE_SAVE_COMMAND_ID, TARGET);
      await settle();
    },
    retires: [TARGET],
  },
  {
    // packages/brink-studio/src/file-commands.ts — file.saveAll (#2426/#2447).
    id: "file.saveAll",
    files: { [TARGET]: "v0", [OTHER]: "v0" },
    scenario: (world) => {
      world.edit(TARGET, "a1");
      world.edit(OTHER, "b1");
      // `b.ink` reaches the disk-confirmation branch; `a.ink` stays in the
      // trivially-settled bucket, so the sweep covers both buckets — the
      // settled one is exactly what PR #2447's draft retired stale.
      world.persistDuringSave(OTHER, "b2");
    },
    run: async (world) => {
      dispatchCommand(world, FILE_SAVE_ALL_COMMAND_ID, TARGET);
      await settle();
    },
    retires: [TARGET, OTHER],
  },
];

describe("confirm→retire is one synchronous step (#2455)", () => {
  it("SAVE_PATHS drives exactly the ids in save-paths.ts (#2480)", () => {
    // The enrolment guard cross-checks every production call site's
    // `SAVE-PATH` marker against `SAVE_PATH_IDS`, so that registry is only
    // meaningful while it names the paths this suite actually drives. The
    // `SavePathId` type already rejects a driver naming an id the registry
    // lacks; this catches the other direction — a registry id with no driver
    // behind it, which would let a marker "enrol" a path nothing sweeps.
    const driven = SAVE_PATHS.map((path) => path.id);
    const registered = [...SAVE_PATH_IDS];
    expect(
      registered.filter((id) => !driven.includes(id)),
      "save-paths.ts lists these ids AHEAD of save-retire-invariant.test.ts: the registry " +
        "names them but no SAVE_PATHS driver sweeps them, so a SAVE-PATH marker naming one " +
        "would pass the enrolment guard while nothing tests the path. Add the driver, or " +
        "drop the id from save-paths.ts",
    ).toEqual([]);
    expect(
      driven.filter((id) => !registered.includes(id)),
      "save-retire-invariant.test.ts is AHEAD of save-paths.ts: these driven ids are missing " +
        "from the registry, so save-path-enrolment.test.ts would reject a marker naming them. " +
        "Add them to SAVE_PATH_IDS",
    ).toEqual([]);
    expect(driven).toEqual(registered); // ordering + duplicates
  });

  for (const path of SAVE_PATHS) {
    it(`${path.id} never retires an edit that lands between its confirming read and markFilesSaved`, async () => {
      // ── Calibrate: an undisturbed run, to count the windows and prove
      // the scenario actually reaches a retirement.
      const calibration = new SaveWorld(path.files, null);
      path.scenario(calibration);
      await path.run(calibration);
      expect(calibration.violations).toEqual([]);
      expect([...calibration.retired].sort()).toEqual(path.retires);
      const reads = calibration.reads;
      expect(reads).toBeGreaterThan(0);

      // ── Sweep: one adversarial edit per read, landing at whatever the
      // implementation awaits next.
      let declined = 0;
      for (let afterRead = 1; afterRead <= reads; afterRead += 1) {
        const world = new SaveWorld(path.files, { afterRead, path: TARGET });
        path.scenario(world);
        await path.run(world);
        await settle(); // let an edit queued behind the LAST read land too

        expect(world.landed, `${path.id}: no edit landed after read #${afterRead}`).toBe(true);
        expect(
          world.violations,
          `${path.id}: an edit landing after read #${afterRead} was retired without being written`,
        ).toEqual([]);
        if (!world.retired.includes(TARGET)) declined += 1;
      }

      // Non-vacuity: at least one injection must have landed inside a real
      // decision window and been declined. If none were, the sweep only
      // proved that edits landing after a save are harmless.
      expect(declined, `${path.id}: every injected edit landed outside a decision window`)
        .toBeGreaterThan(0);
    });
  }

  it("a path whose write is declined stays dirty and warns rather than going quiet", async () => {
    // The complement of the invariant: declining to retire is only safe
    // because the path stays dirty AND the author is told. A guard that
    // dropped the warning would pass the sweep above while still losing the
    // edit from the author's point of view.
    const world = new SaveWorld({ [TARGET]: "v0" }, { afterRead: 2, path: TARGET });
    world.edit(TARGET, "a1");
    world.persistDuringSave(TARGET, "a2");
    dispatchCommand(world, FILE_SAVE_COMMAND_ID, TARGET);
    await settle();

    expect(world.retired).toEqual([]);
    expect(world.dirtyPaths()).toEqual([TARGET]);
    expect(world.notifications.map((n) => n.message)).toEqual([
      `${TARGET} changed while saving — still unsaved`,
    ]);
  });
});
