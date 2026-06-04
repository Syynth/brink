import { memo } from "react";

/**
 * State View — a read-only runtime debugger panel (globals, current location,
 * call stack, visit counts, transcript).
 *
 * Placeholder for now: the structured runtime introspection API it depends on
 * is tracked as a separate needs-design issue. This shell establishes the
 * activity-bar slot so the real view can drop in without further layout work.
 */
function StateViewInner() {
  return (
    <div className="state-view">
      <div className="state-view-empty">
        <p className="state-view-empty-title">State inspection coming soon</p>
        <p className="state-view-empty-hint">
          This panel will show the running story&apos;s variables, current
          location, call stack, and visit counts.
        </p>
      </div>
    </div>
  );
}

export const StateView = memo(StateViewInner);
