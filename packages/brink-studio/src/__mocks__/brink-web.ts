/**
 * Mock wasm module for testing.
 *
 * Implements the same interface as the real brink-web wasm package
 * but stores files in memory and returns minimal JSON responses.
 * Parses `=== knot ===` and `= stitch` headers to produce outlines.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

export default function init(): Promise<void> {
  return Promise.resolve();
}

interface MockSymbol {
  name: string;
  kind: string;
  start: number;
  end: number;
  full_start: number;
  full_end: number;
  children: MockSymbol[];
}

/**
 * Escape a symbol name for literal use inside a `new RegExp(...)` template.
 *
 * `rename_symbol` has always escaped (its own local `esc`); `delete_symbol`
 * interpolated `knot`/`name` raw until #2641 lifted this to module scope so
 * both use one helper.
 *
 * ⚠ Can a real name reach it? **Not through the studio today.** Both the
 * parser's knot/stitch header rules and the mock's own `parseOutline`
 * (`/^===\s+(\w+)\s*===/`, `/^=\s+(\w+)/`) admit `\w+` only, so every name the
 * outline — and therefore the symbol menu — can hand these ops is
 * metacharacter-free. The escape is defence for the *mock's own* callers:
 * `delete_symbol` takes its `knot`/`stitch` as free strings, and a test (or a
 * future caller) passing `"a.b"` or `"a("` got a silently wrong match or a
 * thrown `SyntaxError` rather than a refusal. Escaping costs nothing and
 * removes the difference between the two sibling ops.
 */
function escapeForRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Parse knot/stitch headers from ink source for outline generation. */
function parseOutline(source: string): MockSymbol[] {
  const symbols: MockSymbol[] = [];
  const lines = source.split("\n");
  let offset = 0;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    const knotMatch = line.match(/^===\s+(\w+)\s*===/);
    if (knotMatch) {
      const name = knotMatch[1]!;
      const nameStart = offset + line.indexOf(name);
      const nameEnd = nameStart + name.length;
      symbols.push({
        name,
        kind: "knot",
        start: nameStart,
        end: nameEnd,
        full_start: offset,
        full_end: 0, // filled in below
        children: [],
      });
    }

    const stitchMatch = line.match(/^=\s+(\w+)/);
    if (stitchMatch && !knotMatch) {
      const name = stitchMatch[1]!;
      const nameStart = offset + line.indexOf(name);
      const nameEnd = nameStart + name.length;
      const parent = symbols[symbols.length - 1];
      if (parent) {
        parent.children.push({
          name,
          kind: "stitch",
          start: nameStart,
          end: nameEnd,
          full_start: offset,
          full_end: 0,
          children: [],
        });
      }
    }

    offset += line.length + 1; // +1 for \n
  }

  // Fill in full_end for each symbol
  for (let i = 0; i < symbols.length; i++) {
    const next = symbols[i + 1];
    symbols[i]!.full_end = next ? next.full_start : source.length;

    const knot = symbols[i]!;
    for (let j = 0; j < knot.children.length; j++) {
      const nextChild = knot.children[j + 1];
      knot.children[j]!.full_end = nextChild ? nextChild.full_start : knot.full_end;
    }
  }

  return symbols;
}

// ── Structural region model (#2577) ──────────────────────────────────
//
// The seven `dispatchSymbolAction` ops (`reorder_stitch`, `reorder_knot`,
// `reorder_stitches`, `reorder_knots`, `move_stitch`, `promote_stitch`,
// `demote_knot`) all do the same thing in the real Rust op
// (`brink_ide::structural_move`): slice the file into knot/stitch *ownership
// regions*, rearrange whole regions, requalify the references that the move
// renamed, and reassemble. These helpers give the mock the same shape of
// model over `parseOutline`'s ranges, so each op below is a rearrangement of
// regions rather than an ad-hoc string edit per op.

/** One knot's text split into the regions a structural op moves: `head` is the
 *  knot header plus its own body up to the first stitch; `stitches` are the
 *  per-stitch ownership regions in document order. */
interface KnotPlan {
  name: string;
  head: string;
  stitches: { name: string; text: string }[];
}

/** Slice `source` into the region plan the structural ops rearrange. */
function planKnots(source: string, knots: MockSymbol[]): KnotPlan[] {
  return knots.map((k) => ({
    name: k.name,
    head: source.slice(k.full_start, k.children[0]?.full_start ?? k.full_end),
    stitches: k.children.map((s) => ({
      name: s.name,
      text: source.slice(s.full_start, s.full_end),
    })),
  }));
}

/** Reassemble a plan into full source, preserving the pre-first-knot preamble
 *  (the real ops preserve it too — a file's leading `INCLUDE`s and `VAR`s must
 *  not move when knots are reordered). */
function renderKnots(source: string, knots: MockSymbol[], plan: KnotPlan[]): string {
  if (knots.length === 0) return source;
  const preamble = source.slice(0, knots[0]!.full_start);
  return preamble + plan.map((k) => k.head + k.stitches.map((s) => s.text).join("")).join("");
}

/**
 * Resolve `order` (a list of names) to indices into `current`, or null when it
 * is not a permutation of it — mirroring `structural_move::resolve_permutation`,
 * whose three rejections (length mismatch, unknown name, repeated name) all
 * surface as the one `invalid reorder` error.
 */
function resolvePermutation(current: string[], order: readonly string[]): number[] | null {
  if (current.length !== order.length) return null;
  const used = new Set<number>();
  const out: number[] = [];
  for (const name of order) {
    const i = current.indexOf(name);
    if (i < 0 || used.has(i)) return null;
    used.add(i);
    out.push(i);
  }
  return out;
}

/**
 * Rewrite every `-> old` / `<- old` reference in `src` to `next`. The mock's
 * stand-in for `structural_move::compute_reference_edits`, which requalifies a
 * moved symbol's references (`src.stitch` → `dest.stitch` on a move, `knot.x` →
 * `x` on a promote, `knot` → `dest.knot` on a demote). The real op resolves
 * references through the analyzer; this matches the divert/thread syntax the
 * studio fixtures use. The negative lookahead keeps `-> knot.stitch` from
 * matching a rewrite of the bare `knot`.
 */
function requalifyReferences(src: string, old: string, next: string): string {
  const esc = old.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return src.replace(new RegExp(`((?:->|<-)\\s*)${esc}(?![\\w.])`, "g"), `$1${next}`);
}

/** `= name` → `=== name ===` on a promoted stitch's first header line
 *  (`structural_move::rewrite_stitch_to_knot_header`). */
function stitchHeaderToKnot(text: string, name: string): string {
  const esc = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return text.replace(new RegExp(`^(\\s*)=\\s*${esc}([^\\n]*)`), `$1=== ${name} ===$2`);
}

/** `=== name ===` → `= name` on a demoted knot's first header line
 *  (`structural_move::rewrite_knot_to_stitch_header`). */
function knotHeaderToStitch(text: string, name: string): string {
  const esc = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return text.replace(new RegExp(`^(\\s*)={2,3}\\s*${esc}\\s*={0,3}([^\\n]*)`), `$1= ${name}$2`);
}

interface MockDoc {
  path: string;
  viewStart: number | null;
  viewEnd: number | null;
}

export class EditorSession {
  private files = new Map<string, string>();
  private activePath = "";
  private docs = new Map<number, MockDoc>();
  private nextDocId = 1;

  update_source(source: string): void {
    if (this.viewStart != null && this.viewEnd != null) {
      const full = this.files.get(this.activePath) ?? "";
      const before = full.slice(0, this.viewStart);
      const after = full.slice(this.viewEnd);
      this.files.set(this.activePath, before + source + after);
      this.viewEnd = this.viewStart + source.length;
    } else {
      this.files.set(this.activePath, source);
    }
  }

  update_file(path: string, source: string): void {
    this.files.set(path, source);
    // Shadowing (issue #2306): a real write at a mounted key wins over the
    // mount, mirroring the real `EditorSession::new` doc's contract.
    this.readOnlyPaths.delete(path);
  }

  /**
   * Mock of the real `remove_file` (issue #2306/#2343): refuses (returns
   * `false`, no mutation) for a read-only (mounted) path, mirroring the
   * Rust-side fence added alongside `list_files`'s flag flip — deleting a
   * mounted file used to be unreachable only because `list_files` excluded
   * it from the Binder.
   */
  remove_file(path: string): boolean {
    if (this.readOnlyPaths.has(path)) return false;
    this.files.delete(path);
    return true;
  }

  private viewStart: number | null = null;
  private viewEnd: number | null = null;

  set_active_file(path: string): boolean {
    if (this.files.has(path)) {
      this.activePath = path;
      this.viewStart = null;
      this.viewEnd = null;
      return true;
    }
    return false;
  }

  set_view_context(start: number, end: number): void {
    this.viewStart = start;
    this.viewEnd = end;
  }

  clear_view_context(): void {
    this.viewStart = null;
    this.viewEnd = null;
  }

  get_view_source(): string {
    const content = this.files.get(this.activePath);
    if (content == null) return JSON.stringify(null);
    if (this.viewStart != null && this.viewEnd != null) {
      return JSON.stringify(content.slice(this.viewStart, this.viewEnd));
    }
    return JSON.stringify(content);
  }

  active_file(): string {
    return this.activePath;
  }

