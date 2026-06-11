import { memo, useState } from "react";
import type { KnotNode } from "@brink/wasm-types";
import { useShell } from "@brink/studio-shell";
import { useStudioStore } from "./StoreContext.js";
import { OPEN_COMPILED_OUTPUT_COMMAND_ID } from "./CompiledOutputDocument.js";

/**
 * Program Explorer — a structured, navigable view of the *compiled* program
 * (static), distinct from the runtime State view.
 *
 * Tables for globals / lists / externals plus a knot → stitch tree you drill
 * into for flags, path hash, and name-resolved bytecode. The raw `.inkt`
 * dump lives in the read-only Compiled Output editor document (issue #91,
 * spec §4) — the toolbar button opens it via `program.openCompiledOutput`.
 */
function ProgramViewInner() {
  const model = useStudioStore((s) => s.programModel);
  const { commands } = useShell();

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
            model.knots.map((k) => <KnotRow key={k.path} node={k} depth={0} />)
          )}
        </Section>
      </div>
    </div>
  );
}

// ── Knot/stitch tree row ────────────────────────────────────────────

function KnotRow({ node, depth }: { node: KnotNode; depth: number }) {
  const [open, setOpen] = useState(false);
  const indent = 6 + depth * 12;
  return (
    <div className="pv-knot">
      <button
        type="button"
        className="pv-knot-header"
        style={{ paddingLeft: indent }}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <span className="pv-chevron">{open ? "▾" : "▸"}</span>
        <span className={"pv-knot-name" + (node.kind === "stitch" ? " pv-stitch" : "")}>
          {node.name}
        </span>
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
            <pre className="pv-disasm">{node.disasm.join("\n")}</pre>
          )}
          {node.children.map((c) => (
            <KnotRow key={c.path} node={c} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  );
}

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
