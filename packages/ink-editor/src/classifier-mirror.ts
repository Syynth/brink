/**
 * ClassifierMirror (docs/editor-worker-spec.md §4, W3) — the main-thread
 * classifier instance behind one view's keystroke path.
 *
 * Owns a `ClassifierSessionHandle` mirroring the view's document (every
 * effective push/edit that reaches the project session is forwarded here
 * first-class), plus a version-keyed slice cache with the same semantics
 * as `DocHandle`'s (#3064 option A): fetch-once per segment key,
 * changed-segments-only refetch, config-epoch invalidation, dead-key
 * pruning. Its keys live in the CLASSIFIER's identity space — salsa ids
 * are database-instance-specific, so classifier keys and project-session
 * keys must never mix in one cache (they are cached in separate planes
 * and, where the token blend pairs them, paired positionally).
 *
 * When the project session moves to the worker (W4), this mirror is what
 * keeps same-frame styling: its wasm instance stays on the main thread
 * and never runs analysis.
 */

import type { SegmentManifest } from "@brink-lang/web";
import type { LineContext, SemanticToken } from "@brink/wasm-types";

/** The classifier surface the mirror needs — structurally satisfied by
 *  `ClassifierSessionHandle` (`@brink-lang/web`); an interface here so the
 *  editor package needs no runtime coupling for tests. */
export interface ClassifierLike {
  readonly available: boolean;
  configEpoch(): number;
  open(path: string, source: string): boolean;
  updateSource(source: string): boolean;
  applyEdits(edits: readonly { from: number; to: number; insert: string }[]): boolean;
  getSegmentManifest(): SegmentManifest | null;
  getSegmentLineContexts(key: string): LineContext[] | null;
  getSegmentSemanticTokensFast(key: string): SemanticToken[] | null;
  setDialect(dialect: unknown): void;
  clearDialect(): void;
  free(): void;
}

export class ClassifierMirror {
  private readonly slices = new Map<
    string,
    { contexts?: LineContext[]; tokens?: SemanticToken[] }
  >();
  private manifestCache: SegmentManifest | null = null;
  private manifestStale = true;
  private sliceEpoch = -1;
  private freed = false;
  /** Set when a mirrored edit was refused while the session's succeeded —
   *  the mirror's content can no longer be trusted, so every read returns
   *  null (session-road fallback) until the next effective full push
   *  resynchronizes it. Defensive: both sides validate edits identically,
   *  so this should be unreachable in practice. */
  private desynced = false;

  markDesynced(): void {
    this.desynced = true;
  }

  constructor(private readonly classifier: ClassifierLike) {}

  /** Mirror an effective full push (also clears a desync — full text is
   *  a resynchronization point). */
  push(source: string): void {
    if (this.freed) return;
    if (this.classifier.updateSource(source)) {
      this.manifestStale = true;
      this.desynced = false;
    }
  }

  /** Mirror an effective bounded edit; false → the caller must full-push. */
  applyEdits(edits: readonly { from: number; to: number; insert: string }[]): boolean {
    if (this.freed) return false;
    const ok = this.classifier.applyEdits(edits);
    if (ok) this.manifestStale = true;
    return ok;
  }

  setDialect(dialect: unknown): void {
    if (!this.freed) this.classifier.setDialect(dialect);
  }

  clearDialect(): void {
    if (!this.freed) this.classifier.clearDialect();
  }

  manifest(): SegmentManifest | null {
    if (this.freed || this.desynced) return null;
    const epoch = this.classifier.configEpoch();
    if (epoch !== this.sliceEpoch) {
      this.sliceEpoch = epoch;
      this.slices.clear();
      this.manifestStale = true;
    }
    if (this.manifestStale) {
      this.manifestCache = this.classifier.getSegmentManifest();
      this.manifestStale = false;
      if (this.manifestCache !== null) {
        const live = new Set(this.manifestCache.segments.map((s) => s.key));
        for (const key of this.slices.keys()) {
          if (!live.has(key)) this.slices.delete(key);
        }
      }
    }
    return this.manifestCache;
  }

  /** Same shape as `DocHandle.lineContextSlices` — classifier-plane keys. */
  lineContextSlices(): { key: string; ownedFrom: number; contexts: LineContext[] }[] | null {
    const manifest = this.manifest();
    if (manifest === null) return null;
    const out: { key: string; ownedFrom: number; contexts: LineContext[] }[] = [];
    for (const seg of manifest.segments) {
      const entry = this.slices.get(seg.key);
      let contexts = entry?.contexts;
      if (!contexts) {
        const slice = this.classifier.getSegmentLineContexts(seg.key);
        if (slice === null) return null; // stale key — caller falls back
        contexts = slice;
        this.slices.set(seg.key, { ...entry, contexts });
      }
      out.push({ key: seg.key, ownedFrom: seg.ownedFrom, contexts });
    }
    return out;
  }

  /** One segment's classifier tokens by classifier key, cached
   *  (segment-relative lines — the caller rebases by `ownedFrom`). */
  fastTokens(key: string): SemanticToken[] | null {
    if (this.freed) return null;
    const entry = this.slices.get(key);
    if (entry?.tokens) return entry.tokens;
    const slice = this.classifier.getSegmentSemanticTokensFast(key);
    if (slice !== null) this.slices.set(key, { ...entry, tokens: slice });
    return slice;
  }

  free(): void {
    if (this.freed) return;
    this.freed = true;
    this.slices.clear();
    this.classifier.free();
  }
}