  list_files(): string {
    // Lists read-only (mounted) paths alongside real files, flagged
    // `mounted`, mirroring the real `list_files`'s flag flip (issue
    // #2306/#2343 — superseding #2231's original exclusion, which the
    // ruling found left stdlib neither hidden nor marked read-only).
    return JSON.stringify(
      [...this.files.keys()].map((p) => ({ path: p, mounted: this.readOnlyPaths.has(p) })),
    );
  }

  get_file_source(path: string): string {
    const content = this.files.get(path);
    return JSON.stringify(content ?? null);
  }

  /**
   * Mock of the real `is_read_only` (issue #2306): defaults to `false` for
   * every path — the mock never mounts a stdlib copy on construction (unlike
   * the real `EditorSession::new()`), so nothing is read-only unless a test
   * opts a path in via {@link __mockMarkReadOnlyForTest}.
   */
  is_read_only(path: string): boolean {
    return this.readOnlyPaths.has(path);
  }

  private readonly readOnlyPaths = new Set<string>();

  /**
   * Test-only seam (issue #2306): mark `path` as a mounted/read-only file,
   * mirroring the real session's stdlib mount closely enough to exercise
   * `is_read_only`/`update_document`'s refusal and the TS layers built on
   * them (`ProjectSession.applyEdit`) without pre-seeding a phantom file
   * into every mock session's `list_files()`/`files` map. `update_file`
   * (unlike `update_document`) still un-marks `path` on write, mirroring
   * the real shadowing contract (`EditorSession::new`'s doc,
   * `crates/brink-web/src/editor/mod.rs`).
   */
  __mockMarkReadOnlyForTest(path: string, source: string): void {
    this.files.set(path, source);
    this.readOnlyPaths.add(path);
  }

  /**
   * Mock of the real `EditorSession::compile_project`
   * (`crates/brink-web/src/editor/mod.rs`), the studio's actual compile
   * channel — `IdeSession::compile` -> `CompileEntryError::EntryNotFound`
   * when `entry` doesn't resolve to a loaded file. The mock has no
   * compiler, so it cannot reproduce a diagnostics failure — but it CAN
   * reproduce that entry-not-found failure mode by checking `entry` against
   * the same {@link files} map every other op here reads. Reachable via a
   * constructor `entryFile` naming a path the provider never served, or an
   * entry file deleted after config resolution (NOT a misconfigured
   * `brink.toml` `[project] entry` — `ProjectSession.applyProjectConfig`
   * falls back to `hostEntryFile` instead of adopting an entry that doesn't
   * resolve, so that route never reaches here).
   * Routed through {@link compileRefusal} so #2568's guard covers this site
   * like every other refusal (#2589 — `CompileResult` was pinned into
   * `refusal-shapes.json` by #2577 with no mock call site to check it
   * against until now).
   */
  compile_project(entry: string): string {
    if (!this.files.has(entry)) {
      return EditorSession.compileRefusal(`entry file not found in session: ${entry}`);
    }
    return JSON.stringify({ ok: true, warnings: [] });
  }

  /**
   * Mock of the real `rename_file` op (pure — computes edits, does not mutate
   * the session). Returns a `MoveResult`: `new_source` is the moved file's
   * content (outbound include rewriting is left to the real Rust op; the mock
   * keeps it verbatim), and `cross_file_edits` rewrite any other file whose
   * `INCLUDE` names the old basename to the new one — enough to exercise the
   * studio's apply/egress plumbing. The real inbound/outbound math is covered
   * by Rust unit tests in brink-ide.
   */
  rename_file(oldPath: string, newPath: string): string {
    // Session-level read-only fence (issue #2306/#2343): mirrors the real
    // `rename_file`'s refusal for a mounted source path.
    if (this.readOnlyPaths.has(oldPath)) {
      return EditorSession.structuralRefusal("cannot rename: file is part of the read-only library");
    }
    const source = this.files.get(oldPath);
    if (source === undefined) {
      // #2620: this said `file not loaded` for two waves. Production's
      // `rename_file` has NO such guard — it delegates straight to
      // `brink_ide::file_rename::rename_file`, whose `RenameFileError::NotFound`
      // is `file '{0}' not found`. Driven now, not read.
      return EditorSession.structuralRefusal(`file '${oldPath}' not found`);
    }
    if (oldPath !== newPath && this.files.has(newPath)) {
      return EditorSession.structuralRefusal(`a file already exists at '${newPath}'`);
    }
    const oldBase = oldPath.split("/").pop()!;
    const newBase = newPath.split("/").pop()!;
    const escaped = oldBase.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const includeRe = new RegExp(`(INCLUDE\\s+\\S*?)${escaped}\\b`, "g");
    const crossFileEdits: { path: string; new_source: string }[] = [];
    if (oldPath !== newPath) {
      for (const [p, src] of this.files) {
        if (p === oldPath) continue;
        const rewritten = src.replace(includeRe, `$1${newBase}`);
        if (rewritten !== src) {
          crossFileEdits.push({ path: p, new_source: rewritten });
        }
      }
    }
    return JSON.stringify({
      ok: true,
      path: oldPath,
      new_source: source,
      cross_file_edits: crossFileEdits,
      // Unified StructuralResult gate (#316). The mock does not model the
      // INCLUDE-graph breakage, so a rename is reported safe.
      introduced_diagnostics: [],
      safe: true,
    });
  }

  /**
   * Mock of the real `delete_symbol` op (#316). Removes the named knot's whole
   * region (header + body + nested stitches) or a stitch's region, and reports
   * `E020`-style breakage when any other line still diverts/threads to the
   * removed symbol — enough to drive the studio's safe-by-default report. The
   * precise dangling-reference math is covered by Rust tests.
   *
   * ## The knot is resolved FIRST, and the stitch only inside it (#2641)
   *
   * This used to locate the target with a single `lines.findIndex` over the
   * WHOLE file, which made the mock succeed in two cases production refuses:
   *
   * | call on `TWO_KNOTS`                | mock before        | production        |
   * | ---------------------------------- | ------------------ | ----------------- |
   * | `delete_symbol(p, "two", "b")`     | deleted `b` (under `one`) | `stitch 'b' not found in knot` |
   * | `delete_symbol(p, "ghost", "a")`   | deleted `a` (under `one`) | `source knot not found` |
   *
   * `brink_ide::structural_delete::delete_symbol` resolves the knot first
   * (`MoveError::SourceNotFound` when it is missing) and only then looks the
   * stitch up **inside that knot's body** (`MoveError::StitchNotFound`), so a
   * whole-file scan answers a different question. The `knotRe` guard #2627
   * added only ran on the not-found branch, so once the stitch was found
   * anywhere the knot was never checked at all.
   *
   * ⚠ This is the one divergence class `structural-refusal-shape.test.ts`
   * structurally cannot catch: every mechanism built across
   * #2568/#2577/#2610/#2627 compares refusal WORDING, and here the mock did
   * not refuse — it returned `ok: true`. Hence the two driven sub-cases
   * (`delete_symbol:stitch-under-wrong-knot`,
   * `delete_symbol:stitch-under-missing-knot`) rather than an argument.
   */
  delete_symbol(path: string, knot: string, stitch: string): string {
    const source = this.files.get(path);
    if (source === undefined) {
      // #2620: this said `file not loaded`. Production's `delete_symbol` has no
      // wasm-level guard at all — `brink_ide::structural_delete::delete_symbol`
      // maps BOTH an unloaded path and a missing knot onto the same
      // `MoveError::SourceNotFound` (`source knot not found`), so the mock
      // cannot distinguish them either. Driven now, not read.
      return EditorSession.structuralRefusal("source knot not found");
    }
    const name = stitch || knot;
    const lines = source.split("\n");
    // #2641: interpolated unescaped for three waves, unlike the sibling
    // `rename_symbol`, which has always escaped. See `escapeForRegex`'s doc
    // for why no real symbol name reaches it today and it is fixed anyway.
    const knotRe = new RegExp(`^\\s*={2,3}\\s*${escapeForRegex(knot)}\\b`);
    const knotStart = lines.findIndex((l) => knotRe.test(l));
    if (knotStart < 0) {
      // The knot itself is missing — `MoveError::SourceNotFound`, the same
      // variant an unloaded path folds onto above. Production reaches this
      // BEFORE it considers the stitch at all, so a named stitch that exists
      // under some *other* knot does not rescue the call (#2641 case 2).
      return EditorSession.structuralRefusal("source knot not found");
    }
    // The knot's own region: header through the line before the next
    // top-level header. Every stitch lookup below is bounded by it.
    let knotEnd = knotStart + 1;
    while (knotEnd < lines.length && !/^\s*={2,3}/.test(lines[knotEnd]!)) knotEnd++;

    let start: number;
    let end: number;
    if (stitch) {
      const stitchRe = new RegExp(`^\\s*=\\s+${escapeForRegex(stitch)}\\b`);
      const relative = lines.slice(knotStart + 1, knotEnd).findIndex((l) => stitchRe.test(l));
      if (relative < 0) {
        // #2620 review / #2627: a missing STITCH inside a knot that DOES
        // exist is `MoveError::StitchNotFound` ("stitch '<name>' not found in
        // knot", structural_move.rs:23), not the knot wording. #2641: it is
        // also what production answers when the stitch exists but lives under
        // a DIFFERENT knot, which is why the search is bounded here rather
        // than re-checked after a whole-file hit.
        return EditorSession.structuralRefusal(`stitch '${stitch}' not found in knot`);
      }
      start = knotStart + 1 + relative;
      // A stitch region ends at the next header of any level, and never
      // escapes its own knot.
      end = start + 1;
      while (end < knotEnd && !/^\s*={1,3}/.test(lines[end]!)) end++;
    } else {
      start = knotStart;
      end = knotEnd;
    }
    const kept = [...lines.slice(0, start), ...lines.slice(end)];
    const newSource = kept.join("\n");

    // Breakage: any remaining `-> name` / `<- name` (here or in another file).
    const refRe = new RegExp(`(?:->|<-)\\s*${escapeForRegex(name)}\\b`);
    const introduced: {
      severity: string;
      code: string;
      message: string;
      path: string;
      line: number;
      col: number;
    }[] = [];
    const scan = (p: string, src: string) => {
      src.split("\n").forEach((l, i) => {
        if (refRe.test(l)) {
          introduced.push({
            severity: "error",
            code: "E020",
            message: `unresolved divert to '${name}'`,
            path: p,
            line: i + 1,
            col: 1,
          });
        }
      });
    };
    scan(path, newSource);
    for (const [p, src] of this.files) {
      if (p !== path) scan(p, src);
    }

    return JSON.stringify({
      ok: true,
      path,
      new_source: newSource,
      cross_file_edits: [],
      introduced_diagnostics: introduced,
      safe: introduced.length === 0,
    });
  }

