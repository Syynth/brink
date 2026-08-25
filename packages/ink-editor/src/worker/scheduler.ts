/**
 * Admission-control scheduler for a single-threaded session host
 * (docs/editor-worker-spec.md §6).
 *
 * This is the *policy* every host must observe (spec §5.4: policy, not
 * shared code — a future native host may satisfy the same observable
 * contract with real threads): all queued mutations run before any
 * query; interactive queries run before background pulls; background
 * queries are dropped when superseded (same `coalesceKey` queued behind
 * them) or stale (their `docVersion` predates the latest queued
 * mutation for their doc). Interactive queries and mutations are never
 * dropped.
 *
 * The scheduler never swallows work silently: a dropped query is
 * *returned* as a drop action so the host can answer it with a
 * `dropped:` error, keeping the client's promise from leaking.
 */

import type { DocumentId, SessionRequest } from "@brink/wasm-types";

type QueryRequest = Extract<SessionRequest, { kind: "query" }>;
type MutationRequest = Extract<
  SessionRequest,
  { kind: "edit" | "push" | "config" | "files" }
>;

export type SchedulerAction =
  | { kind: "run"; request: MutationRequest | QueryRequest }
  | { kind: "drop"; request: QueryRequest; reason: "superseded" | "stale" };

export class AdmissionScheduler {
  private readonly mutations: MutationRequest[] = [];
  private readonly interactive: QueryRequest[] = [];
  private readonly background: QueryRequest[] = [];
  /** Latest doc version named by an enqueued mutation, per doc — the
   *  staleness ruler for queued background queries. */
  private readonly docVersions = new Map<DocumentId, number>();

  enqueue(request: SessionRequest): void {
    switch (request.kind) {
      case "edit":
      case "push":
        this.docVersions.set(request.doc, request.docVersion);
        this.mutations.push(request);
        break;
      case "config":
      case "files":
        this.mutations.push(request);
        break;
      case "query":
        (request.priority === "interactive" ? this.interactive : this.background).push(
          request,
        );
        break;
      case "cancel": {
        // Best-effort: removes queued queries only (spec §6). The host
        // answers a cancelled query nowhere — the client rejects its own
        // promise on cancel, so removal here just saves the work.
        remove(this.interactive, request.id);
        remove(this.background, request.id);
        break;
      }
    }
  }

  /** Latest queued/applied mutation version for a doc (host uses this to
   *  stamp query results). */
  latestDocVersion(doc: DocumentId): number | undefined {
    return this.docVersions.get(doc);
  }

  /**
   * Next thing for the host to do, or `null` when idle. Drops are
   * decided at *dequeue* time — the last moment before execution — so a
   * query enqueued fresh can still be superseded or go stale while it
   * waits behind other work.
   */
  nextAction(): SchedulerAction | null {
    const mutation = this.mutations.shift();
    if (mutation) return { kind: "run", request: mutation };
    const interactive = this.interactive.shift();
    if (interactive) return { kind: "run", request: interactive };
    const background = this.background.shift();
    if (background === undefined) return null;
    if (
      background.coalesceKey !== undefined &&
      this.background.some((later) => later.coalesceKey === background.coalesceKey)
    ) {
      return { kind: "drop", request: background, reason: "superseded" };
    }
    if (background.doc !== undefined && background.docVersion !== undefined) {
      const current = this.docVersions.get(background.doc);
      if (current !== undefined && background.docVersion < current) {
        return { kind: "drop", request: background, reason: "stale" };
      }
    }
    return { kind: "run", request: background };
  }
}

function remove(queue: QueryRequest[], id: number): void {
  const index = queue.findIndex((q) => q.id === id);
  if (index !== -1) queue.splice(index, 1);
}
