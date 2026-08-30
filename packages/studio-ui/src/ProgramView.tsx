import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { KnotNode, LinesTable } from "@brink/wasm-types";
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
/** Human-readable byte count — `B` under a KiB, else one-decimal `KB`. */
function fmtBytes(n: number): string {
  return n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`;
}

/** A scope's total bytes: its own rollup plus every child scope's. */
function subtreeBytes(node: KnotNode): number {
  return node.byte_size + node.children.reduce((sum, c) => sum + subtreeBytes(c), 0);
}

/** Scope path → emitted-line count, from the compiled lines table. */
function linesByScope(lines: LinesTable | null): Map<string, number> {
  const map = new Map<string, number>();
  if (lines === null) return map;
  for (const scope of lines.scopes) {
    if (scope.name) map.set(scope.name, scope.lines.length);
  }
  return map;
}

/** A knot's line count including its stitches' scopes. */
function subtreeLines(node: KnotNode, byScope: Map<string, number>): number {
  return (
    (byScope.get(node.path) ?? 0) +
    node.children.reduce((sum, c) => sum + subtreeLines(c, byScope), 0)
  );
}

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
  const programLines = useStudioStore((s) => s.programLines);
  const entryFile = useStudioStore((s) => s.entryFile);
  const currentPosition: RuntimePosition | null = degraded
    ? null
    : (framePosition ?? debugPosition ?? null);

  // Derived joins for the header/footer/size bars. `model` may be null on
  // the first render — memos run unconditionally (hooks), guard inside.
  const byScope = useMemo(() => linesByScope(programLines), [programLines]);
  const totals = useMemo(() => {
    if (!model) return null;
    let stitches = 0;
    let containers = 0;
    let bytes = 0;
    for (const k of model.knots) {
      stitches += k.children.length;
      containers += k.container_count + k.children.reduce((n, c) => n + c.container_count, 0);
      bytes += subtreeBytes(k);
    }
    let lineCount = 0;
    let tableCount = 0;
    let templates = 0;
    if (programLines) {
      for (const scope of programLines.scopes) {
        tableCount += 1;
        lineCount += scope.lines.length;
        for (const line of scope.lines) {
          if (typeof line.content === "object" && line.content !== null) templates += 1;
        }
      }
    }
    const maxKnotBytes = Math.max(1, ...model.knots.map((k) => subtreeBytes(k)));
    const maxKnotLines = Math.max(1, ...model.knots.map((k) => subtreeLines(k, byScope)));
    return { stitches, containers, bytes, lineCount, tableCount, templates, maxKnotBytes, maxKnotLines };
  }, [model, programLines, byScope]);

  if (!model || !totals) {
    return (
      <div className="program-view">
        <div className="state-view-empty">
          <p className="state-view-empty-title">No compiled program</p>
          <p className="state-view-empty-hint">
            Run a compile to inspect its knots, line tables, and bytecode here.
          </p>
        </div>
      </div>
    );
  }

  // The program is a named thing: the entry file's stem, not a hex string.
  const programName = entryFile?.split("/").pop()?.replace(/\.(ink|brink)$/, "") ?? "program";
  // The paused location, named the way a save file would name it.
  const currentKnot =
    currentPosition === null
      ? null
      : findByContainer(model.knots, currentPosition.container_idx);

  return (
    <div className="program-view">
      {/* Identity header (#3339): name + checksum chip + counts, and the
          view switch. Views land one PR at a time — a disabled segment is
          a designed slot, not dead chrome; the title says where it is. */}
      <div className="pv-header">
        <span
          className={"pv-status-dot" + (degraded ? " pv-status-stale" : "")}
          title={degraded ? "running session predates this compile" : "compiled, up to date"}
        />
        <span className="pv-program-name">{programName}</span>
        <span className="pv-checksum" title="source checksum">
          {model.checksum}
        </span>
        <span className="pv-counts">
          {model.knots.length} knots · {totals.stitches} stitches · {totals.containers} containers
          {totals.tableCount > 0 ? ` · ${totals.lineCount} lines` : ""}
        </span>
        <span className="pv-header-spacer" />
        <span className="pv-seg" role="tablist" aria-label="Program Explorer view">
          <button type="button" className="pv-seg-item active" role="tab" aria-selected="true">
            Structure
          </button>
          <button type="button" className="pv-seg-item" role="tab" disabled title="Line tables view — #3339, next phase">
            Line tables
          </button>
          <button type="button" className="pv-seg-item" role="tab" disabled title="Disassembly view — #3339, next phase">
            Disassembly
          </button>
          <button type="button" className="pv-seg-item" role="tab" disabled title="Size view — needs the .inkb size report (#3339)">
            Size
          </button>
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

      <div className="pv-structure">
        {/* Definitions column: what the program declares. */}
        <div className="pv-defs">
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
                      <td className="sv-dim">
                        {e.arg_count} arg{e.arg_count === 1 ? "" : "s"}
                      </td>
                      <td className="sv-val">
                        {e.fallback ? (
                          <span
                            className="pv-ext-fallback"
                            title={`Fallback body: ${e.fallback} — the story runs without a host binding`}
                          >
                            fallback
                          </span>
                        ) : (
                          <span
                            className="pv-ext-host"
                            title="No fallback body — a host binding must be registered"
                          >
                            host
                          </span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </Section>
          )}
        </div>

        {/* Knot map: the story's shape, size at a glance. */}
        <div className="pv-knot-map">
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
                byScope={byScope}
                maxBytes={totals.maxKnotBytes}
                maxLines={totals.maxKnotLines}
              />
            ))
          )}
        </div>
      </div>

      <div className="pv-footer">
        <span>
          <strong>{fmtBytes(totals.bytes)}</strong> bytecode
        </span>
        {totals.tableCount > 0 && (
          <span>
            <strong className="pv-footer-lines">{totals.lineCount}</strong> lines in{" "}
            {totals.tableCount} tables
          </span>
        )}
        {totals.templates > 0 && (
          <span>
            <strong className="pv-footer-templates">{totals.templates}</strong> templates
          </span>
        )}
        <span>
          <strong>{model.externals.length}</strong> externals
        </span>
        <span className="pv-header-spacer" />
        {currentKnot !== null && (
          <span className="pv-footer-paused">
            ● paused — {currentKnot.path} +0x{currentPosition!.offset.toString(16)}
          </span>
        )}
      </div>
    </div>
  );
}

/** The scope node backed by `containerIdx`, searching stitches too. */
function findByContainer(knots: KnotNode[], containerIdx: number): KnotNode | null {
  for (const k of knots) {
    if (k.container_idx !== 0xffffffff && k.container_idx === containerIdx) return k;
    const child = findByContainer(k.children, containerIdx);
    if (child !== null) return child;
  }
  return null;
}

// ── Knot/stitch tree row ────────────────────────────────────────────

function KnotRow({
  node,
  depth,
  currentPosition,
  target,
  byScope,
  maxBytes,
  maxLines,
}: {
  node: KnotNode;
  depth: number;
  currentPosition: RuntimePosition | null;
  target: { address: RuntimePosition; nonce: number } | null;
  byScope: Map<string, number>;
  /** Largest top-level knot subtree, for the size bars' shared scale. */
  maxBytes: number;
  maxLines: number;
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
        {depth === 0 && (
          <>
            <span className="pv-size-bar" title="bytecode (track) and lines (fill), scaled to the largest knot">
              <span
                className="pv-size-bar-bytes"
                style={{ width: `${Math.round((subtreeBytes(node) / maxBytes) * 100)}%` }}
              >
                <span
                  className="pv-size-bar-lines"
                  style={{
                    width: `${Math.round(
                      (subtreeLines(node, byScope) / Math.max(1, maxLines)) * 100,
                    )}%`,
                  }}
                />
              </span>
            </span>
            <span className="pv-size-label">
              {fmtBytes(subtreeBytes(node))}
              {subtreeLines(node, byScope) > 0 && (
                <>
                  {" · "}
                  <span className="pv-size-label-lines">{subtreeLines(node, byScope)} lines</span>
                </>
              )}
            </span>
            <span className="pv-size-cont">
              {node.container_count + node.children.reduce((n, c) => n + c.container_count, 0)}{" "}
              cont.
            </span>
          </>
        )}
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
              byScope={byScope}
              maxBytes={maxBytes}
              maxLines={maxLines}
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
