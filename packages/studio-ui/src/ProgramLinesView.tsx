/**
 * The Program Explorer's Line tables view (#3339 phase 2).
 *
 * The compiled lines, scoped exactly as the compiler scopes them — knots
 * and stitches, the boundary translators think in (`docs/intl-spec.md`
 * §"Line table scoping"). A scope rail on the left, the selected scope's
 * table on the right: index, text, audio, source.
 *
 * Template lines render their structure as CHIPS rather than raw JSON —
 * a mauve chip is a slot (filled at play time; labeled by the slot's
 * name), a sky chip is a select (variant picked by value; the chip shows
 * the variants, the tooltip the keys). That is the point of the view:
 * `EmitLine(5)` in a disassembly and `{torch} torch|torches` in a
 * translator's export are the same line, and here it reads as prose.
 *
 * Source cells are real links — they dispatch the same `editor.reveal`
 * road the Problems panel rows use, so a line round-trips to the text
 * that compiled it.
 */

import { memo, useMemo, useState } from "react";
import type { LinePart, LinesTableLine, LinesTableScope } from "@brink/wasm-types";
import { EDITOR_REVEAL_COMMAND_ID, useShell } from "@brink/studio-shell";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { buildSourceIndex, type SourceIndex } from "./source-index.js";

/** Root first, knots in appearance order, stitches under their knot. */
function orderScopes(scopes: readonly LinesTableScope[]): LinesTableScope[] {
  const roots: LinesTableScope[] = [];
  const knots: LinesTableScope[] = [];
  const stitchesByKnot = new Map<string, LinesTableScope[]>();
  const orphans: LinesTableScope[] = [];
  for (const scope of scopes) {
    if (!scope.name) roots.push(scope);
    else if (scope.name.includes(".")) {
      const knot = scope.name.split(".")[0];
      stitchesByKnot.set(knot, [...(stitchesByKnot.get(knot) ?? []), scope]);
    } else knots.push(scope);
  }
  const out = [...roots];
  for (const knot of knots) {
    out.push(knot);
    const kids = stitchesByKnot.get(knot.name ?? "");
    if (kids) {
      out.push(...kids);
      stitchesByKnot.delete(knot.name ?? "");
    }
  }
  // A stitch whose knot scope never appeared still gets listed.
  for (const kids of stitchesByKnot.values()) orphans.push(...kids);
  return [...out, ...orphans];
}

/** The rail label for the unnamed root scope. */
const ROOT_LABEL = "(root)";

function scopeLabel(scope: LinesTableScope): string {
  return scope.name ? scope.name : ROOT_LABEL;
}

/** Facts a scope header states: lines, templates, selects, audio refs. */
function scopeFacts(scope: LinesTableScope): {
  lines: number;
  templates: number;
  selects: number;
  audio: number;
} {
  let templates = 0;
  let selects = 0;
  let audio = 0;
  for (const line of scope.lines) {
    if (typeof line.content === "object" && line.content !== null) {
      templates += 1;
      selects += countSelects(line.content.template);
    }
    if (line.audio) audio += 1;
  }
  return { lines: scope.lines.length, templates, selects, audio };
}

function countSelects(parts: readonly LinePart[]): number {
  let n = 0;
  for (const part of parts) {
    if (typeof part === "string") continue;
    if ("select" in part) n += 1;
    else if ("span" in part && part.span.children) n += countSelects(part.span.children);
  }
  return n;
}

