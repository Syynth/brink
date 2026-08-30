/**
 * The Program Explorer's Size view (#3339 phase 4) — a squarified treemap
 * of where the `.inkb` bytes go.
 *
 * The numbers are REAL on-disk bytes from the file's own offset table
 * (`size_report_of`), never estimates: the sections plus the header sum to
 * the file, and "shipping only" re-flows against an exact re-serialization
 * without the DebugInfo section — what a release export actually produces.
 *
 * One nuance is stated rather than hidden: inside the bytecode block,
 * knots are proportioned by their CONTENT bytes (the #3342 subtree
 * rollups), while the block's own size is the on-disk section — content
 * plus encoding. The footer says so.
 *
 * Color grammar as taught by the other views: blue bytecode, sky line
 * tables, mauve debug info (dashed — the part a shipping export strips),
 * neutral for definitions. Depth steps the same hue lighter; no new
 * palette.
 */

import { memo, useMemo, useState } from "react";
import type { ProgramModel, SizeReport } from "@brink/wasm-types";
import { useStudioStore } from "./StoreContext.js";
import { squarify } from "./treemap.js";

type Tone = "bytecode" | "lines" | "debug" | "other";

interface SizeNode {
  key: string;
  label: string;
  bytes: number;
  tone: Tone;
  children?: SizeNode[];
}

