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
  /** One sentence of what this block IS, shown when zoomed to a leaf. */
  detail?: string;
  /** A jump into the view that owns this block's contents. */
  jump?: { label: string; view: "disasm" | "lines"; containerIdx?: number; scopePath?: string };
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
    const containers =
      k.container_count + k.children.reduce((n, c) => n + c.container_count, 0);
    const instructions =
      k.disasm.length +
      (k.anon ?? []).reduce((n, a) => n + a.disasm.length, 0) +
      k.children.reduce(
        (n, c) => n + c.disasm.length + (c.anon ?? []).reduce((m, a) => m + a.disasm.length, 0),
        0,
      );
    return {
      key: `knot:${k.path}`,
      label: k.path,
      bytes: subtree,
      tone: "bytecode" as const,
      detail: `${instructions} instructions across ${containers} container${containers === 1 ? "" : "s"}`,
      jump: { label: "open in Disassembly ›", view: "disasm" as const, containerIdx: k.container_idx },
    };
  });
}

/** What each remaining `.inkb` section IS — one honest sentence. */
const SECTION_DETAIL: Record<string, string> = {
  NameTable: "every interned name — knots, variables, labels — deduplicated",
  Variables: "global declarations with their default values",
  ListDefs: "LIST declarations",
  ListItems: "the items of every LIST, with their ordinals",
  Externals: "EXTERNAL declarations and their fallback bodies' addresses",
  Labels: "weave-label address table",
  ListLiterals: "list literal values referenced by bytecode",
  AddressPaths: "stamped container paths — what saves and the graph address",
  LiteralPool: "content-hash-deduplicated constants (PushLiteral)",
  StructShapes: "STRUCT declarations — shape id and field names",
  EffectRows: "per-knot factored effect rows (resume scheduling)",
  Visibility: "the ids of every #@private definition",
  FrameShapes: "declared frame shapes for save validation",
  LineVariantGroups: "ties enumerated alternatives back to one authored line",
  header: "the file preamble and section offset table",
};

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
        detail: undefined,
        jump:
          s.name === null
            ? undefined
            : { label: "open in Line tables ›", view: "lines" as const, scopePath: s.name },
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
    .map((s) => ({
      key: `sec:${s.kind}`,
      label: s.kind,
      bytes: s.bytes,
      tone: "other" as const,
      detail: SECTION_DETAIL[s.kind],
    }));
  rest.push({
    key: "sec:header",
    label: "header",
    bytes: report.header,
    tone: "other",
    detail: SECTION_DETAIL.header,
  });
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