  /**
   * Mock of the real `extract_to_knot` op (#315 H): lift the selected lines
   * (snapped to whole lines) into a new top-level `=== name ===` knot ending
   * with a `->->` tunnel return, and replace them with `-> name ->`. Offsets are
   * whole-file UTF-16 (== byte offsets for the ASCII fixtures the studio tests
   * use). The precise scope-breakage gate is covered by Rust tests; the mock
   * always reports `safe: true`.
   */
  extract_to_knot(path: string, startOffset: number, endOffset: number, name: string): string {
    return this.extractImpl(path, startOffset, endOffset, name, "knot");
  }

  /**
   * Mock of the real `extract_to_function` op (#315 H): as {@link extract_to_knot}
   * but into a `=== function name() ===` decl, replacing the selection with
   * `~ name()`.
   */
  extract_to_function(path: string, startOffset: number, endOffset: number, name: string): string {
    return this.extractImpl(path, startOffset, endOffset, name, "function");
  }

  private extractImpl(
    path: string,
    startOffset: number,
    endOffset: number,
    name: string,
    kind: "knot" | "function",
  ): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    const lo = Math.min(startOffset, endOffset);
    const hi = Math.max(startOffset, endOffset);
    if (lo === hi) {
      return EditorSession.structuralRefusal("empty selection: nothing to extract");
    }
    // Name collision (#2578): mirrors the real op's "name collision" refusal
    // class named in `document-sessions.ts`'s doc-comment ("name collision,
    // header crossing, illegal function body") — enough to drive a genuine
    // `ok: false` extract result in tests without reimplementing the Rust
    // scope-analysis. The precise collision math is covered by Rust tests.
    const esc = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    if (new RegExp(`^\\s*={2,3}\\s*(function\\s+)?${esc}\\b`, "m").test(source)) {
      return EditorSession.structuralRefusal(`a knot or function named '${name}' already exists`);
    }
    // Snap to whole lines.
    const selStart = source.lastIndexOf("\n", lo - 1) + 1;
    const nextNl = source.indexOf("\n", hi);
    const selEnd = nextNl < 0 ? source.length : nextNl + 1;
    const selected = source.slice(selStart, selEnd);

    const call = kind === "knot" ? `-> ${name} ->\n` : `~ ${name}()\n`;
    const header = kind === "knot" ? `=== ${name} ===\n` : `=== function ${name}() ===\n`;
    let body = selected.endsWith("\n") ? selected : `${selected}\n`;
    if (kind === "knot") body += "->->\n";

    let out = source.slice(0, selStart) + call + source.slice(selEnd);
    if (!out.endsWith("\n")) out += "\n";
    if (!out.endsWith("\n\n")) out += "\n";
    out += header + body;