const fmtBytes = (n: number): string =>
  n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`;

/** Knot content bytes, subtree — mirrors the structure view's rollup. */
function knotBytes(model: ProgramModel): SizeNode[] {
  return model.knots.map((k) => {
    const subtree =
      k.byte_size +
      k.children.reduce((s, c) => s + c.byte_size, 0) +
      (k.anon ?? []).reduce((s, a) => s + a.byte_size, 0) +
      k.children.reduce((s, c) => s + (c.anon ?? []).reduce((t, a) => t + a.byte_size, 0), 0);
    return { key: `knot:${k.path}`, label: k.path, bytes: subtree, tone: "bytecode" as const };
  });
}

function buildRoot(
  report: SizeReport,
  model: ProgramModel | null,
  includeDebug: boolean,
): SizeNode {
  const section = (kind: string): number =>
    report.sections.find((s) => s.kind === kind)?.bytes ?? 0;

  const children: SizeNode[] = [];
  const bytecode = section("Containers");
  if (bytecode > 0) {
    children.push({
      key: "bytecode",
      label: "bytecode",
      bytes: bytecode,
      tone: "bytecode",
      children: model ? knotBytes(model) : undefined,
    });
  }
  const lines = section("LineTables");
  if (lines > 0) {
    children.push({
      key: "lines",
      label: "line tables",
      bytes: lines,
      tone: "lines",
      children: report.line_scopes.map((s) => ({
        key: `scope:${s.name ?? "(root)"}`,
        label: s.name ?? "(root)",
        bytes: s.bytes,
        tone: "lines" as const,
      })),
    });
  }
  if (includeDebug && report.debug > 0) {
    children.push({ key: "debug", label: "debug info", bytes: report.debug, tone: "debug" });
  }
  // Everything else, header included: the program's definitions and tables.
  const accounted = bytecode + lines + (includeDebug ? report.debug : 0);
  const rest: SizeNode[] = report.sections
    .filter((s) => !["Containers", "LineTables", "DebugInfo"].includes(s.kind))
    .filter((s) => s.bytes > 0)
    .map((s) => ({ key: `sec:${s.kind}`, label: s.kind, bytes: s.bytes, tone: "other" as const }));
  rest.push({ key: "sec:header", label: "header", bytes: report.header, tone: "other" });
  const restTotal = rest.reduce((s, n) => s + n.bytes, 0);
  if (restTotal > 0) {
    children.push({
      key: "defs",
      label: "definitions & tables",
      bytes: restTotal,
      tone: "other",
      children: rest,
    });
  }
  const total = includeDebug ? report.total : report.shipping;
  void accounted;
  return { key: "root", label: "program", bytes: total, tone: "other", children };
}

/** Same-hue depth stepping: alpha by rank within the parent. */
const TONE_RGB: Record<Tone, string> = {
  bytecode: "var(--bs-accent-rgb, 137 180 250)",
  lines: "137 220 235",
  debug: "203 166 247",
  other: "var(--bs-fg-muted-rgb, 108 112 134)",
};

export function ProgramSizeViewInner() {
  const report = useStudioStore((s) => s.programSize);
  const model = useStudioStore((s) => s.programModel);
  const [includeDebug, setIncludeDebug] = useState(true);
  const [zoom, setZoom] = useState<string | null>(null);

  const root = useMemo(
    () => (report ? buildRoot(report, model, includeDebug) : null),
    [report, model, includeDebug],
  );
  const focus =
    zoom === null ? root : (root?.children?.find((c) => c.key === zoom) ?? root);

  const rects = useMemo(() => {
    const children = focus?.children ?? [];
    return squarify(
      children.map((c) => ({ key: c.key, value: c.bytes })),
      0,
      0,
      100,
      100,
    );
  }, [focus]);

  if (!report || !root || !focus) {
    return (
      <div className="pv-lines-empty state-view-empty">
        <p className="state-view-empty-title">No size report</p>
        <p className="state-view-empty-hint">Recompile to measure this program.</p>
      </div>
    );
  }

  const hasDebug = report.debug > 0;

  return (
    <div className="pv-size">
      <div className="pv-lines-head">
        <button
          type="button"
          className="pv-lines-source-link pv-size-crumb"
          onClick={() => setZoom(null)}
          disabled={zoom === null}
        >
          program
        </button>
        {zoom !== null && <span className="pv-size-crumb-sep">›</span>}
        {zoom !== null && <span className="pv-lines-head-name">{focus.label}</span>}
        <span className="pv-lines-head-facts">
          {fmtBytes(includeDebug ? report.total : report.shipping)}
          {hasDebug &&
            includeDebug &&
            ` = ${fmtBytes(report.shipping)} shipping + ${fmtBytes(report.debug)} debug info (${Math.round(
              (report.debug / report.total) * 100,
            )}%)`}
        </span>
        <span className="pv-header-spacer" />
        {hasDebug ? (
          <span className="pv-seg" role="tablist" aria-label="What the treemap counts">
            <button
              type="button"
              className={"pv-seg-item" + (includeDebug ? " active" : "")}
              onClick={() => setIncludeDebug(true)}
            >
              with debug info
            </button>
            <button
              type="button"
              className={"pv-seg-item" + (!includeDebug ? " active" : "")}
              onClick={() => setIncludeDebug(false)}
            >
              shipping only
            </button>
          </span>
        ) : (
          <span className="pv-lines-head-facts">no debug info — this IS the shipping size</span>
        )}
      </div>

      <div className="pv-size-map">
        {rects.map((rect) => {
          const node = focus.children?.find((c) => c.key === rect.key);
          if (!node) return null;
          const rank = (focus.children ?? [])
            .filter((c) => c.bytes > 0)
            .sort((a, b) => b.bytes - a.bytes)
            .findIndex((c) => c.key === node.key);
          const alpha = Math.max(0.1, 0.26 - rank * 0.03);
          const pct = Math.round((node.bytes / Math.max(1, focus.bytes)) * 100);
          return (
            <button
              key={rect.key}
              type="button"
              className={"pv-size-block" + (node.tone === "debug" ? " pv-size-block-debug" : "")}
              style={{
                left: `${rect.x}%`,
                top: `${rect.y}%`,
                width: `${rect.w}%`,
                height: `${rect.h}%`,
                background: `rgb(${TONE_RGB[node.tone]} / ${alpha})`,
                borderColor: `rgb(${TONE_RGB[node.tone]} / 0.55)`,
              }}
              title={`${node.label} — ${fmtBytes(node.bytes)} · ${pct}%`}
              onClick={() => {
                if (zoom === null && node.children && node.children.length > 0) {
                  setZoom(node.key);
                }
              }}
            >
              <span className="pv-size-block-label">{node.label}</span>
              <span className="pv-size-block-bytes">
                {fmtBytes(node.bytes)} · {pct}%
              </span>
            </button>
          );
        })}
      </div>

      <div className="pv-lines-footer">
        <span className="pv-size-key pv-size-key-bytecode">■ bytecode</span>
        <span className="pv-size-key pv-size-key-lines">■ line tables</span>
        {hasDebug && includeDebug && (
          <span className="pv-size-key pv-size-key-debug">▨ debug info — stripped from a shipping export</span>
        )}
        <span className="pv-size-key">■ definitions &amp; tables</span>
        <span className="pv-header-spacer" />
        <span>
          area ∝ on-disk bytes · knot blocks are content bytes within their section
        </span>
      </div>
    </div>
  );
}

export const ProgramSizeView = memo(ProgramSizeViewInner);