export function ProgramLinesViewInner({
  currentScopePath,
}: {
  /** The paused execution's scope path, for the rail's ● marker. */
  currentScopePath: string | null;
}) {
  const { commands } = useShell();
  const storeApi = useStudioStoreApi();
  const programLines = useStudioStore((s) => s.programLines);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  // Per-file source index (line numbers + byte→UTF-16), lazily built and
  // cached for the life of this compile product — a new compile makes a
  // new map, so edited files never serve a stale index. See
  // source-index.ts for why the conversion is not optional: the table's
  // ranges are UTF-8 bytes and the reveal road is UTF-16.
  const fileIndexes = useMemo(
    () => new Map<string, SourceIndex | null>(),
    [programLines],
  );
  const indexFor = (file: string): SourceIndex | null => {
    let cached = fileIndexes.get(file);
    if (cached === undefined) {
      const source = storeApi.getState()._project?.getSession().getFileSource(file) ?? null;
      cached = source === null ? null : buildSourceIndex(source);
      fileIndexes.set(file, cached);
    }
    return cached;
  };

  // The export's scope order is flat compile order, which scatters
  // stitches away from their knots. The rail is a TREE reading: root
  // first, then each knot in its first-appearance order with its own
  // stitches directly beneath it.
  const scopes = useMemo(() => orderScopes(programLines?.scopes ?? []), [programLines]);
  // Default to the first scope that HAS lines — an empty root scope as the
  // landing view would open the feature on a blank table.
  const selected =
    scopes.find((s) => s.id === selectedId) ?? scopes.find((s) => s.lines.length > 0) ?? scopes[0];

  const facts = useMemo(() => (selected ? scopeFacts(selected) : null), [selected]);

  if (!programLines || scopes.length === 0 || !selected || !facts) {
    return (
      <div className="pv-lines-empty state-view-empty">
        <p className="state-view-empty-title">No lines table</p>
        <p className="state-view-empty-hint">
          This compile product predates line-table capture — recompile to populate it.
        </p>
      </div>
    );
  }

  return (
    <div className="pv-lines">
      <div className="pv-lines-rail">
        <div className="pv-lines-rail-title">
          Scopes · {scopes.length} table{scopes.length === 1 ? "" : "s"}
        </div>
        {scopes.map((scope) => {
          const label = scopeLabel(scope);
          const isStitch = Boolean(scope.name?.includes("."));
          const isCurrent = scope.name != null && scope.name === currentScopePath;
          return (
            <button
              key={scope.id}
              type="button"
              className={
                "pv-lines-scope" +
                (isStitch ? " pv-lines-scope-stitch" : "") +
                (scope.id === selected.id ? " active" : "")
              }
              onClick={() => setSelectedId(scope.id)}
            >
              {isCurrent && (
                <span className="pv-current-marker" title="currently executing">
                  ●
                </span>
              )}
              <span className="pv-lines-scope-name">
                {isStitch ? `= ${label.split(".").pop() ?? label}` : label}
              </span>
              <span className="pv-lines-scope-count">{scope.lines.length}</span>
            </button>
          );
        })}
      </div>

      <div className="pv-lines-main">
        <div className="pv-lines-head">
          <span className="pv-lines-head-name">{scopeLabel(selected)}</span>
          <span className="pv-lines-head-facts">
            {facts.lines} line{facts.lines === 1 ? "" : "s"}
            {facts.templates > 0 && ` · ${facts.templates} templates`}
            {facts.selects > 0 && ` · ${facts.selects} selects`}
            {facts.audio > 0 && ` · ${facts.audio} audio ref${facts.audio === 1 ? "" : "s"}`}
          </span>
          <span className="pv-header-spacer" />
          <span className="pv-lines-head-hint">EmitLine(n) in the disassembly points here</span>
        </div>

        <div className="pv-lines-scroll">
          <table className="pv-lines-table">
            <thead>
              <tr>
                <th className="pv-lines-th-idx">#</th>
                <th>Text</th>
                <th className="pv-lines-th-audio">Audio</th>
                <th className="pv-lines-th-source">Source</th>
              </tr>
            </thead>
            <tbody>
              {selected.lines.map((line) => (
                <LineRow key={line.index} line={line} commands={commands} indexFor={indexFor} />
              ))}
            </tbody>
          </table>
        </div>

        <div className="pv-lines-footer">
          <span>
            Chips are template parts — <span className="pv-chip-slot-legend">slots</span> fill at
            play time, <span className="pv-chip-select-legend">selects</span> pick a variant by
            value. Plain rows are fixed text.
          </span>
          <span className="pv-header-spacer" />
          <span className="pv-lines-footer-audio">♪ = audio ref</span>
        </div>
      </div>
    </div>
  );
}

