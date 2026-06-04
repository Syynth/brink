import { memo } from "react";
import { useStudioStore } from "./StoreContext.js";

/**
 * State View — a read-only runtime debugger panel.
 *
 * Interim: renders `Story::debug_state()` (status, current position, call
 * stack, value stack, output buffer, globals, pending choices) as a raw text
 * dump, refreshed whenever the story advances. The structured, name-resolved
 * `DebugSnapshot` view is tracked in issue #62.
 */
function StateViewInner() {
  const debugState = useStudioStore((s) => s.debugState);

  if (debugState === null) {
    return (
      <div className="state-view">
        <div className="state-view-empty">
          <p className="state-view-empty-title">No running story</p>
          <p className="state-view-empty-hint">
            Run a story in the player to inspect its variables, current location,
            call stack, and visit counts here.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="state-view">
      <pre className="state-view-dump">{debugState}</pre>
    </div>
  );
}

export const StateView = memo(StateViewInner);
