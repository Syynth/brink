/**
 * @brink/studio-shell — notification service (docs/studio-shell-spec.md §7.5).
 *
 * Replaces the one-slot Toast: a `NotificationCenter` owns the model — active
 * toasts (max 3 visible, newest first, "+N more" overflow), a capped session
 * history behind the status-bar bell, an unread count, and per-notification
 * auto-dismiss timers (severity defaults: info 5s, warning 8s, error sticky;
 * hover pauses).
 *
 * Actions dispatch commands only — the model carries `commandId` + `args`,
 * never callbacks (spec §6 "nothing binds a key directly to a function"
 * applies to notification buttons too). This keeps notifications plain,
 * serializable data, callable from feature slices and later from host
 * extensions via the StudioApi facade (§8.2).
 *
 * Framework-free: React bindings live in shell-context (useNotifications /
 * useNotificationState) and notification-ui (NotificationStack /
 * NotificationBell).
 */

export type NotificationSeverity = "info" | "warning" | "error";

/** A notification button: label + the command it dispatches. No callbacks. */
export interface NotificationAction {
  label: string;
  commandId: string;
  args?: unknown;
}

/** The notification model (spec §7.5). */
export interface Notification {
  id: string;
  severity: NotificationSeverity;
  message: string;
  /** Origin tag, shown subdued — "binder", "story", "host.<vendor>", … */
  source?: string;
  actions?: NotificationAction[];
  /**
   * Auto-dismiss delay in ms. Omitted → severity default (SEVERITY_TIMEOUTS);
   * values ≤ 0 mean sticky (no auto-dismiss).
   */
  timeoutMs?: number;
  /** Service-assigned: when the notification was raised (epoch ms). */
  timestamp: number;
}

/** Input to notify(): the model minus service-assigned fields; id optional. */
export interface NotificationInput {
  id?: string;
  severity: NotificationSeverity;
  message: string;
  source?: string;
  actions?: NotificationAction[];
  timeoutMs?: number;
}

/** Returned by notify(): lets the producer dismiss or amend later. */
export interface NotificationHandle {
  id: string;
  dismiss(): void;
  update(patch: Partial<Omit<NotificationInput, "id">>): void;
}

/** Severity-default auto-dismiss delays (spec §7.5); null = sticky. */
export const SEVERITY_TIMEOUTS: Record<NotificationSeverity, number | null> = {
  info: 5000,
  warning: 8000,
  error: null,
};

/** Max toasts in the stack; the rest collapse into "+N more" (spec §7.5). */
export const MAX_VISIBLE_NOTIFICATIONS = 3;

/** Session-history cap — oldest dropped (unbounded-growth guard). */
export const NOTIFICATION_HISTORY_LIMIT = 100;

/** Immutable snapshot for React (useNotificationState). */
export interface NotificationState {
  /** Toasts currently shown, newest first (≤ MAX_VISIBLE_NOTIFICATIONS). */
  visible: Notification[];
  /** Active-but-collapsed count behind the "+N more" row. */
  overflowCount: number;
  /** Session history, newest first, capped (includes dismissed ones). */
  history: Notification[];
  /** Notifications raised since the bell popover was last opened. */
  unread: number;
  /** Whether the bell history popover is open (service-coordinated so the
   *  stack's overflow collapser can open the same popover). */
  historyOpen: boolean;
}

/**
 * Injectable timer/clock seams so center semantics are testable without
 * real time. Defaults resolve the globals *at call time*, which makes the
 * defaults themselves compatible with vitest fake timers.
 */
export interface NotificationCenterOptions {
  setTimeout?: (callback: () => void, ms: number) => unknown;
  clearTimeout?: (handle: unknown) => void;
  now?: () => number;
}

interface TimerEntry {
  /** Live timer handle; null while paused. */
  handle: unknown;
  /** When the live timer fires (epoch ms); meaningless while paused. */
  deadline: number;
  /** Remaining ms captured at pause; null while running. */
  remainingMs: number | null;
}

export class NotificationCenter {
  private readonly setTimeoutFn: (callback: () => void, ms: number) => unknown;
  private readonly clearTimeoutFn: (handle: unknown) => void;
  private readonly nowFn: () => number;

  /** Active (undismissed) notifications, newest first. */
  private active: Notification[] = [];
  /** Session history, newest first, capped. */
  private history: Notification[] = [];
  private unread = 0;
  private historyOpen = false;
  private readonly timers = new Map<string, TimerEntry>();
  private readonly changeListeners = new Set<() => void>();
  private nextId = 1;
  private snapshot: NotificationState;

  constructor(options: NotificationCenterOptions = {}) {
    this.setTimeoutFn = options.setTimeout ?? ((cb, ms) => setTimeout(cb, ms));
    this.clearTimeoutFn =
      options.clearTimeout ??
      ((handle) => clearTimeout(handle as Parameters<typeof clearTimeout>[0]));
    this.nowFn = options.now ?? (() => Date.now());
    this.snapshot = this.buildSnapshot();
  }