export function ProgramSizeViewInner({
  onOpenDisasm,
  onOpenLines,
}: {
  /** Jump to a container in the Disassembly view. */
  onOpenDisasm: (containerIdx: number) => void;
  /** Jump to a scope in the Line tables view. */
  onOpenLines: (scopePath: string) => void;
}) {
  const report = useStudioStore((s) => s.programSize);
  const model = useStudioStore((s) => s.programModel);
  const [includeDebug, setIncludeDebug] = useState(true);
  // Two levels of zoom: a GROUP fills the map, then a CHILD fills the
  // group — both pure CSS transitions on always-mounted blocks. Leaves are
  // enterable too (maintainer, 2026-08-30): a legitimately tiny block is
  // exactly the one you zoom to read.
  const [zoom, setZoom] = useState<{ group: string; child: string | null } | null>(null);

  const root = useMemo(
    () => (report ? buildRoot(report, model, includeDebug) : null),
    [report, model, includeDebug],
  );

  // Top-level rects in container %, and each group's children in the
  // group's own LOCAL % — every block is mounted all the time, so a zoom
  // is nothing but a CSS transition: the group animates to fill the
  // container and its children scale with it (maintainer, 2026-08-30).
  const groups = useMemo(() => {
    const children = root?.children ?? [];
    const rects = squarify(
      children.map((c) => ({ key: c.key, value: c.bytes })),
      0,
      0,
      100,
      100,
    );
    return rects.flatMap((rect) => {
      const node = children.find((c) => c.key === rect.key);
      if (!node) return [];
      // Children plus the honest remainder: a section's on-disk bytes
      // exceed its children's content bytes by real encoding/framing —
      // shown as its own quiet block rather than silently absorbed.
      const kids = [...(node.children ?? [])];
      const kidSum = kids.reduce((s2, k) => s2 + k.bytes, 0);
      if (kids.length > 0 && node.bytes - kidSum > 0) {
        kids.push({
          key: `${node.key}:encoding`,
          label: "encoding",
          bytes: node.bytes - kidSum,
          tone: node.tone,
        });
      }
      const inner = squarify(
        kids.map((k) => ({ key: k.key, value: k.bytes })),
        0,
        0,
        100,
        100,
      );
      return [{ node, rect, kids, inner }];
    });
  }, [root]);

  if (!report || !root) {
    return (
      <div className="pv-lines-empty state-view-empty">
        <p className="state-view-empty-title">No size report</p>
        <p className="state-view-empty-hint">Recompile to measure this program.</p>
      </div>
    );
  }

  const hasDebug = report.debug > 0;
  const zoomed = groups.find((g) => g.node.key === zoom?.group) ?? null;
  const zoomedChild =
    zoomed !== null && zoom?.child != null
      ? (zoomed.kids.find((k) => k.key === zoom.child) ?? null)
      : null;

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
        {zoomed !== null && <span className="pv-size-crumb-sep">›</span>}
        {zoomed !== null &&
          (zoomedChild !== null ? (
            <button
              type="button"
              className="pv-lines-source-link pv-size-crumb"
              onClick={() => setZoom({ group: zoomed.node.key, child: null })}
            >
              {zoomed.node.label}
            </button>
          ) : (
            <span className="pv-lines-head-name">{zoomed.node.label}</span>
          ))}
        {zoomedChild !== null && <span className="pv-size-crumb-sep">›</span>}
        {zoomedChild !== null && (
          <span className="pv-lines-head-name">{zoomedChild.label}</span>
        )}
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
        {groups.map(({ node, rect, kids, inner }) => {
          const isZoomed = zoom?.group === node.key;
          const dimmed = zoom !== null && !isZoomed;
          const pct = Math.round((node.bytes / Math.max(1, root.bytes)) * 100);
          return (
            <div
              key={node.key}
              className={
                "pv-size-group" +
                (node.tone === "debug" ? " pv-size-block-debug" : "") +
                (isZoomed ? " pv-size-zoomed" : "") +
                (dimmed ? " pv-size-dimmed" : "")
              }
              style={{
                left: `${isZoomed ? 0 : rect.x}%`,
                top: `${isZoomed ? 0 : rect.y}%`,
                width: `${isZoomed ? 100 : rect.w}%`,
                height: `${isZoomed ? 100 : rect.h}%`,
                background: `rgb(${TONE_RGB[node.tone]} / 0.10)`,
                borderColor: `rgb(${TONE_RGB[node.tone]} / 0.55)`,
              }}
            >
              <button
                type="button"
                className="pv-size-group-head"
                title={`${node.label} — ${fmtBytes(node.bytes)} · ${pct}%`}
                // One level at a time in BOTH directions: from a leaf,
                // the group head goes up to the group — never straight to
                // root (maintainer, 2026-08-30).
                onClick={() =>
                  setZoom(
                    isZoomed
                      ? zoom?.child != null
                        ? { group: node.key, child: null }
                        : null
                      : { group: node.key, child: null },
                  )
                }
              >
                <span className="pv-size-group-label">{node.label}</span>
                <span className="pv-size-block-bytes">
                  {fmtBytes(node.bytes)} · {pct}%
                </span>
              </button>
              <div className="pv-size-group-body">
                {inner.map((childRect) => {
                  const kid = kids.find((k) => k.key === childRect.key);
                  if (!kid) return null;
                  const rank = kids
                    .filter((k) => k.bytes > 0)
                    .sort((a, b) => b.bytes - a.bytes)
                    .findIndex((k) => k.key === kid.key);
                  const alpha = Math.max(0.08, 0.24 - rank * 0.02);
                  const kidPct = Math.round((kid.bytes / Math.max(1, node.bytes)) * 100);
                  const isEncoding = kid.key.endsWith(":encoding");
                  const isChildZoomed = isZoomed && zoom?.child === kid.key;
                  const childDimmed = isZoomed && zoom?.child != null && !isChildZoomed;
                  return (
                    <button
                      key={kid.key}
                      type="button"
                      className={
                        "pv-size-block" +
                        (isEncoding ? " pv-size-block-encoding" : "") +
                        (isChildZoomed ? " pv-size-zoomed" : "") +
                        (childDimmed ? " pv-size-dimmed" : "")
                      }
                      style={{
                        left: `${isChildZoomed ? 0 : childRect.x}%`,
                        top: `${isChildZoomed ? 0 : childRect.y}%`,
                        width: `${isChildZoomed ? 100 : childRect.w}%`,
                        height: `${isChildZoomed ? 100 : childRect.h}%`,
                        background: `rgb(${TONE_RGB[kid.tone]} / ${alpha})`,
                        borderColor: `rgb(${TONE_RGB[kid.tone]} / 0.4)`,
                      }}
                      title={`${kid.label} — ${fmtBytes(kid.bytes)} · ${kidPct}% of ${node.label}`}
                      // The whole surface zooms, one level at a time: at
                      // root a child hit enters its group; inside a group
                      // it enters the child itself — legitimately tiny
                      // blocks are exactly the ones you zoom to read.
                      onClick={() => {
                        if (!isZoomed) setZoom({ group: node.key, child: null });
                        else if (!isChildZoomed) setZoom({ group: node.key, child: kid.key });
                      }}
                    >
                      <span className="pv-size-block-label">{kid.label}</span>
                      <span className="pv-size-block-bytes">{fmtBytes(kid.bytes)}</span>
                      {/* Revealed by the container query only at leaf-zoom
                          sizes — the same size-aware rule as the labels. */}
                      <span className="pv-size-block-detail">
                        {fmtBytes(kid.bytes)} · {kidPct}% of {node.label}
                        {kid.detail ? ` — ${kid.detail}` : ""}
                        {kid.jump && (
                          <span
                            className="pv-lines-source-link pv-size-jump"
                            role="link"
                            tabIndex={0}
                            onClick={(e) => {
                              e.stopPropagation();
                              if (kid.jump?.view === "disasm" && kid.jump.containerIdx !== undefined)
                                onOpenDisasm(kid.jump.containerIdx);
                              else if (kid.jump?.view === "lines" && kid.jump.scopePath)
                                onOpenLines(kid.jump.scopePath);
                            }}
                          >
                            {kid.jump.label}
                          </span>
                        )}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>
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
          area ∝ on-disk bytes · knot blocks are content bytes; the remainder shows as “encoding”
        </span>
      </div>
    </div>
  );
}

export const ProgramSizeView = memo(ProgramSizeViewInner);
