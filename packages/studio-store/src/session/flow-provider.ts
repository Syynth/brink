/**
 * Flow session provider — a shared-context flow as a {@link SessionProvider}
 * (#200, docs/multi-session-spec.md §7).
 *
 * A "+ New flow" session drives a named `FlowInstance` spawned in the **primary
 * session's** VM, so it **shares** that story's globals / visit counts / rng
 * (true ink concurrent-flow semantics) while keeping its own call stack +
 * output. It does **not** own the underlying handle (the primary
 * `LocalSessionProvider` does); disposing it destroys just the flow.
 *
 * Contrast with {@link LocalSessionProvider}, which is an *independent* session
 * (isolated globals) — the "+ New session" case.
 *
 * Accepts either handle that exposes the shared-flow surface —
 * `StoryRunnerHandle` (used by nothing here anymore, kept structurally
 * compatible) or `StorySessionHandle` (#388: the primary is now
 * session-backed). Duck-typed rather than importing a union so a future
 * third handle only needs to match this shape, not be added to a type union
 * here.
 */

import type { Choice, SourceLocation } from "@brink/wasm-types";

/** The shared-flow surface `FlowSessionProvider` needs from its host handle —
 * satisfied by both `StoryRunnerHandle` and `StorySessionHandle`. */
/** One revealed line as the flow verbs return it. */
export interface FlowLine {
  type: string;
  text: string;
  tags: string[];
  choices?: Choice[];
  /** Transcript provenance (W7/#3300) — the flow verbs' wire lines carry
   * it like every other delivered line; optional for older hosts. */
  source?: SourceLocation;
}

export interface FlowHost {
  programModel(): unknown;
  programInkt(): string;
  continueFlow(name: string): FlowLine;
  /** Run-to-pause counterpart of `continueFlow` (#3011); last element is
   * terminal. Both `StoryRunnerHandle` and `StorySessionHandle` provide it. */
  continueFlowMaximally(name: string): FlowLine[];
  chooseFlow(name: string, index: number): void;
  destroyFlow(name: string): void;
  flowDebugSnapshot(name: string): unknown;
}

import {
  statusOfLine,
  transcriptLine,
  transcriptNotice,
  type TranscriptLine,
  type ProviderCallbacks,
  type SessionCapability,
  type SessionProvider,
  type SessionSnapshot,
  type SessionStatus,
} from "./types.js";

/** A flow drives its own choices/continue; the shared story owns start/stop. */
const FLOW_CAPABILITIES: ReadonlySet<SessionCapability> = new Set([
  "choose",
  "continue",
  // Flows honour the reveal-mode toggle too (#3011): the runner exposes
  // `continueFlowMaximally` alongside the single-line `continueFlow`, so there
  // is nothing special about a flow that would justify withholding it.
  "auto",
]);

const NOOP_CALLBACKS: ProviderCallbacks = {
  notify() {
    /* no-op until bound */
  },
  appendOutput() {
    /* no-op until bound */
  },
};

export class FlowSessionProvider implements SessionProvider {
  readonly kind = "local" as const;
  readonly capabilities = FLOW_CAPABILITIES;

  /** Reveal mode (#3011); `false` reveals one line at a time. */
  private auto = false;

  private readonly runner: FlowHost;
  private readonly flowName: string;
  private callbacks: ProviderCallbacks;
  private readonly listeners = new Set<(s: SessionSnapshot) => void>();

  private status: SessionStatus = "running";
  private transcript: TranscriptLine[] = [];
  private choices: Choice[] = [];
  private debugState: SessionSnapshot["debugState"] = null;
  // Program identity is the shared host's — same program as the primary.
  private readonly programModel: SessionSnapshot["programModel"];
  private readonly programInkt: string | null;
  private readonly programChecksum: string | null;
  private disposed = false;

