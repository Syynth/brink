import { memo } from "react";
import { useStudioStore } from "./StoreContext.js";

/**
 * Program Explorer — a read-only view of the *compiled* program (static),
 * as distinct from the State view (runtime/dynamic).
 *
 * Renders the `.inkt` text dump (`brink_format::write_inkt`): checksum, name
 * table, globals, lists, externals, address paths, and containers with bytecode
 * disassembly. Captured once when a story loads. For compiler conformance and
 * deep debugging. Structured/filterable tables are a follow-up.
 */
function ProgramViewInner() {
  const programInkt = useStudioStore((s) => s.programInkt);

  if (programInkt === null) {
    return (
      <div className="program-view">
        <div className="state-view-empty">
          <p className="state-view-empty-title">No compiled program</p>
          <p className="state-view-empty-hint">
            Run a story to inspect its compiled tables (globals, lists,
            externals, containers) and bytecode disassembly here.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="program-view">
      <pre className="program-view-dump">{programInkt}</pre>
    </div>
  );
}

export const ProgramView = memo(ProgramViewInner);