    return JSON.stringify({
      ok: true,
      path,
      new_source: out,
      cross_file_edits: [],
      introduced_diagnostics: [],
      safe: true,
    });
  }

  /**
   * A refused structural op, in the exact shape the real wasm emits (#2543).
   *
   * Rust's `error_json` (`crates/brink-web/src/editor_refactor.rs`) serializes
   * the whole `StructuralResultJs`, and only `path`/`new_source`/`error` carry
   * `skip_serializing_if` — so a REFUSAL still ships `safe: true` with empty
   * `cross_file_edits`/`introduced_diagnostics` beside its `ok: false`.
   *
   * The rename mocks used to answer `{ ok: false, error }` alone, and that
   * omission is why #2543 survived the studio suite: `isSafeRename` reads
   * `result.safe`, an absent `safe` is falsy, so under the mock a refused
   * rename looked UNSAFE (report shown, nothing committed) while production
   * called it SAFE and committed it. Keep this payload faithful — a mock that
   * understates the contract cannot see a bug that lives in the contract.
   *
   * ⚠ EVERY structural refusal in this file must route through here (#2568) —
   * `rename_file`, `delete_symbol`, `extract_to_knot`/`extract_to_function`,
   * `rename_symbol`, `rename_symbol_at`. Each site that answers its own object
   * literal is another latent invisible instance of the #2543 class. Enforced
   * by `src/__tests__/structural-refusal-shape.test.ts`, which compares every
   * site against `crates/brink-web/fixtures/refusal-shapes.json` — a fixture
   * GENERATED from the Rust structs, not hand-copied from them.
   */
  private static structuralRefusal(error: string): string {
    return JSON.stringify({
      ok: false,
      cross_file_edits: [],
      introduced_diagnostics: [],
      safe: true,
      error,
    });
  }

  /**
   * A refused directory move (`DirMoveResultJs`, the third Rust refusal struct
   * — `crates/brink-web/src/editor_refactor.rs::dir_error_json`). Multi-file,
   * so it carries `moved_files` instead of `path`/`new_source`, and NOTHING is
   * `skip_serializing_if`: a refusal ships the full `moved_files` /
   * `cross_file_edits` / `introduced_diagnostics` / `safe` set beside its
   * `ok: false`, exactly like {@link structuralRefusal}'s (#2577).
   */
  private static dirMoveRefusal(error: string): string {
    return JSON.stringify({
      ok: false,
      moved_files: [],
      cross_file_edits: [],
      introduced_diagnostics: [],
      safe: true,
      error,
    });
  }

  /**
   * A successful structural op, in the shape Rust's `structural_result_json` /
   * `move_result_json_simple` emit. `introduced_diagnostics`/`safe` are the
   * breakage gate: the mock has no analyzer, so — like every other structural
   * mock in this file — it reports a computed op as safe with no introduced
   * diagnostics, and the real gate math stays covered by the Rust tests in
   * brink-ide. Refusals go through {@link structuralRefusal}, never here.
   */
  private static structuralOk(
    path: string,
    newSource: string,
    crossFileEdits: { path: string; new_source: string }[] = [],
  ): string {
    return JSON.stringify({
      ok: true,
      path,
      new_source: newSource,
      cross_file_edits: crossFileEdits,
      introduced_diagnostics: [],
      safe: true,
    });
  }

  /**
   * A refused auto-import (`AutoImportJs`, a *different* Rust struct from
   * {@link structuralRefusal}'s — no `safe`/`cross_file_edits` gate, and
   * `edit` is the only skipped field).
   *
   * These two doc-handle sites already emitted the faithful shape before
   * #2568; the helper exists so they cannot drift away from it, and so the
   * shape-parity test has one named seam per Rust struct rather than a set of
   * ad-hoc literals.
   */
  private static autoImportRefusal(error: string): string {
    return JSON.stringify({ ok: false, already_reachable: false, error });
  }

  /**
   * A refused compile (`CompileResult`, the compile channel's own refusal
   * struct — `crates/brink-web/src/compile.rs`, distinct from every other
   * shape here: no `safe`/`cross_file_edits` gate, no `path`. `warnings` has
   * no `skip_serializing_if` on the real struct, so it always ships — even
   * empty, unlike `story_bytes`, which is omitted entirely on refusal (#2589).
   */
  private static compileRefusal(error: string): string {
    return JSON.stringify({ ok: false, warnings: [], error });
  }

  /**
   * Mock of the real `rename_symbol` op (pure — computes edits, does not
   * mutate the session). Rewrites the symbol's header plus `->`/`<-` diverts
   * to it across every file, and flags an `E022` breakage when renaming a knot
   * onto an existing top-level knot name (the safe-by-default gate, #305). The
   * precise rename + diagnostic-diff math is covered by Rust tests; the mock is
   * enough to drive the studio prompt/report plumbing.
   *
   * ## Two guards, in production's order (#2634)
   *
   * ⚠ The `file not loaded` guard here is FAITHFUL and must stay. Unlike
   * `rename_file` and `delete_symbol` — which delegate straight to `brink-ide`
   * with no wasm-level file guard, which is why #2620 found their mock wording
   * lying — `rename_symbol` really does emit `error_json("file not loaded")` at
   * the wasm level (`crates/brink-web/src/editor/refactor.rs:478`).
   *
   * The `symbol not found` guard below is the one that was MISSING: production
   * resolves `brink_ide::rename::declaration_offset(hir, knot, stitch)` and
   * refuses when it finds no declaration, while the mock proceeded — so a
   * rename of a knot that had been edited away *succeeded* under the mock,
   * rewriting nothing and reporting `ok: true`. `performSymbolRename`
   * (`packages/studio-ui/src/symbolMenuActions.ts`) names exactly this case in
   * its own comment as one it notifies the author about, and that branch was
   * unreachable by the studio suite.
   *
   * `declaration_offset` looks the stitch up **inside the named knot only**, so
   * this does too — a stitch that exists under a different knot is not found.
   */
  rename_symbol(path: string, knot: string, stitch: string, newName: string): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    const oldName = stitch || knot;
    const esc = escapeForRegex;

    // #2634: mirrors `declaration_offset` — resolve the knot, then the stitch
    // within that knot's region. Same `={2,3}` / `=\s+` header vocabulary the
    // rewrite below uses, so a header this function can rename is one this
    // guard can find.
    {
      const lines = source.split("\n");
      const knotLine = lines.findIndex((l) =>
        new RegExp(`^\\s*={2,3}\\s*${esc(knot)}\\b`).test(l),
      );
      if (knotLine < 0) {
        return EditorSession.structuralRefusal("symbol not found");
      }
      if (stitch) {
        let knotEnd = knotLine + 1;
        while (knotEnd < lines.length && !/^\s*={2,3}/.test(lines[knotEnd]!)) knotEnd++;
        const found = lines
          .slice(knotLine + 1, knotEnd)
          .some((l) => new RegExp(`^\\s*=\\s+${esc(stitch)}\\b`).test(l));
        if (!found) {
          return EditorSession.structuralRefusal("symbol not found");
        }
      }
    }

    // Breakage: renaming a knot onto an existing top-level knot collides.
    const introduced: {
      severity: string;
      code: string;
      message: string;
      path: string;
      line: number;
      col: number;
    }[] = [];
    if (!stitch && newName !== knot) {
      const lines = source.split("\n");
      const collisionLine = lines.findIndex(
        (l) => new RegExp(`^\\s*={2,3}\\s*${esc(newName)}\\b`).test(l),
      );
      if (collisionLine >= 0) {
        introduced.push({
          severity: "error",
          code: "E022",
          message: "duplicate knot definition",
          path,
          line: collisionLine + 1,
          col: 1,
        });
      }
    }

    // Rewrite a file's references to `oldName` → `newName`: the header (knot
    // `=== name ===` or stitch `= name`) plus diverts/threads (`-> name`,
    // `<- name`, qualified `knot.name`).
    const rewrite = (src: string): string => {
      let out = src;
      if (stitch) {
        out = out.replace(new RegExp(`(^|\\n)(\\s*=\\s*)${esc(oldName)}\\b`, "g"), `$1$2${newName}`);
      } else {
        out = out.replace(
          new RegExp(`(={2,3}\\s*)${esc(oldName)}(\\s*={0,3})`, "g"),
          `$1${newName}$2`,
        );
      }
      out = out.replace(new RegExp(`((?:->|<-)\\s*)${esc(oldName)}\\b`, "g"), `$1${newName}`);
      return out;
    };

    const newSource = rewrite(source);
    const crossFileEdits: { path: string; new_source: string }[] = [];
    for (const [p, src] of this.files) {
      if (p === path) continue;
      const rewritten = rewrite(src);
      if (rewritten !== src) crossFileEdits.push({ path: p, new_source: rewritten });
    }

    return JSON.stringify({
      ok: true,
      path,
      new_source: newSource,
      cross_file_edits: crossFileEdits,
      introduced_diagnostics: introduced,
      safe: introduced.length === 0,
    });
  }

  /**
   * Offset-based rename (F2). Resolves the knot/stitch whose *declaration name*
   * the UTF-16 file `offset` lands in, then delegates to `rename_symbol`. The
   * mock only resolves declaration sites (enough for the plumbing); the real
   * wasm also resolves references and non-container symbols.
   */
  rename_symbol_at(path: string, offset: number, newName: string): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    for (const knot of parseOutline(source)) {
      if (offset >= knot.start && offset <= knot.end) {
        return this.rename_symbol(path, knot.name, "", newName);
      }
      for (const st of knot.children) {
        if (offset >= st.start && offset <= st.end) {
          return this.rename_symbol(path, knot.name, st.name, newName);
        }
      }
    }
    return EditorSession.structuralRefusal("cannot rename this symbol");
  }

  // ── Structural symbol ops (#2577) ────────────────────────────────
  //
  // The seven ops `dispatchSymbolAction` (packages/studio-ui) calls on a real
  // session. Before #2577 the mock had NO method for any of them, so the
  // studio suite could not exercise the Binder's reorder / move / promote /
  // demote menu at all — not badly, at all: the call threw
  // `session.reorderStitch is not a function`.
  //
  // Each op below mirrors its Rust counterpart in
  // `crates/brink-web/src/editor/refactor.rs` → `brink_ide::structural_move`,
  // read off that source rather than off what a test wanted:
  //
  //   - the SAME refusal vocabulary (`MoveError`'s `Display` strings) in the
  //     SAME order the real op checks them — a mock that refuses for a
  //     different reason than production is a new lie, not a smaller one;
  //   - a boundary reorder (first stitch "up") is NOT a refusal: the real op
  //     returns `Ok(source)` unchanged, so the mock answers a successful,
  //     unchanged `StructuralResult` too;
  //   - refusals go through {@link structuralRefusal} so they carry the full
  //     `safe`/`cross_file_edits`/`introduced_diagnostics` payload production
  //     ships (#2543/#2568).
  //
  // What the mock does NOT model, and must not be read as: the breakage gate.
  // The real move / promote / demote run `gated_move_json`, which re-analyzes
  // and can come back `safe: false`. The mock has no analyzer, so a computed
  // op is always `safe: true` — same simplification `rename_file` and
  // `extract_to_knot` already document above.

  /** Reorder a stitch within its knot. `direction`: >= 0 = down, < 0 = up. */
  reorder_stitch(path: string, knot: string, stitch: string, direction: number): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    const knots = parseOutline(source);
    const k = knots.find((s) => s.name === knot);
    if (!k) {
      return EditorSession.structuralRefusal("source knot not found");
    }
    const si = k.children.findIndex((s) => s.name === stitch);
    if (si < 0) {
      return EditorSession.structuralRefusal(`stitch '${stitch}' not found in knot`);
    }
    const target = direction >= 0 ? si + 1 : si - 1;
    // At the boundary the real op returns the source unchanged, not an error.
    if (target < 0 || target >= k.children.length) {
      return EditorSession.structuralOk(path, source);
    }
    const plan = planKnots(source, knots);
    const stitches = plan.find((p) => p.name === knot)!.stitches;
    [stitches[si], stitches[target]] = [stitches[target]!, stitches[si]!];
    return EditorSession.structuralOk(path, renderKnots(source, knots, plan));
  }

  /** Reorder a knot within the top-level knot list. `direction`: >= 0 = down. */
  reorder_knot(path: string, knot: string, direction: number): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    const knots = parseOutline(source);
    const ki = knots.findIndex((s) => s.name === knot);
    if (ki < 0) {
      return EditorSession.structuralRefusal("source knot not found");
    }
    const target = direction >= 0 ? ki + 1 : ki - 1;
    if (target < 0 || target >= knots.length) {
      return EditorSession.structuralOk(path, source);
    }
    const plan = planKnots(source, knots);
    [plan[ki], plan[target]] = [plan[target]!, plan[ki]!];
    return EditorSession.structuralOk(path, renderKnots(source, knots, plan));
  }

  /** Reorder every stitch in `knot` to match `order` (a permutation of its
   *  stitch names) — the drag-and-drop / multi-select entry point. */
  reorder_stitches(path: string, knot: string, order: string[]): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    const knots = parseOutline(source);
    const k = knots.find((s) => s.name === knot);
    if (!k) {
      return EditorSession.structuralRefusal("source knot not found");
    }
    const indices = resolvePermutation(
      k.children.map((s) => s.name),
      order,
    );
    if (!indices) {
      return EditorSession.structuralRefusal(
        "invalid reorder: list is not a permutation of the existing names",
      );
    }
    const plan = planKnots(source, knots);
    const entry = plan.find((p) => p.name === knot)!;
    entry.stitches = indices.map((i) => entry.stitches[i]!);
    return EditorSession.structuralOk(path, renderKnots(source, knots, plan));
  }

  /** Reorder every top-level knot to match `order` (a permutation of the knot
   *  names). */
  reorder_knots(path: string, order: string[]): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    const knots = parseOutline(source);
    if (knots.length === 0) {
      return EditorSession.structuralRefusal("source knot not found");
    }
    const indices = resolvePermutation(
      knots.map((s) => s.name),
      order,
    );
    if (!indices) {
      return EditorSession.structuralRefusal(
        "invalid reorder: list is not a permutation of the existing names",
      );
    }
    const plan = planKnots(source, knots);
    return EditorSession.structuralOk(
      path,
      renderKnots(
        source,
        knots,
        indices.map((i) => plan[i]!),
      ),
    );
  }

  /** Move a stitch from one knot to another, requalifying `src.stitch` →
   *  `dest.stitch` here and in every other file. */
  move_stitch(path: string, srcKnot: string, stitch: string, destKnot: string): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    const knots = parseOutline(source);
    const src = knots.find((s) => s.name === srcKnot);
    if (!src) {
      return EditorSession.structuralRefusal("source knot not found");
    }
    const dest = knots.find((s) => s.name === destKnot);
    if (!dest) {
      return EditorSession.structuralRefusal("destination knot not found");
    }
    // The real op checks the destination collision BEFORE resolving the source
    // stitch, so a move onto an occupied name reports the collision even when
    // the source stitch is also missing. Keep the order.
    if (dest.children.some((s) => s.name === stitch)) {
      return EditorSession.structuralRefusal(
        `name collision: '${stitch}' already exists in ${destKnot}`,
      );
    }
    const si = src.children.findIndex((s) => s.name === stitch);
    if (si < 0) {
      return EditorSession.structuralRefusal(`stitch '${stitch}' not found in knot`);
    }
    const plan = planKnots(source, knots);
    const [moved] = plan.find((p) => p.name === srcKnot)!.stitches.splice(si, 1);
    plan.find((p) => p.name === destKnot)!.stitches.push(moved!);
    const rendered = renderKnots(source, knots, plan);
    const oldQual = `${srcKnot}.${stitch}`;
    const newQual = `${destKnot}.${stitch}`;
    return EditorSession.structuralOk(
      path,
      requalifyReferences(rendered, oldQual, newQual),
      this.requalifyOtherFiles(path, oldQual, newQual),
    );
  }

  /** Promote a stitch to a top-level knot, inserted immediately after its
   *  former parent (the position the real op uses) and requalifying
   *  `knot.stitch` → `stitch`. */
  promote_stitch(path: string, knot: string, stitch: string): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    const knots = parseOutline(source);
    // Checked first by the real op, before the parent knot is resolved.
    if (knots.some((s) => s.name === stitch)) {
      return EditorSession.structuralRefusal(
        `name collision: '${stitch}' already exists in top-level knots`,
      );
    }
    const ki = knots.findIndex((s) => s.name === knot);
    if (ki < 0) {
      return EditorSession.structuralRefusal("source knot not found");
    }
    const si = knots[ki]!.children.findIndex((s) => s.name === stitch);
    if (si < 0) {
      return EditorSession.structuralRefusal(`stitch '${stitch}' not found in knot`);
    }
    const plan = planKnots(source, knots);
    const [moved] = plan[ki]!.stitches.splice(si, 1);
    let promoted = stitchHeaderToKnot(moved!.text, stitch);
    if (!promoted.endsWith("\n")) promoted += "\n";
    plan.splice(ki + 1, 0, { name: stitch, head: promoted, stitches: [] });
    const rendered = renderKnots(source, knots, plan);
    const oldQual = `${knot}.${stitch}`;
    return EditorSession.structuralOk(
      path,
      requalifyReferences(rendered, oldQual, stitch),
      this.requalifyOtherFiles(path, oldQual, stitch),
    );
  }

  /** Demote a top-level knot into another knot as its last stitch,
   *  requalifying `knot` → `dest.knot`. */
  demote_knot(path: string, knot: string, destKnot: string): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    const knots = parseOutline(source);
    const ki = knots.findIndex((s) => s.name === knot);
    if (ki < 0) {
      return EditorSession.structuralRefusal("source knot not found");
    }
    const dest = knots.find((s) => s.name === destKnot);
    if (!dest) {
      return EditorSession.structuralRefusal("destination knot not found");
    }
    if (knots[ki]!.children.length > 0) {
      return EditorSession.structuralRefusal(
        "illegal nesting: knot has sub-stitches and cannot be demoted",
      );
    }
    if (dest.children.some((s) => s.name === knot)) {
      return EditorSession.structuralRefusal(
        `name collision: '${knot}' already exists in ${destKnot}`,
      );
    }
    const plan = planKnots(source, knots);
    const [removed] = plan.splice(ki, 1);
    let demoted = knotHeaderToStitch(removed!.head, knot);
    if (!demoted.endsWith("\n")) demoted += "\n";
    plan.find((p) => p.name === destKnot)!.stitches.push({ name: knot, text: demoted });
    const rendered = renderKnots(source, knots, plan);
    const newQual = `${destKnot}.${knot}`;
    return EditorSession.structuralOk(
      path,
      requalifyReferences(rendered, knot, newQual),
      this.requalifyOtherFiles(path, knot, newQual),
    );
  }

  /** Every OTHER file's requalification, in `cross_file_edits` shape. */
  private requalifyOtherFiles(
    path: string,
    old: string,
    next: string,
  ): { path: string; new_source: string }[] {
    const edits: { path: string; new_source: string }[] = [];
    for (const [p, src] of this.files) {
      if (p === path) continue;
      const rewritten = requalifyReferences(src, old, next);
      if (rewritten !== src) edits.push({ path: p, new_source: rewritten });
    }
    return edits;
  }

  /**
   * Mock of the real `rename_dir` op (#314): relocate every file under
   * `oldPrefix` to `newPrefix` and re-point the `INCLUDE`s that named them.
   *
   * Answers `DirMoveResultJs` — the multi-file payload, NOT `StructuralResult`:
   * `moved_files` (each `old_path`/`new_path`/`new_source`) instead of a single
   * `path`/`new_source`, plus the shared `cross_file_edits`/`safe`/
   * `introduced_diagnostics` gate. Both refusals use the real op's own wording
   * (`DirRenameError`).
   *
   * Simplifications, all deliberate: a moved file's own source travels verbatim
   * (the real op also rewrites the moved file's outbound relative includes —
   * same simplification `rename_file` above already makes), and inbound rewrites
   * are a `oldPrefix/` → `newPrefix/` substitution on `INCLUDE` lines in files
   * outside the folder. Note the real op has NO read-only fence here (unlike
   * `rename_file`), so the mock has none either.
   */
  rename_dir(oldPrefixRaw: string, newPrefixRaw: string): string {
    // The real op trims trailing slashes off both prefixes before doing
    // anything else (`brink_ide::dir_rename::rename_dir`,
    // crates/internal/brink-ide/src/dir_rename.rs:123-124) — without this,
    // `renameDir("chapters/", "acts/")` refuses here where production
    // succeeds, because `startsWith("chapters//")` never matches.
    const oldPrefix = oldPrefixRaw.replace(/\/+$/, "");
    const newPrefix = newPrefixRaw.replace(/\/+$/, "");
    const moved = [...this.files.keys()].filter((p) => p.startsWith(`${oldPrefix}/`)).sort();
    if (moved.length === 0) {
      return EditorSession.dirMoveRefusal(`no files found under directory '${oldPrefix}'`);
    }
    const remap = (p: string): string => {
      const rest = p.slice(oldPrefix.length + 1);
      return newPrefix === "" ? rest : `${newPrefix}/${rest}`;
    };
    const movedSet = new Set(moved);
    for (const old of moved) {
      const dest = remap(old);
      if (this.files.has(dest) && !movedSet.has(dest)) {
        return EditorSession.dirMoveRefusal(`a file already exists at '${dest}'`);
      }
    }
    const movedFiles = moved.map((old) => ({
      old_path: old,
      new_path: remap(old),
      new_source: this.files.get(old)!,
    }));
    const includeRe = new RegExp(
      `(INCLUDE\\s+\\S*?)${oldPrefix.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/`,
      "g",
    );
    const crossFileEdits: { path: string; new_source: string }[] = [];
    for (const [p, src] of this.files) {
      if (movedSet.has(p)) continue;
      const rewritten = src.replace(includeRe, `$1${newPrefix === "" ? "" : `${newPrefix}/`}`);
      if (rewritten !== src) crossFileEdits.push({ path: p, new_source: rewritten });
    }
    return JSON.stringify({
      ok: true,
      moved_files: movedFiles,
      cross_file_edits: crossFileEdits,
      introduced_diagnostics: [],
      safe: true,
    });
  }

  /**
   * Mock of the real `resolve_code_action` (`crates/brink-web/src/editor/
   * code_actions.rs`): apply the action a `CodeAction.data` payload names and
   * answer a `StructuralResult`.
   *
   * `data_json` is the internally-tagged `CodeActionData` (`action` is the
   * discriminator). The mock resolves the families it can model over
   * `parseOutline` — `SortKnots`/`SortStitches` and the four that delegate
   * straight to the structural ops above (`ReorderStitch`, `MoveStitch`,
   * `PromoteStitch`, `DemoteKnot`).
   *
   * ⚠ Every OTHER known action (`FormatKnot`, `FormatStitch`, `AddImport`, the
   * `#fn(...)` quick-fixes) is UNMODELLED and answers the real op's own
   * no-change refusal. That is deliberately the real vocabulary — the real op
   * emits exactly this string whenever its rewrite is a no-op — but do not read
   * it as production's answer for those actions.
   *
   * The refusal ORDER is production's: an unknown handle / unloaded file
   * outranks malformed data, and a structural action whose op errors falls
   * through to `code action produced no change` (the Rust path `.ok()`s the
   * `MoveError` away before the pure resolver returns `None`), NOT to the
   * op's own message.
   *
   * ⚠ The two malformed-`data_json` refusals (unknown `action` tag, missing
   * `action` field) are MOCK-ONLY ABBREVIATIONS of what serde_json actually
   * produces for `CodeActionData`'s internally-tagged enum — real serde
   * output names every known variant and a source line/column, which these
   * strings omit. Unlike `code action produced no change` above, these two
   * do NOT carry vocabulary parity with production; see the inline comments
   * at each `return` for the real wording.
   */
  resolve_code_action(dataJson: string, offset: number): string {
    return this.resolveCodeActionImpl(this.activePath, dataJson, offset);
  }

  /**
   * Document-handle variant of {@link resolve_code_action}.
   *
   * ⚠ THIS IS THE ONLY DEFINITION — keep it that way. #2585 added a second
   * `resolve_code_action_doc` down in the doc-handle block (a stub that always
   * refused). JS class semantics let the later definition win silently, so the
   * doc-handle op could no longer succeed at all under the mock, and the
   * refusal vocabulary changed out from under `structural-refusal-shape.test.ts`.
   * It reached `main` because #2583 and #2585 were each green against a `main`
   * that did not yet contain the other. `mock-single-definition.test.ts` now
   * fails on any duplicated method name in this file.
   */
  resolve_code_action_doc(doc: number, dataJson: string, offset: number): string {
    const d = this.docs.get(doc);
    if (!d) {
      return EditorSession.structuralRefusal("unknown document handle");
    }
    return this.resolveCodeActionImpl(d.path, dataJson, offset);
  }

  private resolveCodeActionImpl(path: string, dataJson: string, _offset: number): string {
    const source = this.files.get(path);
    if (source === undefined) {
      return EditorSession.structuralRefusal("file not loaded");
    }
    let data: { action?: unknown; [key: string]: unknown };
    try {
      data = JSON.parse(dataJson) as { action?: unknown };
    } catch (e) {
      return EditorSession.structuralRefusal(
        `invalid code-action data: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
    const action = typeof data.action === "string" ? data.action : null;
    if (action === null) {
      // MOCK-ONLY ABBREVIATION, not serde's wording: production's serde_json
      // error for a missing internally-tagged discriminator is `missing field
      // \`action\` at line L column C`. This string is not that string.
      return EditorSession.structuralRefusal(
        "invalid code-action data: missing `action` discriminator",
      );
    }
    const noChange = EditorSession.structuralRefusal("code action produced no change");
    // ⚠ Same class as the two comments above: `asString` silently substitutes
    // `""` for a missing field instead of refusing. Production's serde deserializes
    // `CodeActionData` as a whole, so e.g. `{ action: "SortStitches" }` with no
    // `knot` is a hard `missing field \`knot\`` refusal there; here it falls
    // through to `code action produced no change` instead. Not modelled, same
    // as the unmodelled action families above.
    const asString = (key: string): string => (typeof data[key] === "string" ? data[key] : "");
    switch (action) {
      case "SortKnots": {
        const knots = parseOutline(source);
        const plan = planKnots(source, knots);
        const sorted = [...plan].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
        const next = renderKnots(source, knots, sorted);
        return next === source ? noChange : EditorSession.structuralOk(path, next);
      }
      case "SortStitches": {
        const knots = parseOutline(source);
        const knot = asString("knot");
        const entry = planKnots(source, knots).map((p) =>
          p.name === knot
            ? {
                ...p,
                stitches: [...p.stitches].sort((a, b) =>
                  a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
                ),
              }
            : p,
        );
        const next = renderKnots(source, knots, entry);
        return next === source ? noChange : EditorSession.structuralOk(path, next);
      }
      case "ReorderStitch":
        return this.codeActionOrNoChange(
          this.reorder_stitch(
            path,
            asString("knot"),
            asString("stitch"),
            data.direction === "Up" ? -1 : 1,
          ),
          source,
        );
      case "MoveStitch":
        return this.codeActionOrNoChange(
          this.move_stitch(path, asString("src_knot"), asString("stitch"), asString("dest_knot")),
          source,
        );
      case "PromoteStitch":
        return this.codeActionOrNoChange(
          this.promote_stitch(path, asString("knot"), asString("stitch")),
          source,
        );
      case "DemoteKnot":
        return this.codeActionOrNoChange(
          this.demote_knot(path, asString("knot"), asString("dest_knot")),
          source,
        );
      case "FormatKnot":
      case "FormatStitch":
      case "AddImport":
      case "TrimFnLiteralArgs":
      case "BindFnLiteralRefArgs":
      case "TrimValueCallArgs":
        // Known to the real op, unmodelled here — see this method's doc.
        return noChange;
      default:
        // MOCK-ONLY ABBREVIATION, not serde's wording: production's serde_json
        // error for an internally-tagged enum's unknown variant is `unknown
        // variant \`${action}\`, expected one of \`SortKnots\`, \`SortStitches\`,
        // ...\` at line L column C` (verified against serde/serde_json directly,
        // not read off this switch's tag list). This string omits the
        // `expected one of` clause and the line/column suffix.
        return EditorSession.structuralRefusal(
          `invalid code-action data: unknown variant \`${action}\``,
        );
    }
  }

  /** Fold a delegated structural op's answer into the code-action contract: a
   *  refusal or an unchanged rewrite both surface as `code action produced no
   *  change`, because the Rust resolver discards the `MoveError` and then
   *  reports a no-op rewrite as `None`. */
  private codeActionOrNoChange(delegated: string, source: string): string {
    const parsed = JSON.parse(delegated) as { ok: boolean; new_source?: string };
    if (!parsed.ok || parsed.new_source === source) {
      return EditorSession.structuralRefusal("code action produced no change");
    }
    return delegated;
  }

  /**
   * `[project] entry` from the most recently parsed `brink.toml` (issue
   * #2331) — set wholesale by {@link readProjectConfigWarnings} on every
   * `apply_project_config`/`discover_project_config` call, mirroring the
   * real `EditorSession`'s `configured_entry` field: `undefined` when the
   * parsed file didn't set `entry` (or no file was parsed yet), never
   * "sticky" across a call whose file removed the key.
   */
  private configuredEntry: string | undefined;

  /**
   * Mock of `apply_project_config` (#1005) — applies TOML text handed to it
   * directly, without any discovery.
   */
  apply_project_config(toml: string): string {
    return JSON.stringify(this.readProjectConfigWarnings(toml));
  }

  /**
   * Mock of `discover_project_config` (#1414, issue #2324's wiring target):
   * walks up from `entry`'s directory over `this.files` (this session's own
   * in-memory documents — the mock's stand-in for the real
   * `brink_source_tree::SourceTree` walk) looking for a `brink.toml` at each
   * ancestor, exactly like the real op's exact-string-equality ancestor
   * search. Returns `"[]"` (never an error) when none is found.
   */
  discover_project_config(entry: string): string {
    const slash = entry.lastIndexOf("/");
    let dir = slash >= 0 ? entry.slice(0, slash) : "";
    for (;;) {
      const candidate = dir === "" ? "brink.toml" : `${dir}/brink.toml`;
      const text = this.files.get(candidate);
      if (text !== undefined) {
        return JSON.stringify(this.readProjectConfigWarnings(text));
      }
      if (dir === "") break;
      const idx = dir.lastIndexOf("/");
      dir = idx >= 0 ? dir.slice(0, idx) : "";
    }
    // No brink.toml found anywhere in the walk-up: mirrors the real
    // discovery's "missing config = unchanged defaults" contract — a
    // previously configured entry must not stick around either.
    this.configuredEntry = undefined;
    return "[]";
  }

  /**
   * Mock of `configured_entry` (issue #2331): the `[project] entry` value
   * from the most recently parsed `brink.toml`, or `undefined` if unset.
   */
  configured_entry(): string | undefined {
    return this.configuredEntry;
  }

  /**
   * Minimal `[project]`/`[lints]` reader backing both config ops above —
   * mirrors just enough of `brink_project_config::parse_str_at`'s
   * known-key set (#1005/#1397/#1417/#1880/#2331) to drive studio tests:
   * `dialect`/`types`/`conventions`/`unprune-dirs`/the deprecated
   * `elements` alias/`entry` are recognized. `entry`'s parsed value is
   * stashed into {@link configuredEntry} (read back by
   * {@link configured_entry}) — every other recognized key is accepted
   * silently, with no session-state effect to simulate. Every unrecognized
   * `[project]` key is reported as a warning, and every `[lints]` key is
   * accepted without validation (this mock has no diagnostic-code registry
   * to check against). Deliberately line-oriented, not a real TOML parser —
   * enough for the flat tables `brink.toml` actually uses in tests/fixtures.
   */
  private readProjectConfigWarnings(toml: string): string[] {
    const KNOWN_PROJECT_KEYS = new Set([
      "dialect",
      "types",
      "conventions",
      "elements",
      "unprune-dirs",
      "entry",
    ]);
    const warnings: string[] = [];
    let section: "project" | "lints" | null = null;
    // Wholesale replace (#2331, mirroring `conventions`'s own no-precedence
    // contract): reset before scanning, so a file that dropped `entry`
    // since the last call actually clears it.
    this.configuredEntry = undefined;
    for (const raw of toml.split("\n")) {
      const line = raw.trim();
      if (line === "" || line.startsWith("#")) continue;
      const sectionMatch = /^\[(.+)\]$/.exec(line);
      if (sectionMatch) {
        const name = sectionMatch[1]!.trim();
        section = name === "project" ? "project" : name === "lints" ? "lints" : null;
        continue;
      }
      const kv = /^([^=]+)=\s*(.*)$/.exec(line);
      if (!kv) continue;
      const key = kv[1]!.trim();
      if (section === "project" && !KNOWN_PROJECT_KEYS.has(key)) {
        warnings.push(`unknown key \`project.${key}\` in brink.toml (ignored)`);
      }
      if (section === "project" && key === "entry") {
        const valueMatch = /^"([^"]*)"$/.exec(kv[2]!.trim());
        if (valueMatch && valueMatch[1] !== "") this.configuredEntry = valueMatch[1];
      }
    }
    return warnings;
  }

  // Host-capability manifest + value cache (#174) — no-ops in the mock.
  set_host_manifest(_json: string): void { /* no-op */ }
  clear_host_manifest(): void { /* no-op */ }
  set_host_values(_json: string): void { /* no-op */ }
  clear_host_values(): void { /* no-op */ }

  // Dialogue dialect (#368) — no-ops in the mock; `line_contexts_doc`/
  // `line_contexts` always return "[]" here, so there is no dialect facet
  // to populate either way. Mirrors the host-manifest no-op pattern above.
  set_dialect(_json: string): void { /* no-op */ }
  clear_dialect(): void { /* no-op */ }
  set_fold_runs_enabled(_enabled: boolean): void { /* no-op */ }

  /** Lists read-only (mounted) files alongside real ones, flagged `mounted`
   *  — see {@link list_files}'s doc (issue #2306/#2343). */
  project_outline(): string {
    const outline = [];
    for (const [path, source] of this.files) {
      outline.push({ path, symbols: parseOutline(source), mounted: this.readOnlyPaths.has(path) });
    }
    return JSON.stringify(outline);
  }

  /**
   * Story graph (#96): nodes derived from the same header parse as the
   * outline (knots + stitches with parent ids), no edges. The real edge
   * extraction is covered by Rust tests in brink-ide/brink-web. Nodes carry
   * `mounted` — see {@link list_files}'s doc (issue #2306/#2343).
   */
  story_graph(): string {
    const nodes = [];
    for (const [path, source] of this.files) {
      const mounted = this.readOnlyPaths.has(path);
      for (const sym of parseOutline(source)) {
        nodes.push({
          id: sym.name,
          name: sym.name,
          kind: "knot",
          file: path,
          start: sym.start,
          end: sym.end,
          mounted,
        });
        for (const child of sym.children) {
          const id = `${sym.name}.${child.name}`;
          nodes.push({
            id,
            name: id,
            kind: "stitch",
            file: path,
            start: child.start,
            end: child.end,
            parent: sym.name,
            mounted,
          });
        }
      }
    }
    nodes.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
    return JSON.stringify({ nodes, edges: [] });
  }

  // ── Document handles (mirrors brink-web's multi-document API) ──

  open_document(path: string): number {
    if (!this.files.has(path)) return 0;
    const id = this.nextDocId++;
    this.docs.set(id, { path, viewStart: null, viewEnd: null });
    return id;
  }

  open_fragment(path: string, start: number, end: number): number {
    if (!this.files.has(path)) return 0;
    const id = this.nextDocId++;
    this.docs.set(id, { path, viewStart: start, viewEnd: end });
    return id;
  }

  close_document(doc: number): boolean {
    return this.docs.delete(doc);
  }

  update_document(doc: number, source: string): string {
    const d = this.docs.get(doc);
    if (!d) return "null";
    // Session-level read-only enforcement (issue #2306): mirrors the real
    // `update_document`'s refusal for a handle whose file is still mounted.
    if (this.readOnlyPaths.has(d.path)) return "null";
    const full = this.files.get(d.path) ?? "";
    if (d.viewStart != null && d.viewEnd != null) {
      const start = d.viewStart;
      const end = d.viewEnd;
      const before = full.slice(0, start);
      const after = full.slice(end);
      // The real splice maintains a "\n" separator after the fragment when
      // the original view boundary sat on one and the new text doesn't end
      // with it; the simple mock just splices verbatim.
      this.files.set(d.path, before + source + after);
      d.viewEnd = start + source.length;
      return JSON.stringify({ path: d.path, start, end });
    }
    const prevLength = full.length;
    this.files.set(d.path, source);
    return JSON.stringify({ path: d.path, start: 0, end: prevLength });
  }

  /**
   * Mock of `auto_import_include_doc` (#312 F): report whether `target` is
   * reachable from the file backing `doc` and, when not, the whole-file
   * INCLUDE-insertion edit. The mock's reachability is a plain substring check
   * for an `INCLUDE <target-basename>` line — enough to drive the studio
   * accept path. The edit always inserts at file top (offset 0).
   */
  auto_import_include_doc(doc: number, target: string): string {
    const d = this.docs.get(doc);
    if (!d) {
      // Production's wording verbatim (`crates/brink-web/src/editor/refactor.rs`,
      // `auto_import_include_doc`); pinned by the driven `messages` map in
      // `crates/brink-web/fixtures/refusal-shapes.json` (#2603).
      return EditorSession.autoImportRefusal("unknown document handle");
    }
    const source = this.files.get(d.path) ?? "";
    const base = target.split("/").pop()!;
    const reachable = new RegExp(`^INCLUDE\\s+\\S*${base.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "m").test(source);
    if (reachable) {
      return JSON.stringify({ ok: true, already_reachable: true });
    }
    return JSON.stringify({
      ok: true,
      already_reachable: false,
      edit: { from: 0, to: 0, insert: `INCLUDE ${base}\n` },
    });
  }

  /**
   * Mock of `auto_import_apply_include_doc` (#312 F, fragment-view path): apply
   * the INCLUDE to the whole file AND rebase every open fragment view on that
   * file that begins at/after the insertion point. This mirrors the real op —
   * without the rebase, the next `update_document` fragment splice would clobber
   * the INCLUDE line (the very bug under test). Returns the applied edit (as a
   * shift descriptor) with no expectation the caller re-applies it.
   *
   * The read-only-mount fence below is #2621: production refuses a handle on a
   * mounted stdlib file BEFORE attempting the include, and until now the mock
   * modelled no fence at all, so the studio suite could not reach that branch.
   * Only `auto_import_apply_include_doc` gets it — `auto_import_include_doc` is
   * a pure query that computes an edit without writing, and production applies
   * no such fence to it (`crates/brink-web/src/editor/refactor.rs`); adding one
   * here would be a divergence, not fidelity.
   */
  auto_import_apply_include_doc(doc: number, target: string): string {
    const d = this.docs.get(doc);
    if (!d) {
      // Production's wording verbatim (`crates/brink-web/src/editor/refactor.rs`,
      // `auto_import_apply_include_doc`); pinned by the driven `messages` map in
      // `crates/brink-web/fixtures/refusal-shapes.json` (#2603).
      return EditorSession.autoImportRefusal("unknown document handle");
    }
    if (this.readOnlyPaths.has(d.path)) {
      // #2621. Wording driven, not typed — see the fixture's
      // `auto_import_apply_include_doc:read-only-mount`. Ordered after the
      // unknown-handle check exactly as production orders it.
      return EditorSession.autoImportRefusal(
        "document handle is read-only (mounted stdlib file)",
      );
    }
    const source = this.files.get(d.path) ?? "";
    const base = target.split("/").pop()!;
    const reachable = new RegExp(`^INCLUDE\\s+\\S*${base.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "m").test(source);
    if (reachable) {
      return JSON.stringify({ ok: true, already_reachable: true });
    }
    const insert = `INCLUDE ${base}\n`;
    const at = 0;
    // Apply at file top.
    this.files.set(d.path, insert + source);
    // Rebase every open fragment view on this file whose range starts at/after
    // the insertion point.
    const delta = insert.length;
    for (const od of this.docs.values()) {
      if (od.path !== d.path) continue;
      if (od.viewStart != null && od.viewStart >= at) od.viewStart += delta;
      if (od.viewEnd != null && od.viewEnd >= at) od.viewEnd += delta;
    }
    return JSON.stringify({
      ok: true,
      already_reachable: false,
      edit: { from: at, to: at, insert },
    });
  }

  get_view_source_doc(doc: number): string {
    const d = this.docs.get(doc);
    if (!d) return JSON.stringify(null);
    const content = this.files.get(d.path);
    if (content == null) return JSON.stringify(null);
    if (d.viewStart != null && d.viewEnd != null) {
      return JSON.stringify(content.slice(d.viewStart, d.viewEnd));
    }
    return JSON.stringify(content);
  }

  line_contexts_doc(_doc: number): string { return "[]"; }
  semantic_tokens_doc(_doc: number): string { return "[]"; }
  hir_spans_doc(_doc: number): string { return mockHirProjectionJson; }
  completions_doc(_doc: number, _offset: number): string { return "[]"; }
  hover_doc(_doc: number, _offset: number): string { return "null"; }
  explain_match_doc(_doc: number, _offset: number): string { return "null"; }
  goto_definition_doc(_doc: number, _offset: number): string { return "null"; }
  find_references_doc(_doc: number, _offset: number): string { return "[]"; }
  prepare_rename_doc(_doc: number, _offset: number): string { return "null"; }
  code_actions_doc(_doc: number, _offset: number): string {
    // One synthetic entry — enough to let a test open the real code-actions
    // menu and select something. Its `data: {}` carries no `action`
    // discriminator, so resolving it lands on the missing-discriminator
    // refusal in `resolveCodeActionImpl`; that is what lets
    // `code-actions-apply-reachability.test.ts` (#2578) prove `applyCodeAction`
    // forwards a refusal to `onApplyStructural` UNCONDITIONALLY. This method's
    // own shape (title/kind) is not under test.
    //
    // ⚠ `resolve_code_action_doc` is NOT defined here. It lives with its
    // `resolve_code_action` sibling above, sharing `resolveCodeActionImpl` so
    // both handle variants model production identically. A second definition
    // here silently overrode that one between #2583 and #2585 — see the
    // regression note on `resolve_code_action_doc` itself.
    return JSON.stringify([{ title: "Mock quickfix", kind: "quickfix", data: {} }]);
  }
  inlay_hints_doc(_doc: number, _start: number, _end: number): string { return "[]"; }
  signature_help_doc(_doc: number, _offset: number): string { return "null"; }
  folding_ranges_doc(_doc: number): string { return "[]"; }
  document_symbols_doc(_doc: number): string { return "[]"; }
  convert_element_doc(_doc: number, _offset: number, _target: string): string { return "null"; }
  format_document_doc(_doc: number): string { return '""'; }

  /** Outline-shaped symbols for one file (used by symbol-range resolution). */
  file_symbols(path: string): string {
    const source = this.files.get(path);
    return JSON.stringify(source == null ? [] : parseOutline(source));
  }

  semantic_tokens(): string { return "[]"; }
  completions(_offset: number): string { return "[]"; }
  hover(_offset: number): string { return "null"; }
  explain_match(_offset: number): string { return "null"; }
  goto_definition(_offset: number): string { return "null"; }
  find_references(_offset: number): string { return "[]"; }
  prepare_rename(_offset: number): string { return "null"; }
  rename(_offset: number, _name: string): string { return "[]"; }
  code_actions(_offset: number): string { return "[]"; }
  inlay_hints(_start: number, _end: number): string { return "[]"; }
  signature_help(_offset: number): string { return "null"; }
  folding_ranges(): string { return "[]"; }
  document_symbols(): string { return "[]"; }
  file_includes(_path: string): string { return "[]"; }
  line_contexts(): string { return "[]"; }
  format_document(): string { return '""'; }
  convert_element(_offset: number, _target: string): string { return "null"; }
  free(): void { /* no-op */ }
}