function LineRow({
  line,
  commands,
  indexFor,
}: {
  line: LinesTableLine;
  commands: { dispatch: (id: string, arg?: unknown) => void };
  indexFor: (file: string) => SourceIndex | null;
}) {
  const source = line.source ?? null;
  const index = source ? indexFor(source.file) : null;
  const lineNo = source && index ? index.lineForByte(source.range_start) : null;
  return (
    <tr>
      <td className="pv-lines-idx">{line.index}</td>
      <td className="pv-lines-text">{renderContent(line)}</td>
      <td className="pv-lines-audio">
        {line.audio ? (
          <span className="pv-lines-audio-ref" title={`audio_ref: ${line.audio}`}>
            ♪
          </span>
        ) : (
          <span className="pv-lines-audio-none">·</span>
        )}
      </td>
      <td className="pv-lines-source">
        {source && (
          <button
            type="button"
            className="pv-lines-source-link"
            title={`${source.file} · bytes ${source.range_start}–${source.range_end}`}
            onClick={() =>
              // The same road a Problems row rides (`diagnosticLocation`) —
              // one reveal contract, not a second navigation. The table's
              // ranges are UTF-8 bytes; the road is UTF-16 — convert, or
              // every multibyte character above the target shifts the
              // highlight. A file the session cannot serve (no index)
              // still navigates, best-effort, with the raw offsets.
              commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
                kind: "source",
                file: source.file,
                span:
                  index === null
                    ? { start: source.range_start, end: source.range_end }
                    : {
                        start: index.utf16ForByte(source.range_start),
                        end: index.utf16ForByte(source.range_end),
                      },
              })
            }
          >
            {source.file.split("/").pop()}
            {lineNo !== null ? `:${lineNo}` : ""}
          </button>
        )}
      </td>
    </tr>
  );
}

/** A line's content as prose with chips — never raw JSON. */
function renderContent(line: LinesTableLine): React.ReactNode {
  const content = line.content;
  if (content === undefined || content === null) return <span className="sv-dim">—</span>;
  if (typeof content === "string") return content;
  return renderParts(content.template, line);
}

function renderParts(parts: readonly LinePart[], line: LinesTableLine): React.ReactNode {
  return parts.map((part, i) => {
    if (typeof part === "string") return <span key={i}>{part}</span>;
    if ("slot" in part) {
      const name = line.slots?.find((s) => s.index === part.slot)?.name;
      return (
        <span
          key={i}
          className="pv-chip pv-chip-slot"
          title={`slot ${part.slot}${name ? ` — ${name}` : ""}: filled at play time`}
        >
          {`{${name ?? `slot ${part.slot}`}}`}
        </span>
      );
    }
    if ("select" in part) {
      const variants = part.select.variants.map((v) => Object.values(v)[0] ?? "");
      const keys = part.select.variants.map((v) => Object.keys(v)[0] ?? "");
      return (
        <span
          key={i}
          className="pv-chip pv-chip-select"
          title={`select on slot ${part.select.slot} — ${keys.join(", ")}; default “${part.select.default}”`}
        >
          {[...variants, part.select.default].join("|")}
        </span>
      );
    }
    // An inline markup span: its children read as prose; the span's name
    // rides in the tooltip rather than as clutter in the line.
    return (
      <span key={i} className="pv-line-span" title={`[${part.span.name}] span`}>
        {part.span.children ? renderParts(part.span.children, line) : null}
      </span>
    );
  });
}

export const ProgramLinesView = memo(ProgramLinesViewInner);
