import { memo, useEffect, useRef, useState } from "react";
import type { KnotNode } from "@brink/wasm-types";
import { useShell } from "@brink/studio-shell";
import { sessionDegraded } from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";
import { OPEN_COMPILED_OUTPUT_COMMAND_ID } from "./CompiledOutputDocument.js";

/** `(container_idx, offset)` — the shape both `DebugState.position` and a
 *  `KnotNode.disasm` entry's own `offset` (paired with the node's
 *  `container_idx`) are compared against for the current-instruction
 *  highlight (D9, #3187). */
interface RuntimePosition {
  container_idx: number;
  offset: number;
}

/**
 * Program Explorer — a structured, navigable view of the *compiled* program
 * (static), distinct from the runtime State view.
 *
 * Tables for globals / lists / externals plus a knot → stitch tree you drill
 * into for flags, path hash, and name-resolved bytecode. The raw `.inkt`
 * dump lives in the read-only Compiled Output editor document (issue #91,
 * spec §4) — the toolbar button opens it via `program.openCompiledOutput`.
 *
 * Session-overlaid (D9, #3187): while a story runs, the currently executing
 * instruction is highlighted in its knot's disassembly, keyed by
 * `(container_idx, offset)` — the same join `docs/live-inspector-spec.md`
 * §5 already gates the Story Graph's current-location highlight on.
 * `sessionDegraded` suppresses the highlight (never shows a stale one) the
 * moment the running program's checksum diverges from the studio's latest
 * compile — an edited-but-not-yet-restarted session, the normal case, not
 * an error.
 */
