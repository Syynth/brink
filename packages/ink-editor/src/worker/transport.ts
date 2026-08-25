/**
 * Session transport boundary (docs/editor-worker-spec.md §5).
 *
 * A transport carries {@link SessionRequest} envelopes to a session host
 * and delivers {@link SessionResponse} envelopes back. Three
 * implementations are planned by the spec: `LocalTransport` (W1 —
 * in-process, microtask-scheduled), `WorkerTransport` (W4 —
 * postMessage), and a hypothetical native transport (§5.4 — a sidecar
 * byte stream). Consumers depend only on this interface.
 *
 * Wire shapes are mirrored from the Rust source of truth
 * (`crates/brink-web/src/protocol.rs`) via `@brink/wasm-types`.
 */

import type { SessionRequest, SessionResponse } from "@brink/wasm-types";

export interface SessionTransport {
  /**
   * Send one request envelope. Ordering guarantee: requests posted from
   * the same task are delivered to the host in post order (the host's
   * scheduler — not the transport — decides execution order beyond the
   * mutation stream's FIFO).
   *
   * Throws `TypeError` if the payload is not JSON-serializable — every
   * transport enforces the spec §5.1 JSON-safety contract, so a payload
   * that would silently mangle over a byte stream fails loudly over
   * every transport, including the in-process one.
   */
  post(request: SessionRequest): void;

  /** Register the single response listener. Replaces any previous one. */
  setOnResponse(listener: (response: SessionResponse) => void): void;

  /** Tear down. Posting after close throws. */
  close(): void;
}