  constructor(
    runner: FlowHost,
    flowName: string,
    callbacks: ProviderCallbacks = NOOP_CALLBACKS,
  ) {
    this.runner = runner;
    this.flowName = flowName;
    this.callbacks = callbacks;
    this.programModel = this.capture(
      () => runner.programModel() as SessionSnapshot["programModel"],
    );
    this.programInkt = this.capture(() => runner.programInkt());
    this.programChecksum = this.programModel?.checksum ?? null;
  }

  setCallbacks(callbacks: ProviderCallbacks): void {
    this.callbacks = callbacks;
  }

  getSnapshot(): SessionSnapshot {
    return {
      status: this.status,
      transcript: this.transcript,
      choices: this.choices,
      debugState: this.debugState,
      programChecksum: this.programChecksum,
      programModel: this.programModel,
      programInkt: this.programInkt,
      // No hot-reload on a flow view either — the HOST session reloads.
      reloadedAt: null,
      // No debug surface on the flow provider (no `debug` capability):
      // never paused, never a debug outcome.
      paused: false,
      debugOutcome: null,
      auto: this.auto,
    };
  }

  subscribe(listener: (snapshot: SessionSnapshot) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /** Reveal the flow's first line (the flow was already spawned). */
  start(): void {
    this.reveal();
  }

  continue(): void {
    this.reveal();
  }

  /** Set the reveal mode (#3011). Takes effect on the next reveal. */
  setAuto(auto: boolean): void {
    if (this.disposed || this.auto === auto) return;
    this.auto = auto;
    this.emit();
  }

  choose(index: number): void {
    if (this.disposed) return;
    const choiceText = this.choices.find((c) => c.index === index)?.text;
    try {
      this.runner.chooseFlow(this.flowName, index);
    } catch (e) {
      this.fail("Choose error", e);
      return;
    }
    if (choiceText)
      this.transcript = [...this.transcript, { text: `> ${choiceText}`, kind: "marker" as const, tags: [] }];
    this.choices = [];
    this.reveal();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    try {
      this.runner.destroyFlow(this.flowName);
    } catch {
      // The runner may already be gone (primary reloaded) — nothing to free.
    }
    this.listeners.clear();
  }

  // ── Internals ─────────────────────────────────────────────────────

  private reveal(): void {
    if (this.disposed) return;
    try {
      // One line by default, the whole run to the next pause under `auto`
      // (#3011). `continueFlowMaximally` is the flow-scoped counterpart of the
      // primary session's `continueToPause`, so both providers offer the same
      // choice rather than the flow being arbitrarily stuck single-stepping.
      const lines = this.auto
        ? this.runner.continueFlowMaximally(this.flowName)
        : [this.runner.continueFlow(this.flowName)];
      for (const line of lines) {
        const text = line.text.replace(/\n$/, "");
        if (text) this.transcript = [...this.transcript, transcriptLine(text, line.tags, line.source)];
      }
      // An empty maximal result would leave status untouched rather than
      // crashing on `undefined` — the single-line branch always has one.
      const last = lines.at(-1);
      this.choices = last?.type === "choices" ? (last.choices ?? []) : [];
      this.status = last ? statusOfLine(last.type) : this.status;
    } catch (e) {
      this.fail("Runtime error", e);
      return;
    }
    this.refreshDebug();
    this.emit();
  }

  private refreshDebug(): void {
    try {
      this.debugState = this.runner.flowDebugSnapshot(
        this.flowName,
      ) as SessionSnapshot["debugState"];
    } catch {
      this.debugState = null;
    }
  }

  private fail(label: string, e: unknown): void {
    const msg = e instanceof Error ? e.message : String(e);
    this.status = "error";
    this.transcript = [...this.transcript, transcriptNotice(`${label}: ${msg}`)];
    this.choices = [];
    this.callbacks.appendOutput("story", `${label}: ${msg}`);
    this.emit();
  }

  private capture<T>(fn: () => T): T | null {
    try {
      return fn();
    } catch {
      return null;
    }
  }

  private emit(): void {
    const snap = this.getSnapshot();
    for (const listener of this.listeners) listener(snap);
  }
}