/**
 * Test hook (#494): the projection `hir_spans_doc` returns. Defaults to the
 * empty projection — mirroring a real session before its first
 * compile/analysis completes. Tests set a populated projection to simulate
 * analysis finishing, then reset in their afterEach.
 */
const EMPTY_HIR_PROJECTION = '{"spans":[],"lines":[]}';
let mockHirProjectionJson = EMPTY_HIR_PROJECTION;

export function setMockHirProjection(json: string | null): void {
  mockHirProjectionJson = json ?? EMPTY_HIR_PROJECTION;
}

export function compile(_source: string): string {
  return JSON.stringify({ ok: true });
}

/** Deterministic stand-in for the source-identity checksum: a stable hash of
 * the bytes, formatted like the real `0x{:08x}` — distinct bytes → distinct
 * value, so degraded-mode comparisons behave. */
export function program_checksum(bytes: Uint8Array): string {
  let sum = 0;
  for (const b of bytes) sum = (sum + b) >>> 0;
  return "0x" + sum.toString(16).padStart(8, "0");
}

export function token_type_names(): string {
  return JSON.stringify(["comment", "keyword", "string", "number", "function", "variable"]);
}

export function token_modifier_names(): string {
  return JSON.stringify([]);
}

export class StoryRunner {
  constructor(_bytes: Uint8Array) { /* no-op */ }
  continue_story(): string { return JSON.stringify([{ type: "end", text: "", tags: [] }]); }
  continue_single(): string { return JSON.stringify({ type: "end", text: "", tags: [] }); }
  choose(_index: number): void { /* no-op */ }
  reset(): void { /* no-op */ }
  free(): void { /* no-op */ }
  // Replay-recording surface (mirrors the real StoryRunner; #173/#189): the
  // mock records nothing, so has_recording() is always false → the studio's
  // post-reload re-walk runs live, exactly as before this feature.
  reload(_bytes: Uint8Array): void { /* no-op */ }
  begin_replay(): void { /* no-op */ }
  end_replay(): void { /* no-op */ }
  has_recording(): boolean { return false; }
  // #1573: `didSafeExit` wrapper passthrough — the mock story never reaches
  // an explicit `-> DONE`, so this is always false.
  did_safe_exit(): boolean { return false; }
  // Shared-flow surface (#200): a minimal in-memory flow registry so the studio
  // multi-flow path is exercisable. Each flow ends immediately, like the mock
  // story.
  private flows = new Set<string>();
  spawn_flow(name: string, _path?: string): void { this.flows.add(name); }
  continue_flow(_name: string): string { return JSON.stringify({ type: "end", text: "", tags: [] }); }
  choose_flow(_name: string, _index: number): void { /* no-op */ }
  destroy_flow(name: string): void { this.flows.delete(name); }
  flow_names(): string { return JSON.stringify([...this.flows].sort()); }
  flow_debug_snapshot(_name: string): string {
    return JSON.stringify({
      status: "ended", current_location: null, turn_index: 0,
      globals: [], call_stack: [], visit_counts: [], pending_choices: [],
      rng: { seed: 0, previous: 0 },
    });
  }
}

