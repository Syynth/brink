/**
 * TODOs — tool window over ink `TODO:` author notes (#3050, ruled
 * 2026-08-23, docs/decision-log.md "TODO feature").
 *
 * Data source: the compile slice's `diagnosticsList`, filtered to the E189
 * code the lowering pass emits per `AUTHOR_WARNING` — no dedicated query;
 * the panel derives everything (note text, line, containing knot/stitch)
 * from diagnostics + the outline it already has. Rows dispatch
 * `editor.reveal` like Problems rows; group headers navigate too.
 *
 * Lifetime is existence-in-source: a note that disappears from the next
 * compile's diagnostics is kept on screen briefly with the `is-leaving`
 * class (strikethrough + fade, styled in todos.css) and then dropped —
 * there is no completion state to persist.
 */

import { memo, useEffect, useMemo, useRef, useState } from "react";
import { EDITOR_REVEAL_COMMAND_ID, useShell } from "@brink/studio-shell";
import { lineColAt } from "@brink-lang/editor";
import type { Diagnostic, DocumentSymbol, FileOutline } from "@brink/wasm-types";
import { useStudioStore } from "./StoreContext.js";
import { diagnosticLocation } from "./ProblemsView.js";
import { FilterIcon, GroupByFileIcon } from "./icons.js";

/** The diagnostic code lowering assigns to `TODO:` author notes. */
export const TODO_DIAGNOSTIC_CODE = "E189";

/**
 * A leading `(tag)` on a note, with an optional `:` after it.
 *
 * `TODO(audio): mix this down` reaches here as `(audio): mix this down` —
 * the parser already takes everything after `TODO` to end of line, so the
 * tag needs no language support, only splitting.
 */
const TODO_TAG_RE = /^\(\s*([^)]*?)\s*\)\s*:?\s*/;

/**
 * Split a leading `(tag)` off a note's text.
 *
 * `()` and `(   )` are NOT tags: an empty chip would be unlabelled and
 * unfilterable, so the parens stay as written text instead.
 */
export function splitTodoTag(text: string): { tag: string | null; text: string } {
  const m = TODO_TAG_RE.exec(text);
  if (m === null) return { tag: null, text };
  const tag = (m[1] ?? "").trim();
  if (tag === "") return { tag: null, text };
  return { tag, text: text.slice(m[0].length) };
}

/** Every tag present, in first-appearance order — the chip row's contents. */
export function todoTags(items: readonly TodoItem[]): string[] {
  const seen: string[] = [];
  for (const item of items) {
    if (item.tag !== null && !seen.includes(item.tag)) seen.push(item.tag);
  }
  return seen;
}

/**
 * Notes carrying any selected tag. An empty selection means no filter —
 * every note shows, including untagged ones.
 */
export function filterTodosByTag(
  items: readonly TodoItem[],
  selected: readonly string[],
): TodoItem[] {
  if (selected.length === 0) return [...items];
  return items.filter((i) => i.tag !== null && selected.includes(i.tag));
}

// ── Pure helpers (unit-tested) ──────────────────────────────────────

export interface TodoItem {
  file: string;
  start: number;
  end: number;
  /** The note's text, minus the `TODO:` prefix AND any leading `(tag)`. */
  text: string;
  /** The `TODO(tag):` tag, or null when the note carries none. */
  tag: string | null;
  /** 1-based line when the file's source is available, else null. */
  line: number | null;
  /** Qualified containing symbol (`knot` / `knot.stitch`); null = file level. */
  container: string | null;
}

export interface TodoContainerGroup {
  container: string | null;
  items: TodoItem[];
}

export interface TodoFileGroup {
  file: string;
  /** Symbols start offset for header navigation (0 = top of file). */
  groups: TodoContainerGroup[];
  count: number;
}

/** Deepest knot/stitch (by `full_start..full_end`) containing `offset`.
 *  Exported for the Search panel's card headers (same lookup, spec PR C). */
export function containerAt(symbols: readonly DocumentSymbol[], offset: number): string | null {
  for (const sym of symbols) {
    if (sym.kind !== "knot" && sym.kind !== "stitch" && sym.kind !== "function") continue;
    if (offset < sym.full_start || offset >= sym.full_end) continue;
    const inner = containerAt(sym.children, offset);
    return inner === null ? sym.name : `${sym.name}.${inner}`;
  }
  return null;
}

