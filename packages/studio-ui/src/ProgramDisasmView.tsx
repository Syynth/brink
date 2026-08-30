/**
 * The Program Explorer's Disassembly view (#3339 phase 3).
 *
 * A container rail (knots → stitches → their anonymous `c-N` children —
 * the stamps' spelling, and the rows a paused position actually sits in)
 * and the selected container's bytecode. What makes the dump readable is
 * that EVERY OPERAND RESOLVES to what it means, ghosted at the row's
 * right edge:
 *
 * - `emit_line #4` shows the line it will emit, linking into the Line
 *   tables view — the two views are the same data met from opposite ends;
 * - `get_global torch` shows the LIVE value while paused;
 * - `get_temp 2` shows the followed frame's local, by name;
 * - `jump_if_false 46` names its landing offset in the row's own hex;
 * - `call_external play_se` states the binding contract (fallback body
 *   vs host-required) from the externals table.
 *
 * The resolutions are read-time joins over the model, the lines table and
 * the debug state — nothing here re-derives compiler facts. The operand
 * spellings parsed below are produced by `format_opcode` in
 * `crates/brink-web/src/program_model.rs`; its Rust tests pin them, and a
 * change there fails the tests beside this file.
 *
 * stepi lives HERE, beside the code it steps (the canvas ruling) — the
 * same `ProgramExplorerActions` the panel title bar carried.
 */

import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { KnotNode, LinesTableLine, ProgramModel } from "@brink/wasm-types";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { SourceLink } from "./SourceLink.js";
import { buildSourceIndex, type SourceIndex } from "./source-index.js";
import { useShell } from "@brink/studio-shell";
import { ProgramExplorerActions } from "./ProgramView.js";

interface RuntimePosition {
  container_idx: number;
  offset: number;
}

/** One rail row — a scope container or an anonymous child. */
interface RailRow {
  label: string;
  /** The scope path whose LINE TABLE owns this container's emit_lines. */
  scopePath: string;
  /** True for a scope's child container — its display path is
   *  `scopePath.label`, whether the label is `opts` or `c-2`. */
  isAnon: boolean;
  containerIdx: number;
  depth: number;
  disasm: KnotNode["disasm"];
}

function railRows(model: ProgramModel): RailRow[] {
  const rows: RailRow[] = [];
  const push = (node: KnotNode, depth: number): void => {
    rows.push({
      label: depth === 0 ? node.path : `= ${node.name}`,
      scopePath: node.path,
      isAnon: false,
      containerIdx: node.container_idx,
      depth,
      disasm: node.disasm,
    });
    for (const anon of node.anon ?? []) {
      rows.push({
        label: anon.label,
        scopePath: node.path,
        isAnon: true,
        containerIdx: anon.container_idx,
        depth: depth + 1,
        disasm: anon.disasm,
      });
    }
    for (const child of node.children) push(child, depth + 1);
  };
  for (const knot of model.knots) push(knot, 0);
  return rows;
}

const hex = (n: number): string => `+0x${n.toString(16).padStart(2, "0")}`;

/** A line's text flattened to a preview string — chips become {name}. */
function linePreview(line: LinesTableLine | undefined): string | null {
  const content = line?.content;
  if (content === undefined || content === null) return null;
  if (typeof content === "string") return content;
  let out = "";
  for (const part of content.template) {
    if (typeof part === "string") out += part;
    else if ("slot" in part) {
      const name = line?.slots?.find((s) => s.index === part.slot)?.name;
      out += `{${name ?? `slot ${part.slot}`}}`;
    } else if ("select" in part) out += `{…|…}`;
    else if ("span" in part) out += "[…]";
  }
  return out;
}

