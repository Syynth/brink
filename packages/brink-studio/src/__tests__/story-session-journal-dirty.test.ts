/**
 * StorySessionHandle journal-dirty notification tests (#390,
 * docs/story-session-spec.md): the spec-mandated persistence hook that #389
 * dropped when it closed #387. The hook must fire deferred (never
 * synchronously inside a `StorySessionHandle` call) and debounced (a burst of
 * choose/advance calls coalesces into one notification), and the callback
 * must never re-enter while a `StorySessionHandle` method is still on the
 * call stack.
 *
 * Runs against the `brink-web` mock's `WebSession` stand-in
 * (src/__mocks__/brink-web.ts) — the mock bumps an in-memory event counter
 * one-for-one with every journal-mutating call, which is all this hook's
 * TS-side debounce/coalescing logic needs to be exercised faithfully; the
 * real journaling semantics are covered by the Rust `brink-runtime` session
 * tests.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { StorySessionHandle, type JournalDirtySignal } from "@brink-lang/web";

const STORY_BYTES = new Uint8Array([0, 1, 2, 3]);

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("StorySessionHandle.onJournalDirty", () => {
  it("never fires synchronously inside a mutating call", () => {
    const session = new StorySessionHandle(STORY_BYTES);
    const signals: JournalDirtySignal[] = [];
    session.onJournalDirty((s) => signals.push(s));

    session.advance();
    // The notification must not have fired yet — it's deferred to a macrotask,
    // not delivered synchronously as part of `advance()` returning.
    expect(signals).toHaveLength(0);

    vi.runAllTimers();
    expect(signals).toHaveLength(1);
  });

  it("coalesces a burst of choose/advance calls into one notification", () => {
    const session = new StorySessionHandle(STORY_BYTES);
    const signals: JournalDirtySignal[] = [];
    session.onJournalDirty((s) => signals.push(s));

    // A rapid burst, each call landing well inside the debounce window.
    session.advance();
    vi.advanceTimersByTime(10);
    session.choose(0);
    vi.advanceTimersByTime(10);
    session.advance();
    vi.advanceTimersByTime(10);

    expect(signals).toHaveLength(0); // still within the debounce window

    vi.runAllTimers();

    // Exactly one coalesced notification for the whole burst, carrying the
    // latest event count (not one per call).
    expect(signals).toHaveLength(1);
    expect(signals[0]!.eventCount).toBe(3);
  });

  it("fires again for a second, separated burst", () => {
    const session = new StorySessionHandle(STORY_BYTES);
    const signals: JournalDirtySignal[] = [];
    session.onJournalDirty((s) => signals.push(s));

    session.advance();
    vi.runAllTimers();
    expect(signals).toHaveLength(1);
    expect(signals[0]!.eventCount).toBe(1);

    session.choose(0);
    vi.runAllTimers();
    expect(signals).toHaveLength(2);
    expect(signals[1]!.eventCount).toBe(2);
  });

  it("does not notify when nothing grew the journal", () => {
    const session = new StorySessionHandle(STORY_BYTES);
    const signals: JournalDirtySignal[] = [];
    session.onJournalDirty((s) => signals.push(s));

    // Reads that don't mutate the journal (no wrapper call at all).
    void session.hasPendingExternal();
    void session.saveState();

    vi.runAllTimers();
    expect(signals).toHaveLength(0);
  });

  it("never re-enters a StorySessionHandle method while one is on the stack", () => {
    const session = new StorySessionHandle(STORY_BYTES);
    let reentered = false;
    let insideMutatingCall = false;

    session.onJournalDirty(() => {
      // If the notification were ever delivered synchronously (or from
      // within another wrapper call's own stack frame), this listener body
      // running while `insideMutatingCall` is true would prove re-entrancy.
      if (insideMutatingCall) {
        reentered = true;
      }
      // Also prove the callback itself can safely call back into the
      // session (the pull side of the hook) without deadlocking or
      // corrupting state, exactly as a host's persistence code would.
      session.exportJournal();
    });

    insideMutatingCall = true;
    session.advance();
    session.choose(0);
    insideMutatingCall = false;

    vi.runAllTimers();

    expect(reentered).toBe(false);
  });

  it("unsubscribe stops further notifications", () => {
    const session = new StorySessionHandle(STORY_BYTES);
    const signals: JournalDirtySignal[] = [];
    const unsubscribe = session.onJournalDirty((s) => signals.push(s));

    session.advance();
    vi.runAllTimers();
    expect(signals).toHaveLength(1);

    unsubscribe();
    session.choose(0);
    vi.runAllTimers();
    expect(signals).toHaveLength(1); // no new notification after unsubscribe
  });

  it("restart resets the dirty baseline so a fresh journal isn't reported dirty", () => {
    const session = new StorySessionHandle(STORY_BYTES);
    const signals: JournalDirtySignal[] = [];

    session.advance();
    session.restart();
    session.onJournalDirty((s) => signals.push(s));

    vi.runAllTimers();
    expect(signals).toHaveLength(0);

    session.advance();
    vi.runAllTimers();
    expect(signals).toHaveLength(1);
    expect(signals[0]!.eventCount).toBe(1);
  });
});
