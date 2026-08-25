/**
 * Async session facade (docs/editor-worker-spec.md §5.2).
 *
 * `SessionClient` is the one thing consumers talk to once migrated
 * (W2): mutations are fire-and-forget and strictly ordered; queries
 * return promises tagged with the doc version and config epoch that
 * produced them, so each consumer can apply its own staleness policy
 * (spec §5.3 — positional results map-then-land, point queries drop,
 * whole-project results supersede).
 *
 * The client owns the per-document version counter: every edit/push it
 * sends bumps the doc's version, and `stale` on a query result means
 * "the doc has moved since this was computed".
 */

import type {
  DocumentId,
  SessionEditSpan,
  SessionQueryPriority,
  SessionRequestId,
  SessionResponse,
} from "@brink/wasm-types";
import type { SessionTransport } from "./transport.js";

export interface QueryOptions {
  /** Scheduling class; defaults to `"interactive"`. */
  priority?: SessionQueryPriority;
  /** Document this query reads — enables staleness tagging and drops. */
  doc?: DocumentId;
  /** Background-only supersession handle (spec §6). */
  coalesceKey?: string;
}

export interface QueryResult<T> {
  value: T;
  /** Doc version at execution time; `undefined` for doc-less queries. */
  docVersion?: number;
  configEpoch: number;
  /** True when the doc moved after this result was computed — the
   *  consumer decides whether to map-then-land or re-request. */
  stale: boolean;
}

export interface QueryHandle<T> {
  readonly id: SessionRequestId;
  readonly promise: Promise<QueryResult<T>>;
  /** Best-effort: drops the query if still queued and rejects the
   *  promise with `QueryDroppedError("cancelled")`. */
  cancel(): void;
}

/** A query removed by scheduler policy or by `cancel()` before running. */
export class QueryDroppedError extends Error {
  constructor(readonly reason: "superseded" | "stale" | "cancelled") {
    super(`dropped:${reason}`);
    this.name = "QueryDroppedError";
  }
}

/** A query that reached the session and threw. */
export class QueryFailedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "QueryFailedError";
  }
}

interface Pending {
  doc: DocumentId | undefined;
  resolve(result: QueryResult<unknown>): void;
  reject(error: Error): void;
}

export class SessionClient {
  private nextId: SessionRequestId = 1;
  private readonly pending = new Map<SessionRequestId, Pending>();
  private readonly docVersions = new Map<DocumentId, number>();
  private readonly lastAcks = new Map<DocumentId, { docVersion: number; applied: boolean }>();
  private readonly eventListeners = new Set<(event: unknown) => void>();

  constructor(private readonly transport: SessionTransport) {
    transport.setOnResponse((response) => this.onResponse(response));
  }

  /** The client-side version counter for a doc (0 before any mutation). */
  docVersion(doc: DocumentId): number {
    return this.docVersions.get(doc) ?? 0;
  }

  /** Latest ack received for a doc — `applied: false` means the host
   *  refused the mutation and the caller should fall back to a full
   *  `pushSource`. Acks always precede any later query's result. */
  lastAck(doc: DocumentId): { docVersion: number; applied: boolean } | null {
    return this.lastAcks.get(doc) ?? null;
  }

  /** Ordered, fire-and-forget bounded edit. Returns the new doc version. */
  applyEdits(doc: DocumentId, edits: readonly SessionEditSpan[]): number {
    const docVersion = this.bump(doc);
    this.transport.post({ kind: "edit", doc, docVersion, edits: [...edits] });
    return docVersion;
  }

  /** Ordered, fire-and-forget full-text push. Returns the new doc version. */
  pushSource(doc: DocumentId, source: string): number {
    const docVersion = this.bump(doc);
    this.transport.post({ kind: "push", doc, docVersion, source });
    return docVersion;
  }

  /** Ordered config-surface mutation (`setDialect`, `applyProjectConfig`, …). */
  config(method: string, ...args: unknown[]): void {
    this.transport.post({ kind: "config", op: { method, args } });
  }

  /** Ordered file-surface mutation (`updateFile`, `removeFile`, …). */
  files(method: string, ...args: unknown[]): void {
    this.transport.post({ kind: "files", op: { method, args } });
  }

  query<T>(method: string, args: unknown[] = [], options: QueryOptions = {}): QueryHandle<T> {
    const id = this.nextId;
    this.nextId += 1;
    const doc = options.doc;
    const promise = new Promise<QueryResult<T>>((resolve, reject) => {
      this.pending.set(id, {
        doc,
        resolve: resolve as Pending["resolve"],
        reject,
      });
    });
    try {
      this.transport.post({
        kind: "query",
        id,
        priority: options.priority ?? "interactive",
        ...(doc !== undefined ? { doc, docVersion: this.docVersion(doc) } : {}),
        ...(options.coalesceKey !== undefined ? { coalesceKey: options.coalesceKey } : {}),
        method,
        args,
      });
    } catch (error) {
      // JSON-unsafe args (or a closed transport) throw synchronously —
      // don't leave the never-answerable entry in `pending`.
      this.pending.delete(id);
      throw error;
    }
    return {
      id,
      promise,
      cancel: () => {
        const entry = this.pending.get(id);
        if (!entry) return;
        this.pending.delete(id);
        this.transport.post({ kind: "cancel", id });
        entry.reject(new QueryDroppedError("cancelled"));
      },
    };
  }

  /** Subscribe to host events (file-change egress, config warnings, …).
   *  Returns the unsubscribe function. */
  onEvent(listener: (event: unknown) => void): () => void {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  close(): void {
    for (const [, entry] of this.pending) {
      entry.reject(new QueryDroppedError("cancelled"));
    }
    this.pending.clear();
    this.transport.close();
  }

  private bump(doc: DocumentId): number {
    const next = this.docVersion(doc) + 1;
    this.docVersions.set(doc, next);
    return next;
  }

  private onResponse(response: SessionResponse): void {
    switch (response.kind) {
      case "ack":
        this.lastAcks.set(response.doc, {
          docVersion: response.docVersion,
          applied: response.applied,
        });
        return;
      case "result": {
        const entry = this.pending.get(response.id);
        if (!entry) return; // cancelled after execution — drop silently
        this.pending.delete(response.id);
        entry.resolve({
          value: response.value,
          ...(response.docVersion !== undefined ? { docVersion: response.docVersion } : {}),
          configEpoch: response.configEpoch,
          stale:
            entry.doc !== undefined &&
            response.docVersion !== undefined &&
            response.docVersion < this.docVersion(entry.doc),
        });
        return;
      }
      case "error": {
        const entry = this.pending.get(response.id);
        if (!entry) return;
        this.pending.delete(response.id);
        if (response.message === "dropped:superseded") {
          entry.reject(new QueryDroppedError("superseded"));
        } else if (response.message === "dropped:stale") {
          entry.reject(new QueryDroppedError("stale"));
        } else {
          entry.reject(new QueryFailedError(response.message));
        }
        return;
      }
      case "event":
        for (const listener of this.eventListeners) listener(response.event);
        return;
    }
  }
}