/** Pure-diff stand-in for the real `diffSnapshots` wasm export — the mock
 * carries no snapshot state, so this exists only so `StorySessionHandle`'s
 * import resolves; nothing in the studio test suite calls it against real
 * snapshot data. */
export function diffSnapshots(_a: string, _b: string): string {
  return JSON.stringify({
    added_globals: {}, removed_globals: {}, changed_globals: {},
    list_deltas: {}, pushed_frames: [], popped_frames: [],
  });
}

/**
 * Minimal stand-in for the real `WebSession` (#390's `StorySessionHandle`
 * over `crates/brink-web`). Every journal-mutating call bumps an in-memory
 * event counter one-for-one — enough to exercise `StorySessionHandle`'s
 * TS-side deferred+debounced `onJournalDirty` hook (the behavior under test)
 * without reimplementing the Rust session/journal semantics. Story content is
 * a fixed two-line-then-`done` script; it does not parse `_storyBytes`.
 */
export class WebSession {
  private events = 0;
  private turn = 0;
  private flows = new Set<string>();

  constructor(_storyBytes: Uint8Array, _seed?: number, _deferred?: string[]) { /* no-op */ }

  // ── Program inspection (#388) ──────────────────────────────────
  debug_snapshot(): string {
    return JSON.stringify({
      status: this.turn === 0 ? "active" : "ended",
      current_location: null,
      turn_index: this.turn,
      globals: [],
      call_stack: [],
      visit_counts: [],
      pending_choices: [],
      rng: { seed: 0, previous: 0 },
    });
  }
  program_inkt(): string {
    return "";
  }
  program_model(): string {
    return JSON.stringify({
      checksum: "0xmock0000",
      globals: [],
      lists: [],
      externals: [],
      knots: [],
    });
  }