export function ProgramDisasmViewInner({
  currentPosition,
  target,
  onRevealLine,
}: {
  currentPosition: RuntimePosition | null;
  /** "Reveal in Program Explorer" (W9) — marks and scrolls an instruction. */
  target: { address: RuntimePosition; nonce: number } | null;
  onRevealLine: (scopePath: string, lineIndex: number) => void;
}) {
  const { commands } = useShell();
  const storeApi = useStudioStoreApi();
  const model = useStudioStore((s) => s.programModel);
  const programLines = useStudioStore((s) => s.programLines);

  // Per-file source index for the provenance column — same cache pattern
  // and invalidation (one compile product) as the Line tables view.
  const fileIndexes = useMemo(() => new Map<string, SourceIndex | null>(), [model]);
  const indexFor = (file: string): SourceIndex | null => {
    let cached = fileIndexes.get(file);
    if (cached === undefined) {
      const source = storeApi.getState()._project?.getSession().getFileSource(file) ?? null;
      cached = source === null ? null : buildSourceIndex(source);
      fileIndexes.set(file, cached);
    }
    return cached;
  };
  const debugState = useStudioStore((s) => s.debugState);
  const selectedFrameIdx = useStudioStore((s) => s.selectedFrameIdx);

  const rows = useMemo(() => (model ? railRows(model) : []), [model]);
  const [selectedIdx, setSelectedIdx] = useState<number | null>(null);

  // Follow: the paused position (or a reveal target) selects its container —
  // the same "the view follows what is being inspected" policy the
  // structure view's highlight already obeys.
  useEffect(() => {
    if (currentPosition !== null && rows.some((r) => r.containerIdx === currentPosition.container_idx)) {
      setSelectedIdx(currentPosition.container_idx);
    }
  }, [currentPosition?.container_idx, rows]);
  useEffect(() => {
    if (target !== null && rows.some((r) => r.containerIdx === target.address.container_idx)) {
      setSelectedIdx(target.address.container_idx);
    }
  }, [target?.nonce, rows]);

  const selected =
    rows.find((r) => r.containerIdx === selectedIdx) ??
    rows.find((r) => r.disasm.length > 0) ??
    rows[0];

  // Followed frame's locals, by slot — `get_temp 2` resolves through this.
  const frame =
    debugState?.call_stack?.[selectedFrameIdx ?? 0] ?? debugState?.call_stack?.[0] ?? null;
  const globalsByName = useMemo(() => {
    const map = new Map<string, string>();
    for (const g of debugState?.globals ?? []) map.set(g.name, g.value);
    return map;
  }, [debugState?.globals]);

  const scopeLines = useMemo(() => {
    if (!selected || !programLines) return null;
    return programLines.scopes.find((s) => s.name === selected.scopePath) ?? null;
  }, [selected, programLines]);

  const targetLineRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    targetLineRef.current?.scrollIntoView?.({ block: "center" });
  }, [target?.nonce, selected?.containerIdx]);

  if (!model || rows.length === 0 || !selected) {
    return (
      <div className="pv-lines-empty state-view-empty">
        <p className="state-view-empty-title">No compiled program</p>
      </div>
    );
  }

  const isCurrentContainer =
    currentPosition !== null && selected.containerIdx === currentPosition.container_idx;
  // The caller a step-out would return to: one frame above the followed one.
  const followedIdx = selectedFrameIdx ?? 0;
  const caller = debugState?.call_stack?.[followedIdx + 1] ?? null;
  const callerRow = caller?.position
    ? rows.find((r) => r.containerIdx === caller.position?.container_idx)
    : null;

  return (
    <div className="pv-lines">
      <div className="pv-lines-rail">
        <div className="pv-lines-rail-title">Containers · {rows.length}</div>
        {rows.map((row) => {
          const isCurrent =
            currentPosition !== null && row.containerIdx === currentPosition.container_idx;
          return (
            <button
              key={row.containerIdx}
              type="button"
              className={
                "pv-lines-scope" +
                (row.depth > 0 ? " pv-lines-scope-stitch" : "") +
                (row.containerIdx === selected.containerIdx ? " active" : "")
              }
              style={row.depth > 1 ? { paddingLeft: 12 + row.depth * 12 } : undefined}
              onClick={() => setSelectedIdx(row.containerIdx)}
            >
              {isCurrent && (
                <span className="pv-current-marker" title="currently executing">
                  ●
                </span>
              )}
              <span className="pv-lines-scope-name">{row.label}</span>
              <span className="pv-lines-scope-count">{row.disasm.length}</span>
            </button>
          );
        })}
      </div>

      <div className="pv-lines-main">
        <div className="pv-lines-head">
          {isCurrentContainer && (
            <span className="pv-current-marker" title="currently executing">
              ●
            </span>
          )}
          <span className="pv-lines-head-name">
            {selected.scopePath}
            {selected.isAnon ? `.${selected.label}` : ""}
          </span>
          {isCurrentContainer && currentPosition !== null && (
            <span className="pv-lines-head-facts">paused at {hex(currentPosition.offset)}</span>
          )}
          {callerRow && caller?.position && (
            <span className="pv-lines-head-facts">
              · called from{" "}
              <button
                type="button"
                className="pv-lines-source-link"
                onClick={() => setSelectedIdx(callerRow.containerIdx)}
              >
                {callerRow.scopePath}
                {callerRow.isAnon ? `.${callerRow.label}` : ""}{" "}
                {hex(caller.position.offset)}
              </button>
            </span>
          )}
          <span className="pv-header-spacer" />
          {!model.debug_info && (
            <span
              className="pv-lines-head-facts"
              title="Enable debug info in Settings ▸ Player ▸ Debugging to map instructions back to source"
            >
              no debug info — provenance off
            </span>
          )}
          <ProgramExplorerActions />
        </div>

        <div className="pv-lines-scroll pv-disasm-body">
          {selected.disasm.length === 0 ? (
            <p className="sv-empty pv-disasm-empty">
              empty container — a weave endpoint with no code of its own
            </p>
          ) : (
            selected.disasm.map((line) => {
              const isCurrent =
                isCurrentContainer && currentPosition !== null && line.offset === currentPosition.offset;
              const isTarget =
                target !== null &&
                selected.containerIdx === target.address.container_idx &&
                line.offset === target.address.offset;
              return (
                <div
                  key={line.offset}
                  ref={isTarget ? targetLineRef : undefined}
                  className={
                    "pv-disasm-row" +
                    (isCurrent ? " pv-current-instruction" : "") +
                    (isTarget ? " pv-target-instruction" : "")
                  }
                >
                  <span className="pv-disasm-offset">
                    {isCurrent ? "▶ " : ""}
                    {hex(line.offset)}
                  </span>
                  <span className="pv-disasm-text">
                    <span className="pv-disasm-op">{line.text.split(" ")[0]}</span>
                    {line.text.includes(" ") ? ` ${line.text.slice(line.text.indexOf(" ") + 1)}` : ""}
                  </span>
                  <span className="pv-disasm-src">
                    {line.src && (
                      <SourceLink
                        file={line.src.file}
                        startByte={line.src.start}
                        endByte={line.src.end}
                        indexFor={indexFor}
                        commands={commands}
                      />
                    )}
                  </span>
                  <Resolution
                    text={line.text}
                    scopeLines={scopeLines}
                    scopePath={selected.scopePath}
                    paused={currentPosition !== null && debugState != null}
                    globalsByName={globalsByName}
                    frameLocals={frame?.locals ?? null}
                    externals={model.externals}
                    onRevealLine={onRevealLine}
                  />
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * The ghosted meaning of one instruction's operand. Spellings come from
 * `format_opcode` — regular by construction, pinned by its Rust tests.
 */
function Resolution({
  text,
  scopeLines,
  scopePath,
  paused,
  globalsByName,
  frameLocals,
  externals,
  onRevealLine,
}: {
  text: string;
  scopeLines: { lines: LinesTableLine[] } | null;
  scopePath: string;
  paused: boolean;
  globalsByName: Map<string, string>;
  frameLocals: { slot: number; name: string; value: unknown }[] | null;
  externals: ProgramModel["externals"];
  onRevealLine: (scopePath: string, lineIndex: number) => void;
}) {
  const emit = /^emit_line #(\d+)\b/.exec(text);
  if (emit) {
    const idx = Number(emit[1]);
    const preview = linePreview(scopeLines?.lines.find((l) => l.index === idx));
    return (
      <span className="pv-disasm-res">
        {preview !== null && (
          <span className="pv-disasm-res-line">
            “{preview.length > 60 ? `${preview.slice(0, 60)}…` : preview}”
          </span>
        )}{" "}
        <button
          type="button"
          className="pv-lines-source-link"
          title="Open in the Line tables view"
          onClick={() => onRevealLine(scopePath, idx)}
        >
          line ›
        </button>
      </span>
    );
  }

  const global = /^(?:get_global|set_global|push_var_pointer) (\S+)/.exec(text);
  if (global && paused) {
    const value = globalsByName.get(global[1]);
    if (value !== undefined) {
      return <span className="pv-disasm-res">= {value} now</span>;
    }
    return null;
  }

  const temp = /^(?:get_temp|set_temp|get_temp_raw) (\d+)/.exec(text);
  if (temp && paused && frameLocals) {
    const local = frameLocals.find((l) => l.slot === Number(temp[1]));
    if (local) {
      return (
        <span className="pv-disasm-res">
          = {local.name}: {typeof local.value === "string" ? local.value : JSON.stringify(local.value)}
        </span>
      );
    }
    return null;
  }

  const jump = /^(?:jump|jump_if_false) (\d+)/.exec(text);
  if (jump) {
    return <span className="pv-disasm-res">→ {hex(Number(jump[1]))}</span>;
  }

  const ext = /^call_external (\S+)/.exec(text);
  if (ext) {
    const external = externals.find((e) => e.name === ext[1]);
    if (external === undefined) return null;
    return external.fallback ? (
      <span className="pv-disasm-res pv-disasm-res-fallback">fallback body if unbound</span>
    ) : (
      <span className="pv-disasm-res">host binding required</span>
    );
  }

  return null;
}

export const ProgramDisasmView = memo(ProgramDisasmViewInner);