function ProgramViewInner() {
  const model = useStudioStore((s) => s.programModel);
  const { commands } = useShell();
  const degraded = useStudioStore((s) => sessionDegraded(s.programChecksum, s.compiledChecksum));
  const debugPosition = useStudioStore((s) => s.debugState?.position);
  // Frame-follow (W9/#3302): a selected non-top stack frame retargets the
  // current-instruction highlight to ITS resume position — the explorer
  // follows what the Debugger panel is inspecting, not just the top.
  const framePosition = useStudioStore((s) =>
    s.selectedFrameIdx !== null
      ? (s.debugState?.call_stack?.[s.selectedFrameIdx]?.position ?? null)
      : null,
  );
  const explorerTarget = useStudioStore((s) => s.programExplorerTarget);
  const currentPosition: RuntimePosition | null = degraded
    ? null
    : (framePosition ?? debugPosition ?? null);

  if (!model) {
    return (
      <div className="program-view">
        <div className="state-view-empty">
          <p className="state-view-empty-title">No compiled program</p>
          <p className="state-view-empty-hint">
            Run a story to inspect its globals, lists, externals, and knots
            (with bytecode) here.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="program-view">
      <div className="pv-toolbar">
        <span className="pv-checksum" title="source checksum">
          {model.checksum}
        </span>
        <button
          type="button"
          className="pv-open-inkt"
          title="Open the .inkt dump as a read-only editor document"
          onClick={() => commands.dispatch(OPEN_COMPILED_OUTPUT_COMMAND_ID)}
        >
          open .inkt
        </button>
      </div>

      <div className="pv-body">
        <Section title={`Globals (${model.globals.length})`}>
          {model.globals.length === 0 ? (
            <p className="sv-empty">none</p>
          ) : (
            <table className="sv-table">
              <tbody>
                {model.globals.map((g) => (
                  <tr key={g.name}>
                    <td className="sv-key">{g.name}</td>
                    <td className="sv-dim pv-ty">{g.ty}</td>
                    <td className="sv-val sv-mono">{g.default}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Section>

        {model.lists.length > 0 && (
          <Section title={`Lists (${model.lists.length})`}>
            {model.lists.map((l) => (
              <div key={l.name} className="pv-list">
                <div className="sv-key pv-list-name">{l.name}</div>
                <div className="pv-list-items">
                  {l.items.map((it) => (
                    <span key={it.name} className="pv-list-item">
                      {it.name}
                      <span className="sv-dim">·{it.ordinal}</span>
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </Section>
        )}

        {model.externals.length > 0 && (
          <Section title={`Externals (${model.externals.length})`}>
            <table className="sv-table">
              <tbody>
                {model.externals.map((e) => (
                  <tr key={e.name}>
                    <td className="sv-key">{e.name}</td>
                    <td className="sv-dim">{e.arg_count} arg{e.arg_count === 1 ? "" : "s"}</td>
                    <td className="sv-val sv-path">{e.fallback ? `→ ${e.fallback}` : ""}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Section>
        )}

        <Section title={`Knots (${model.knots.length})`}>
          {model.knots.length === 0 ? (
            <p className="sv-empty">none</p>
          ) : (
            model.knots.map((k) => (
              <KnotRow
                key={k.path}
                node={k}
                depth={0}
                currentPosition={currentPosition}
                target={explorerTarget}
              />
            ))
          )}
        </Section>
      </div>
    </div>
  );
}

// ── Knot/stitch tree row ────────────────────────────────────────────

function KnotRow({
  node,
  depth,
  currentPosition,
  target,
}: {
  node: KnotNode;
  depth: number;
  currentPosition: RuntimePosition | null;
  target: { address: RuntimePosition; nonce: number } | null;
}) {
  const [open, setOpen] = useState(false);
  const indent = 6 + depth * 12;
  // "Reveal in Program Explorer" (W9/#3302): a target inside this knot's
  // container auto-opens the row and scrolls its instruction into view.
  // A container held by a DESCENDANT also opens this row (the child can't
  // render while its ancestors are closed) — cheap recursive check.
  const targetsSelf =
    target !== null &&
    node.container_idx !== 0xffffffff &&
    node.container_idx === target.address.container_idx;
  const targetsSubtree = target !== null && subtreeHasContainer(node, target.address.container_idx);
  const targetLineRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (targetsSubtree) setOpen(true);
  }, [targetsSubtree, target?.nonce]);
  useEffect(() => {
    if (targetsSelf && open) {
      // jsdom has no scrollIntoView — the auto-open + marker class are
      // what tests pin; the scroll is a browser nicety.
      targetLineRef.current?.scrollIntoView?.({ block: "center" });
    }
  }, [targetsSelf, open, target?.nonce]);
  // `currentPosition` is already `null` while degraded (the caller
  // computed that); a container_idx match alone is not enough — a knot
  // with no backing container (`u32.MAX` sentinel, program_model.rs's
  // synthesized-node case) must never match.
  const isCurrentKnot =
    currentPosition !== null &&
    node.container_idx !== 0xffffffff &&
    node.container_idx === currentPosition.container_idx;
  return (
    <div className="pv-knot">
      <button
        type="button"
        className={"pv-knot-header" + (isCurrentKnot ? " pv-current-knot" : "")}
        style={{ paddingLeft: indent }}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <span className="pv-chevron">{open ? "▾" : "▸"}</span>
        <span className={"pv-knot-name" + (node.kind === "stitch" ? " pv-stitch" : "")}>
          {node.name}
        </span>
        {isCurrentKnot && (
          <span className="pv-current-marker" title="currently executing">
            ▶
          </span>
        )}
        {node.flags.length > 0 && <span className="pv-flags">{node.flags.join(" ")}</span>}
      </button>
      {open && (
        <div className="pv-knot-body" style={{ paddingLeft: indent + 14 }}>
          <div className="pv-meta">
            <span className="sv-key">path</span>
            <span className="sv-path">{node.path}</span>
            {node.path_hash !== 0 && (
              <>
                <span className="sv-key">hash</span>
                <span className="sv-num">{node.path_hash}</span>
              </>
            )}
          </div>
          {node.disasm.length > 0 && (
            <pre className="pv-disasm">
              {node.disasm.map((line) => {
                const isTargetLine = targetsSelf && line.offset === target.address.offset;
                return (
                  <div
                    key={line.offset}
                    ref={isTargetLine ? targetLineRef : undefined}
                    className={
                      "pv-disasm-line" +
                      (isCurrentKnot && line.offset === currentPosition?.offset
                        ? " pv-current-instruction"
                        : "") +
                      (isTargetLine ? " pv-target-instruction" : "")
                    }
                  >
                    <span className="pv-disasm-offset">{line.offset}</span>
                    <span className="pv-disasm-text">{line.text}</span>
                  </div>
                );
              })}
            </pre>
          )}
          {node.children.map((c) => (
            <KnotRow
              key={c.path}
              node={c}
              depth={depth + 1}
              currentPosition={currentPosition}
              target={target}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/** Whether `node` or any descendant is backed by `containerIdx`. */
function subtreeHasContainer(node: KnotNode, containerIdx: number): boolean {
  if (node.container_idx !== 0xffffffff && node.container_idx === containerIdx) return true;
  return node.children.some((c) => subtreeHasContainer(c, containerIdx));
}

/** Instruction-stepping controls (W9/#3302) for the Program Explorer's
 * header-actions slot — the granularity ladder's programmer-assist tier
 * (`stepi`), RULED to live here and never in the Player toolbar. Same
 * enablement as the transport: paused only. */
function ProgramExplorerActionsInner() {
  const debugCapable = useStudioStore((s) => s.debugCapable);
  const paused = useStudioStore((s) => s.sessionPaused);
  const debugStep = useStudioStore((s) => s.debugStep);
  if (!debugCapable) return null;
  return (
    <span className="dp-actions">
      <button
        type="button"
        className="dp-action"
        title="stepi — one instruction, descending into calls"
        aria-label="Step instruction"
        disabled={!paused}
        onClick={() => debugStep("into")}
      >
        {"⇣i"}
      </button>
      <button
        type="button"
        className="dp-action"
        title="stepi over — one instruction, calls run to completion"
        aria-label="Step instruction over"
        disabled={!paused}
        onClick={() => debugStep("over")}
      >
        {"⇢i"}
      </button>
      <button
        type="button"
        className="dp-action"
        title="stepi out — run until the current frame returns"
        aria-label="Step instruction out"
        disabled={!paused}
        onClick={() => debugStep("out")}
      >
        {"⇡i"}
      </button>
    </span>
  );
}
export const ProgramExplorerActions = memo(ProgramExplorerActionsInner);

// ── Collapsible section ─────────────────────────────────────────────

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <div className="sv-section">
      <button
        type="button"
        className={"sv-section-header" + (open ? " open" : "")}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="sv-chevron">{open ? "▾" : "▸"}</span>
        {title}
      </button>
      {open && <div className="sv-section-body">{children}</div>}
    </div>
  );
}

export const ProgramView = memo(ProgramViewInner);
