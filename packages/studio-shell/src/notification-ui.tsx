/**
 * @brink/studio-shell — notification presentation (spec §7.5).
 *
 * NotificationStack: toasts stacked bottom-right above the status bar,
 * newest on top, max 3 with a "+N more" collapser. Hover pauses that toast's
 * auto-dismiss. Action buttons dispatch commands by id (never callbacks) and
 * dismiss the toast. Quiet per the Zed direction — hairline border, severity
 * as a left accent edge, not a filled background.
 *
 * NotificationBell: a status-bar segment (registered from the app side like
 * the other right-group segments — the shell never registers itself) with an
 * unread badge; click opens the session history on the overlay primitive
 * (§7.7). The open state lives in the NotificationCenter so the stack's
 * overflow collapser opens the same popover without knowing about the bell.
 */

import { useState } from "react";
import { useNotificationState, useShell } from "./shell-context.js";
import { Overlay } from "./overlay.js";
import type { Notification } from "./notifications.js";

// ── Toast stack ─────────────────────────────────────────────────────

function NotificationToast({ notification: n }: { notification: Notification }) {
  const { commands, notifications } = useShell();
  return (
    <div
      className={`shell-notification severity-${n.severity}`}
      data-notification-id={n.id}
      onMouseEnter={() => notifications.pauseTimeout(n.id)}
      onMouseLeave={() => notifications.resumeTimeout(n.id)}
    >
      <div className="shell-notification-body">
        {n.source !== undefined && (
          <span className="shell-notification-source">{n.source}</span>
        )}
        <span className="shell-notification-message">{n.message}</span>
        {n.actions !== undefined && n.actions.length > 0 && (
          <div className="shell-notification-actions">
            {n.actions.map((action) => (
              <button
                key={action.label}
                type="button"
                className="shell-notification-action"
                onClick={() => {
                  commands.dispatch(action.commandId, action.args);
                  notifications.dismiss(n.id);
                }}
              >
                {action.label}
              </button>
            ))}
          </div>
        )}
      </div>
      <button
        type="button"
        className="shell-notification-close"
        aria-label="Dismiss notification"
        onClick={() => notifications.dismiss(n.id)}
      >
        {"×"}
      </button>
    </div>
  );
}

/**
 * The toast stack region. Mounted once at the app root (next to the command
 * palette); renders nothing while no notification is active.
 */
export function NotificationStack() {
  const { notifications } = useShell();
  const { visible, overflowCount } = useNotificationState();
  if (visible.length === 0) return null;
  return (
    <div
      className="shell-notifications"
      role="region"
      aria-label="Notifications"
      aria-live="polite"
    >
      {visible.map((n) => (
        <NotificationToast key={n.id} notification={n} />
      ))}
      {overflowCount > 0 && (
        <button
          type="button"
          className="shell-notification-overflow"
          onClick={() => notifications.openHistory()}
        >
          +{overflowCount} more
        </button>
      )}
    </div>
  );
}

// ── Status-bar bell ─────────────────────────────────────────────────

/** Monochrome bell glyph (currentColor, like the strip icons). A component —
 *  not a module-level JSX const — so importing this module never executes
 *  JSX (vitest runs without the React plugin's automatic transform). */
function BellIcon() {
  return (
    <svg
      width={13}
      height={13}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M8 2a4 4 0 0 0-4 4c0 3-1.2 4.6-1.2 4.6h10.4S12 9 12 6a4 4 0 0 0-4-4z" />
      <path d="M6.8 13.5a1.3 1.3 0 0 0 2.4 0" />
    </svg>
  );
}

function pad(value: number): string {
  return value.toString().padStart(2, "0");
}

function formatTimestamp(epochMs: number): string {
  const d = new Date(epochMs);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/**
 * Bell segment for the status bar's right group (registered in main.tsx with
 * the other segments). Unread badge + the anchored history popover: newest
 * first, severity dot, source, message, HH:MM:SS time, and a Clear button.
 */
export function NotificationBell() {
  const { notifications } = useShell();
  const { history, unread, historyOpen } = useNotificationState();
  // The anchor is state (not a ref) so the Overlay re-renders once the
  // button exists and repositions via floating-ui's autoUpdate from then on.
  const [anchor, setAnchor] = useState<HTMLButtonElement | null>(null);

  return (
    <>
      <button
        ref={setAnchor}
        type="button"
        className="shell-notification-bell clickable"
        title="Notifications"
        aria-label={
          unread > 0 ? `Notifications (${unread} unread)` : "Notifications"
        }
        onClick={() => notifications.toggleHistory()}
      >
        <BellIcon />
        {unread > 0 && <span className="shell-notification-badge">{unread}</span>}
      </button>
      <Overlay
        open={historyOpen}
        onClose={() => notifications.closeHistory()}
        anchor={anchor}
        placement="top-end"
        className="shell-notification-popover"
      >
        <div className="shell-notification-history-header">
          <span>Notifications</span>
          <button
            type="button"
            onClick={() => notifications.clearHistory()}
            disabled={history.length === 0}
          >
            Clear
          </button>
        </div>
        {history.length === 0 ? (
          <div className="shell-notification-history-empty">No notifications</div>
        ) : (
          <ul className="shell-notification-history">
            {history.map((n) => (
              <li
                key={n.id}
                className={`shell-notification-history-item severity-${n.severity}`}
              >
                <span className="dot" aria-hidden />
                <span className="text">
                  {n.source !== undefined && (
                    <span className="source">{n.source}</span>
                  )}
                  {n.message}
                </span>
                <span className="time">{formatTimestamp(n.timestamp)}</span>
              </li>
            ))}
          </ul>
        )}
      </Overlay>
    </>
  );
}