/**
 * Resolve E189 diagnostics into display items. `getSource` is consulted
 * once per file (null = source unavailable → no line number). Order is
 * preserved from `diagnostics` (the canonical file/offset sort).
 */
export function collectTodoItems(
  diagnostics: readonly Diagnostic[],
  outline: readonly FileOutline[],
  getSource: (file: string) => string | null,
): TodoItem[] {
  const outlineByFile = new Map(outline.map((f) => [f.path, f.symbols]));
  const sources = new Map<string, string | null>();
  const items: TodoItem[] = [];
  for (const d of diagnostics) {
    if (d.code !== TODO_DIAGNOSTIC_CODE) continue;
    let text = sources.get(d.file);
    if (text === undefined) {
      text = getSource(d.file);
      sources.set(d.file, text);
    }
    items.push({
      file: d.file,
      start: d.start,
      end: d.end,
      ...splitTodoTag(d.message.replace(/^TODO:?\s*/, "")),
      line: text === null ? null : lineColAt(text, d.start).line,
      container: containerAt(outlineByFile.get(d.file) ?? [], d.start),
    });
  }
  return items;
}

/** Case-insensitive filter over note text, file, and container. */
export function matchesTodoFilter(item: TodoItem, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (q === "") return true;
  return (
    item.text.toLowerCase().includes(q) ||
    item.file.toLowerCase().includes(q) ||
    (item.container ?? "").toLowerCase().includes(q)
  );
}

/**
 * Group items by file → containing symbol. Items are expected in the
 * canonical (file, offset) order; file-level notes precede knot notes
 * within a file exactly when they do in the source.
 */
export function groupTodoItems(items: readonly TodoItem[]): TodoFileGroup[] {
  const files: TodoFileGroup[] = [];
  for (const item of items) {
    let file = files[files.length - 1];
    if (!file || file.file !== item.file) {
      file = { file: item.file, groups: [], count: 0 };
      files.push(file);
    }
    let group = file.groups[file.groups.length - 1];
    if (!group || group.container !== item.container) {
      group = { container: item.container, items: [] };
      file.groups.push(group);
    }
    group.items.push(item);
    file.count++;
  }
  return files;
}

/**
 * Stable identity for exit-animation diffing: file + line (JSON key per
 * house rule). Ruled 2026-08-23: keying by LOCATION, not text, so editing
 * a note's wording never churns the panel — the row just updates in place;
 * only deleting the line itself counts as removal. Two notes can't share a
 * line, so the pair is unique; a source-less file falls back to the offset.
 */
export function todoKey(item: TodoItem): string {
  return JSON.stringify([item.file, item.line ?? `@${item.start}`]);
}

/** Key every item by its location. */
export function keyTodoItems(items: readonly TodoItem[]): Map<string, TodoItem> {
  const out = new Map<string, TodoItem>();
  for (const item of items) out.set(todoKey(item), item);
  return out;
}

// ── Strip badge ─────────────────────────────────────────────────────

/** TODO count bubble on the tool-window strip; hidden at zero. */
export function TodosBadge() {
  const count = useStudioStore(
    (s) => s.diagnosticsList.filter((d) => d.code === TODO_DIAGNOSTIC_CODE).length,
  );
  if (count === 0) return null;
  return (
    <span className="shell-strip-badge is-todo">{count > 99 ? "99+" : count}</span>
  );
}

// ── View ────────────────────────────────────────────────────────────

interface LeavingTodo {
  key: string;
  item: TodoItem;
  /** Epoch ms when this entry drops out. */
  expiresAt: number;
}

/** How long a removed note lingers (strikethrough) before dropping out. */
const LEAVE_MS = 1400;

/**
 * TODOs controls, rendered by the shell in the panel's chrome header
 * (`ToolWindowDescriptor.actions`) — the same seam Problems uses, and the
 * reason this state lives in the store rather than in the view.
 */