  /**
   * Raise a notification. The id is auto-generated when omitted; an explicit
   * id matching an active notification replaces it (the replacement re-enters
   * at the top of the stack with a fresh history entry).
   */
  notify(input: NotificationInput): NotificationHandle {
    const id = input.id ?? `notification-${this.nextId++}`;
    if (this.active.some((n) => n.id === id)) {
      this.removeActive(id);
    }

    const notification: Notification = {
      id,
      severity: input.severity,
      message: input.message,
      source: input.source,
      actions: input.actions,
      timeoutMs: input.timeoutMs,
      timestamp: this.nowFn(),
    };

    this.active = [notification, ...this.active];
    this.history = [notification, ...this.history].slice(
      0,
      NOTIFICATION_HISTORY_LIMIT,
    );
    // The bell badge counts what the user hasn't seen — a notification raised
    // while the history popover is open is being seen right now.
    if (!this.historyOpen) this.unread += 1;
    this.startTimer(notification);
    this.emit();

    return {
      id,
      dismiss: () => this.dismiss(id),
      update: (patch) => this.update(id, patch),
    };
  }

  /** Dismiss an active notification (history keeps its entry). */
  dismiss(id: string): void {
    if (!this.removeActive(id)) return;
    this.emit();
  }

  /**
   * Amend a notification in place. Active entries keep their stack position;
   * the auto-dismiss timer restarts from the (possibly updated) effective
   * timeout. Dismissed entries are amended in history only.
   */
  update(id: string, patch: Partial<Omit<NotificationInput, "id">>): void {
    const apply = (n: Notification): Notification => ({
      ...n,
      ...patch,
      id: n.id,
      timestamp: n.timestamp,
    });

    let updated: Notification | null = null;
    this.active = this.active.map((n) => {
      if (n.id !== id) return n;
      updated = apply(n);
      return updated;
    });
    this.history = this.history.map((n) => (n.id === id ? apply(n) : n));
    if (updated !== null) {
      this.stopTimer(id);
      this.startTimer(updated);
    }
    this.emit();
  }

  /**
   * Pause an active notification's auto-dismiss (toast hover); the remaining
   * delay is captured and resumes on `resumeTimeout`. No-op for sticky or
   * already-paused notifications.
   */
  pauseTimeout(id: string): void {
    const timer = this.timers.get(id);
    if (timer === undefined || timer.remainingMs !== null) return;
    this.clearTimeoutFn(timer.handle);
    timer.handle = null;
    timer.remainingMs = Math.max(0, timer.deadline - this.nowFn());
  }

  /** Resume a paused auto-dismiss with the remaining delay (hover leave). */
  resumeTimeout(id: string): void {
    const timer = this.timers.get(id);
    if (timer === undefined || timer.remainingMs === null) return;
    const remaining = timer.remainingMs;
    timer.remainingMs = null;
    timer.deadline = this.nowFn() + remaining;
    timer.handle = this.setTimeoutFn(() => this.dismiss(id), remaining);
  }

  /** Open the bell history popover; seen = read, so the badge resets. */
  openHistory(): void {
    if (this.historyOpen && this.unread === 0) return;
    this.historyOpen = true;
    this.unread = 0;
    this.emit();
  }

  closeHistory(): void {
    if (!this.historyOpen) return;
    this.historyOpen = false;
    this.emit();
  }

  toggleHistory(): void {
    if (this.historyOpen) this.closeHistory();
    else this.openHistory();
  }

  /** Empty the session history ("cleared on demand", spec §7.5). Active
   *  toasts stay on screen — clearing the log doesn't retract them. */
  clearHistory(): void {
    if (this.history.length === 0 && this.unread === 0) return;
    this.history = [];
    this.unread = 0;
    this.emit();
  }

  /** Subscribe to state changes. Returns an unsubscribe function. */
  onDidChange(listener: () => void): () => void {
    this.changeListeners.add(listener);
    return () => {
      this.changeListeners.delete(listener);
    };
  }

  /** Immutable snapshot — stable between changes (useSyncExternalStore). */
  getState(): NotificationState {
    return this.snapshot;
  }

  /** Cancel all timers (teardown; tests). State is left as-is. */
  dispose(): void {
    for (const timer of this.timers.values()) {
      if (timer.handle !== null) this.clearTimeoutFn(timer.handle);
    }
    this.timers.clear();
  }

  // ── Internals ─────────────────────────────────────────────────────

  /** Effective auto-dismiss delay: explicit wins (≤ 0 = sticky), else the
   *  severity default. */
  private effectiveTimeout(n: Notification): number | null {
    if (n.timeoutMs !== undefined) return n.timeoutMs > 0 ? n.timeoutMs : null;
    return SEVERITY_TIMEOUTS[n.severity];
  }

  private startTimer(n: Notification): void {
    const timeout = this.effectiveTimeout(n);
    if (timeout === null) return;
    this.timers.set(n.id, {
      handle: this.setTimeoutFn(() => this.dismiss(n.id), timeout),
      deadline: this.nowFn() + timeout,
      remainingMs: null,
    });
  }

  private stopTimer(id: string): void {
    const timer = this.timers.get(id);
    if (timer === undefined) return;
    if (timer.handle !== null) this.clearTimeoutFn(timer.handle);
    this.timers.delete(id);
  }

  /** Remove from the active stack + stop its timer. False if not active. */
  private removeActive(id: string): boolean {
    const next = this.active.filter((n) => n.id !== id);
    if (next.length === this.active.length) return false;
    this.active = next;
    this.stopTimer(id);
    return true;
  }

  private buildSnapshot(): NotificationState {
    return {
      visible: this.active.slice(0, MAX_VISIBLE_NOTIFICATIONS),
      overflowCount: Math.max(0, this.active.length - MAX_VISIBLE_NOTIFICATIONS),
      history: [...this.history],
      unread: this.unread,
      historyOpen: this.historyOpen,
    };
  }

  private emit(): void {
    this.snapshot = this.buildSnapshot();
    for (const listener of this.changeListeners) listener();
  }
}
