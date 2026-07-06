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

import type { Choice } from "@brink/wasm-types";

/** The shared-flow surface `FlowSessionProvider` needs from its host handle —
 * satisfied by both `StoryRunnerHandle` and `StorySessionHandle`. */
export interface FlowHost {
  programModel(): unknown;
  programInkt(): string;
  continueFlow(name: string): { type: string; text: string; tags: string[]; choices?: Choice[] };
  chooseFlow(name: string, index: number): void;
  destroyFlow(name: string): void;
  flowDebugSnapshot(name: string): unknown;
}

import {
  statusOfLine,
  type ProviderCallbacks,
  type SessionCapability,
  type SessionProvider,
  type SessionSnapshot,
  type SessionStatus,
} from "./types.js";

/** A flow drives its own choices/continue; the shared story owns start/stop. */
const FLOW_CAPABILITIES: ReadonlySet<SessionCapability> = new Set(["choose", "continue"]);

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

  private readonly runner: FlowHost;
  private readonly flowName: string;
  private callbacks: ProviderCallbacks;
  private readonly listeners = new Set<(s: SessionSnapshot) => void>();

  private status: SessionStatus = "running";
  private transcript: string[] = [];
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

  choose(index: number): void {
    if (this.disposed) return;
    const choiceText = this.choices.find((c) => c.index === index)?.text;
    try {
      this.runner.chooseFlow(this.flowName, index);
    } catch (e) {
      this.fail("Choose error", e);
      return;
    }
    if (choiceText) this.transcript = [...this.transcript, `> ${choiceText}`];
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
      const line = this.runner.continueFlow(this.flowName);
      const text = line.text.replace(/\n$/, "");
      if (text) this.transcript = [...this.transcript, text];
      this.choices = line.type === "choices" ? (line.choices ?? []) : [];
      this.status = statusOfLine(line.type);
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
    this.transcript = [...this.transcript, `${label}: ${msg}`];
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