function TodosActionsInner() {
  const filterOpen = useStudioStore((s) => s.todosFilterOpen);
  const grouped = useStudioStore((s) => s.todosGrouped);
  const toggleFilter = useStudioStore((s) => s.toggleTodosFilter);
  const toggleGrouped = useStudioStore((s) => s.toggleTodosGrouped);
  const selected = useStudioStore((s) => s.todosSelectedTags);

  return (
    <>
      <button
        type="button"
        className={"brink-binder-tool" + (filterOpen ? " active" : "")}
        aria-pressed={filterOpen}
        title="Filter TODOs"
        aria-label="Filter TODOs"
        onClick={toggleFilter}
      >
        <FilterIcon />
        {/* A closed filter that is still narrowing the list would be a
            panel hiding notes with no visible cause. Closing clears the
            selection, so this count is only ever a live one. */}
        {selected.length > 0 && <span className="todos-filter-count">{selected.length}</span>}
      </button>
      <button
        type="button"
        className={"brink-binder-tool" + (grouped ? " active" : "")}
        aria-pressed={grouped}
        title={grouped ? "Show as a flat list" : "Group by file"}
        aria-label="Group by file"
        onClick={toggleGrouped}
      >
        <GroupByFileIcon />
      </button>
    </>
  );
}

export const TodosActions = memo(TodosActionsInner);