  // ── Shared flows (#388 mirror of StoryRunner's) ─────────────────
  spawn_flow(name: string, _path?: string): void {
    this.flows.add(name);
  }
  continue_flow(_name: string): string {
    return JSON.stringify({ type: "end", text: "", tags: [] });
  }
  choose_flow(_name: string, _index: number): void { /* no-op */ }
  destroy_flow(name: string): void {
    this.flows.delete(name);
  }
  flow_names(): string {
    return JSON.stringify([...this.flows].sort());
  }
  flow_debug_snapshot(_name: string): string {
    return JSON.stringify({
      status: "ended", current_location: null, turn_index: 0,
      globals: [], call_stack: [], visit_counts: [], pending_choices: [],
      rng: { seed: 0, previous: 0 },
    });
  }

  private bumpAndLine(): string {
    this.events += 1;
    this.turn += 1;
    if (this.turn === 1) {
      return JSON.stringify({
        type: "line",
        line: { type: "text", text: "Hello, world!\n", tags: [] },
      });
    }
    return JSON.stringify({
      type: "line",
      line: { type: "done", text: "", tags: [] },
    });
  }

  advance(): string { return this.bumpAndLine(); }
  continue_single(): string {
    const outcome = JSON.parse(this.bumpAndLine()) as { line: unknown };
    return JSON.stringify(outcome.line);
  }
  continue_to_pause(): string {
    const outcome = JSON.parse(this.bumpAndLine()) as { line: unknown };
    return JSON.stringify([outcome.line]);
  }
  choose(_index: number): void { this.events += 1; }
  resolve_external(_value: unknown): void { this.events += 1; }
  has_pending_external(): boolean { return false; }
  // #1573: `didSafeExit` wrapper passthrough — the mock story never reaches
  // an explicit `-> DONE`, so this is always false.
  did_safe_exit(): boolean { return false; }
  set_var(_name: string, _value: unknown): boolean { this.events += 1; return true; }
  go_to_path(_path: string, _args: unknown[]): void { this.events += 1; }
  save_state(): string { return JSON.stringify({ globals: {}, visited: [], turn_index: this.turn }); }
  load_state(_json: string): void { this.events += 1; }
  call_function(_name: string, _args: unknown[]): unknown { this.events += 1; return null; }
  snapshot(): string {
    return JSON.stringify({
      globals: {}, lists: {}, turn_index: this.turn, visit_counts: {},
      turn_counts: {}, call_stack: [], status: "active",
    });
  }
  diff(a: string, b: string): string { return diffSnapshots(a, b); }
  journal_event_count(): number { return this.events; }
  export_journal(): string {
    return JSON.stringify({
      version: 1, program_checksum: 0, events: [], truncated: false,
    });
  }
  static restore(
    _storyBytes: Uint8Array,
    _journalJson: string,
    _seed?: number,
    _deferred?: string[],
  ): WebSession {
    return new WebSession(_storyBytes, _seed, _deferred);
  }
  last_replay_outcome(): string | undefined { return undefined; }
  reload(_storyBytes: Uint8Array): string {
    this.events += 1;
    return JSON.stringify({ type: "replayed", warnings: [] });
  }
  continue_replay(): string {
    this.events += 1;
    return JSON.stringify({ type: "replayed", warnings: [] });
  }
  restart(): void { this.events = 0; this.turn = 0; }
  free(): void { /* no-op */ }
}