function TodosViewInner() {
  const diagnostics = useStudioStore((s) => s.diagnosticsList);
  const outline = useStudioStore((s) => s.outline);
  const project = useStudioStore((s) => s._project);
  const { commands } = useShell();
  const filterOpen = useStudioStore((s) => s.todosFilterOpen);
  const selectedTags = useStudioStore((s) => s.todosSelectedTags);
  const grouped = useStudioStore((s) => s.todosGrouped);
  const toggleTag = useStudioStore((s) => s.toggleTodoTag);
  const [query, setQuery] = useState("");

  // The text filter shares the collapsible row with the chips, so it
  // shares their rule too: a closed row leaves nothing narrowing the list.
  useEffect(() => {
    if (!filterOpen) setQuery("");
  }, [filterOpen]);
  const [leaving, setLeaving] = useState<LeavingTodo[]>([]);
  const prevKeyed = useRef<Map<string, TodoItem> | null>(null);

  const items = useMemo(
    () =>
      collectTodoItems(diagnostics, outline, (file) => {
        if (!project) return null;
        try {
          return project.getSession().getFileSource(file);
        } catch {
          return null;
        }
      }),
    [diagnostics, outline, project],
  );

  const keyed = useMemo(() => keyTodoItems(items), [items]);

  // Exit animation: a key present last render but absent now lingers as a
  // struck-through row until its own deadline. Existence in source is the
  // state — this is presentation only, nothing is persisted. Each entry
  // carries its expiry so identity-only churn (every recompile delivers a
  // new-but-equal diagnostics array) can neither strike live rows nor
  // cancel a pending drop (pinned by todos-leaving.test).
  useEffect(() => {
    const prev = prevKeyed.current;
    prevKeyed.current = keyed;
    if (!prev) return;
    const removed: LeavingTodo[] = [];
    for (const [key, item] of prev) {
      if (!keyed.has(key)) {
        removed.push({ key, item, expiresAt: Date.now() + LEAVE_MS });
      }
    }
    setLeaving((cur) => {
      // A note that reappeared (undo) stops leaving; fresh removals join.
      const kept = cur.filter((l) => !keyed.has(l.key) && !removed.some((r) => r.key === l.key));
      if (removed.length === 0 && kept.length === cur.length) return cur;
      return [...kept, ...removed];
    });
  }, [keyed]);

  // Purge each entry on its own deadline.
  useEffect(() => {
    if (leaving.length === 0) return;
    const next = Math.min(...leaving.map((l) => l.expiresAt));
    const timer = setTimeout(
      () => {
        const now = Date.now();
        setLeaving((cur) => cur.filter((l) => l.expiresAt > now));
      },
      Math.max(0, next - Date.now()),
    );
    return () => clearTimeout(timer);
  }, [leaving]);

  // Live and leaving items merged in (file, offset) order, then filtered
  // and grouped — a leaving row holds its old place in its old group.
  /** Every note on screen, before the tag filter — the chip row's source.
   *  Chips come from the UNFILTERED set so selecting one cannot make the
   *  others disappear, which would strand the selection. */
  const visible = useMemo(() => {
    const merged = [...items, ...leaving.map((l) => l.item)];
    merged.sort((a, b) => (a.file < b.file ? -1 : a.file > b.file ? 1 : a.start - b.start));
    return merged.filter((i) => matchesTodoFilter(i, query));
  }, [items, leaving, query]);

  const tags = useMemo(() => todoTags(visible), [visible]);
  const shown = useMemo(
    () => filterTodosByTag(visible, selectedTags),
    [visible, selectedTags],
  );
  const groups = useMemo(() => groupTodoItems(shown), [shown]);

  const reveal = (file: string, start: number, end: number) => {
    commands.dispatch(
      EDITOR_REVEAL_COMMAND_ID,
      diagnosticLocation({ file, start, end, message: "", severity: "Info" }),
    );
  };

  const isLeavingItem = (item: TodoItem) => leaving.some((l) => l.item === item);

  /** One note. Shared by the grouped and flat lists so they cannot drift.
   *  The tag renders as its own chip rather than staying in the text —
   *  `(audio)` repeated down a column is noise, and the chip is also what
   *  the filter is keyed on. */
  const row = (item: TodoItem, showFile = false) => (
    <button
      type="button"
      className={"todos-row" + (isLeavingItem(item) ? " is-leaving" : "")}
      onClick={() => reveal(item.file, item.start, item.end)}
      title={item.tag === null ? item.text : `(${item.tag}) ${item.text}`}
    >
      <span className="todos-mark" aria-hidden="true" />
      {item.tag !== null && <span className="todos-row-tag">{item.tag}</span>}
      <span className="todos-text">{item.text === "" ? "TODO" : item.text}</span>
      {showFile && <span className="todos-row-file">{item.file}</span>}
      {item.line !== null && <span className="todos-line">:{item.line}</span>}
    </button>
  );

  return (
    <div className="todos-view">
      {filterOpen && (
        <div className="todos-filter">
          <input
            type="search"
            className="todos-filter-input"
            placeholder="Filter TODOs…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label="Filter TODOs"
          />
          {tags.length > 0 && (
            <div className="todos-tags" role="group" aria-label="Filter by tag">
              {tags.map((tag) => {
                const on = selectedTags.includes(tag);
                return (
                  <button
                    key={tag}
                    type="button"
                    className={"todos-tag" + (on ? " on" : "")}
                    aria-pressed={on}
                    onClick={() => toggleTag(tag)}
                  >
                    {tag}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      )}
      {groups.length === 0 ? (
        <p className="todos-empty">
          {query.trim() === "" && selectedTags.length === 0 ? "No TODOs" : "No matching TODOs"}
        </p>
      ) : !grouped ? (
        <div className="todos-list">
          <ul className="todos-items">
            {shown.map((item, i) => (
              <li key={`${item.file}:${item.start}:${i}`}>{row(item, true)}</li>
            ))}
          </ul>
        </div>
      ) : (
        <div className="todos-list">
          {groups.map((fileGroup) => (
            <section key={fileGroup.file} className="todos-file-group">
              <button
                type="button"
                className="todos-file-header"
                onClick={() => reveal(fileGroup.file, 0, 0)}
              >
                <span className="todos-file-name">{fileGroup.file}</span>
                <span className="todos-count">{fileGroup.count}</span>
              </button>
              {fileGroup.groups.map((group, gi) => (
                <div key={`${group.container ?? ""}:${gi}`} className="todos-container-group">
                  {group.container !== null && (
                    <button
                      type="button"
                      className="todos-container-header"
                      onClick={() => {
                        const first = group.items[0];
                        if (first) reveal(first.file, first.start, first.end);
                      }}
                    >
                      {group.container}
                    </button>
                  )}
                  <ul className="todos-items">
                    {group.items.map((item, ii) => (
                      <li key={`${item.start}:${ii}`}>{row(item)}</li>
                    ))}
                  </ul>
                </div>
              ))}
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

export const TodosView = memo(TodosViewInner);
