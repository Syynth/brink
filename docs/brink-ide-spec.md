# brink-ide specification

`brink-ide` is a protocol-agnostic query layer that provides IDE intelligence (navigation, completion, hover, semantic tokens, refactoring, etc.) for ink source files. It depends on `brink-db`, `brink-ir`, `brink-syntax`, `brink-analyzer`, and `brink-fmt`. It does NOT depend on any async runtime, LSP protocol types, or platform-specific APIs.

See also: [compiler-spec](compiler-spec.md) (pipeline that produces the analysis data brink-ide queries), [runtime-spec](runtime-spec.md) (execution of compiled stories), [brink-driver-spec](brink-driver-spec.md) (orchestration layer that brink-ide's shells use to keep analysis current).

## Motivation

The LSP backend (`brink-lsp/src/backend.rs`, ~3100 lines) contains all IDE intelligence tightly coupled to `tower-lsp` types and `tokio` concurrency primitives. This coupling prevents reuse in other contexts:

1. **Web editor** — a planned wasm-based editor (`brink-web`) needs the same IDE features (goto-def, completions, hover, semantic tokens) but cannot use tower-lsp or tokio. The entire compile+run pipeline already compiles clean for `wasm32-unknown-unknown`.
2. **LSP maintainability** — the monolithic backend mixes protocol dispatch, concurrency management, and query logic. Extracting queries into a shared layer makes the LSP a thin adapter.
3. **Testing** — protocol-agnostic query functions are directly testable without standing up an LSP server.

## Architecture

### Layer separation

```
┌─────────────────────────────────────────────────────────┐
│  SHELLS (protocol-specific, own concurrency)            │
│  brink-lsp: tower-lsp + tokio, URI handling, file I/O   │
│  brink-web: wasm-bindgen, JsValue serialization          │
├─────────────────────────────────────────────────────────┤
│  brink-ide (this crate)                                  │
│  Protocol-agnostic query functions                       │
│  Domain types (HoverInfo, CompletionItem, SemanticToken) │
│  LineIndex (byte offset <-> line/col conversion)         │
├─────────────────────────────────────────────────────────┤
│  brink-driver (orchestration)                            │
│  File discovery, cross-file analysis, diagnostics        │
├─────────────────────────────────────────────────────────┤
│  brink-db / brink-analyzer / brink-ir / brink-syntax     │
│  Data storage, analysis, IR types, parsing               │
└─────────────────────────────────────────────────────────┘
```

### Dependency graph position

```
TIER 5 (products):
    brink-ide → brink-driver, brink-db, brink-syntax, brink-ir, brink-analyzer, brink-fmt

TIER 6 (shells):
    brink-lsp → brink-ide, tower-lsp, tokio
    brink-web → brink-ide, brink-compiler, brink-runtime, wasm-bindgen
```

brink-ide sits at the same tier as `brink-compiler`. It is a product crate that composes lower-tier crates to provide user-facing functionality.

### What brink-ide is NOT

- **Not an async runtime.** All functions are synchronous. Concurrency (locks, channels, debouncing) is the shell's responsibility.
- **Not a protocol adapter.** It returns domain types, not LSP types or JsValues.
- **Not a file system accessor.** It takes data (source text, analysis results, parse trees) as inputs. It never reads from disk.
- **Stateless queries, stateful session.** All query functions are pure functions of their inputs. `IdeSession` is a stateful wrapper that manages the parse/analyze lifecycle and delegates to these pure functions. Shells should use `IdeSession` rather than orchestrating `ProjectDb` + analysis + query calls themselves. See [IdeSession](#idesession).

## Wasm compatibility

brink-ide MUST compile for `wasm32-unknown-unknown` without feature flags or conditional compilation. This means:

- No `std::fs`, `std::path::Path`, `std::net`, `std::process`
- No `tokio`, `async-std`, or any async runtime
- No `tower-lsp` or other protocol-specific dependencies
- No platform-specific code (the `resolve_include_path` fix in brink-driver handles the `Path` separator issue)
- `rowan::TextSize` / `TextRange` are fine (pure arithmetic)

## Snapshot pattern

### `QueryInput`

Every brink-ide query function takes its required data as plain arguments — there is no `QueryInput` struct that all queries share. Different queries need different subsets of data:

- **Navigation queries** (goto-def, references, rename) need: `AnalysisResult`, source text, `FileId`, and project file metadata
- **Completion queries** need: `AnalysisResult`, source text, byte offset
- **Semantic token queries** need: `AnalysisResult`, source text, `SyntaxNode` root, `FileId`
- **Document structure queries** (symbols, folding) need: `HirFile`, source text, `SymbolManifest`
- **Formatting queries** need: source text only
- **Hover/signature help** need: `AnalysisResult`, source text, `FileId`, project file metadata

The shell is responsible for acquiring this data (e.g., taking a lock on `ProjectDb`, cloning out the needed pieces, releasing the lock, then calling brink-ide). This keeps concurrency concerns entirely in the shell layer.

**Rationale:** A single snapshot struct would force every query to carry data it doesn't need, and would create a coupling point between the shell's data model and brink-ide's API. By accepting plain arguments, each query documents exactly what it needs, and the shell can optimize its locking strategy per-query.

### Project file metadata

Several queries need to map a `FileId` to a file path and source text (e.g., goto-def needs to locate the target file, rename needs to produce edits across files). This is provided as:

```rust
/// (FileId, path, source) tuples for files in the current project.
type ProjectFiles = [(FileId, String, String)];
```

The shell constructs this from `ProjectDb` under lock and passes it by reference.

## IdeSession

`IdeSession` is the recommended entry point for all IDE operations. It owns a `ProjectDb` and cached `AnalysisResult`, managing the parse/lower/analyze lifecycle so shells do not reimplement it.

The pure query functions in the [API surface](#api-surface) remain public for unit testing and for callers that already have pre-computed analysis data. However, brink-ide does **not** expose any public convenience helpers that take raw source strings and perform the full parse + lower + analyze pipeline. The expensive orchestration lives exclusively inside `IdeSession`. This is deliberate: making it easy to reparse from scratch on every call is exactly the performance problem that `IdeSession` exists to solve.

**Rationale:** brink-web's current architecture calls `analyze_source(source)` on every IDE query -- full parse, HIR lower, and cross-file analysis from scratch. This pattern emerged because the stateless function API makes it trivially easy to do the wrong thing. `IdeSession` makes the right thing (cached, incremental updates) the path of least resistance.

### API

```rust
pub struct IdeSession {
    db: ProjectDb,
    analysis: AnalysisResult,
}

impl IdeSession {
    /// Create an empty session.
    pub fn new() -> Self;

    /// Update source for a file. Triggers parse + HIR lower in the ProjectDb.
    /// Does NOT re-analyze -- call `update_and_analyze` for single-threaded use,
    /// or use the snapshot pattern for async shells.
    pub fn update_source(&mut self, path: &str, source: String);

    /// Take a snapshot of the current state for off-thread analysis.
    /// The snapshot contains cloned analysis inputs (HIR + manifests).
    /// The caller can release &mut self, analyze the snapshot, then
    /// apply results back.
    pub fn snapshot(&self) -> IdeSnapshot;

    /// Apply analysis results computed from a snapshot.
    pub fn apply_analysis(&mut self, result: AnalysisResult);

    /// Convenience: update source + re-analyze in one call.
    /// For single-threaded contexts (wasm) where there is no lock contention.
    pub fn update_and_analyze(&mut self, path: &str, source: String);

    // -- Query methods --
    // Each resolves path to FileId, retrieves cached data from
    // ProjectDb + AnalysisResult, and delegates to the corresponding
    // pure function.

    pub fn hover(&self, path: &str, offset: TextSize) -> Option<HoverInfo>;
    pub fn completions(&self, path: &str, offset: usize) -> Vec<CompletionItem>;
    pub fn goto_definition(&self, path: &str, offset: TextSize) -> Option<LocationResult>;
    pub fn find_references(
        &self, path: &str, offset: TextSize, include_declaration: bool,
    ) -> Vec<LocationResult>;
    pub fn semantic_tokens(&self, path: &str) -> Vec<SemanticToken>;
    pub fn line_contexts(&self, path: &str) -> Vec<LineContext>;
    pub fn document_symbols(&self, path: &str) -> Vec<DocumentSymbol>;
    pub fn folding_ranges(&self, path: &str) -> Vec<FoldRange>;
    pub fn inlay_hints(&self, path: &str, range: TextRange) -> Vec<InlayHint>;
    pub fn signature_help(&self, path: &str, offset: usize) -> Option<SignatureInfo>;
    pub fn prepare_rename(&self, path: &str, offset: TextSize) -> Option<TextRange>;
    pub fn rename(
        &self, path: &str, offset: TextSize, new_name: &str,
    ) -> Option<RenameResult>;
    pub fn code_actions(&self, path: &str, offset: usize) -> Vec<CodeAction>;
    // ... remaining queries follow the same pattern
}
```

### IdeSnapshot

`IdeSnapshot` supports the LSP's lock-release-analyze pattern. It captures the analysis inputs (cloned HIR + manifests) so analysis can run without holding `&mut IdeSession`.

```rust
pub struct IdeSnapshot {
    inputs: Vec<(FileId, HirFile, SymbolManifest)>,
}

impl IdeSnapshot {
    /// Run cross-file analysis on the snapshot's inputs.
    /// This is the expensive operation that the LSP runs off-thread.
    pub fn analyze(&self) -> AnalysisResult;
}
```

### Shell usage patterns

**brink-web (single-threaded wasm):**

```rust
// On document change:
session.update_and_analyze("main.ink", new_source);

// Queries read from cached state -- no reparse:
let tokens = session.semantic_tokens("main.ink");
let contexts = session.line_contexts("main.ink");
```

**brink-lsp (async, multi-threaded):**

```rust
// In didChange handler:
{
    let mut session = self.session.lock();
    session.update_source(&path, source);  // parse + lower only
    let snap = session.snapshot();
}   // lock released -- other queries can proceed

// Off-thread analysis (expensive, no lock held):
let result = snap.analyze();

// Apply results:
{
    let mut session = self.session.lock();
    session.apply_analysis(result);
}
```

The LSP wraps `IdeSession` in `Arc<Mutex<_>>` (or equivalent). The snapshot pattern lets it release the lock during the expensive analysis pass, keeping the session responsive to concurrent queries.

### What brink-web changes

With `IdeSession`, brink-web's wasm exports become thin methods on a `#[wasm_bindgen]` struct that owns an `IdeSession`:

```rust
#[wasm_bindgen]
pub struct EditorSession {
    session: IdeSession,
}

#[wasm_bindgen]
impl EditorSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self;

    /// Call on every document change. Reparses only the changed file.
    pub fn update_source(&mut self, source: &str);

    /// Returns JSON-serialized Vec<LineContext>.
    pub fn line_contexts(&self) -> String;

    /// Returns JSON-serialized Vec<SemanticToken>.
    pub fn semantic_tokens(&self) -> String;

    // ... all other IDE queries delegate to self.session
}
```

The existing stateless `pub fn semantic_tokens(source: &str) -> String` free functions in brink-web are removed. The `compile()` free function and `StoryRunner` struct remain unchanged -- compilation and runtime are separate concerns.

On the TypeScript side, `packages/brink-studio/src/wasm.ts` replaces its collection of `getSemanticTokens(source)`, `getCompletions(source, offset)`, etc. with methods on the `EditorSession` wasm object. The `BrinkStudioOptions` interface in `extensions.ts` changes accordingly: instead of passing individual callback functions, the editor receives an `EditorSession` handle.

## Domain types

brink-ide defines its own result types. These are plain Rust structs with no protocol dependencies. Shells convert them to protocol-specific representations (LSP types, JsValues, etc.).

### Position and range types

brink-ide uses `rowan::TextSize` and `rowan::TextRange` for byte-level positions internally. The `LineIndex` type (moved from brink-lsp's `convert.rs`) handles conversion between byte offsets and `(line, column)` pairs.

```rust
/// Maps byte offsets to (line, col) positions.
/// Columns are measured in UTF-16 code units (matching LSP and Monaco conventions).
pub struct LineIndex { .. }

impl LineIndex {
    pub fn new(source: &str) -> Self;
    pub fn line_col(&self, offset: TextSize) -> (u32, u32);
    pub fn offset(&self, line: u32, col: u32) -> TextSize;
}
```

**Rationale:** Both LSP and Monaco (the web editor's likely editor component) use UTF-16 code units for column positions. By having `LineIndex` in brink-ide, both shells get the same correct UTF-16 handling without duplicating the logic.

### Navigation results

```rust
/// Result of a goto-definition or find-references lookup.
pub struct LocationResult {
    pub file: FileId,
    pub range: TextRange,
}

/// A text edit in a specific file.
pub struct FileEdit {
    pub file: FileId,
    pub range: TextRange,
    pub new_text: String,
}

/// Result of a rename operation.
pub struct RenameResult {
    pub edits: Vec<FileEdit>,
}
```

### Hover

```rust
pub struct HoverInfo {
    /// Symbol kind label (e.g., "knot", "stitch", "variable").
    pub kind: String,
    /// Symbol name.
    pub name: String,
    /// Formatted parameter list, e.g., "(ref x, -> target)". Empty if none.
    pub params: String,
    /// Additional detail (e.g., "function" for function knots).
    pub detail: Option<String>,
    /// Path of the file where this symbol is defined.
    pub defined_in: Option<String>,
    /// Range of the hovered word/symbol for highlighting.
    pub range: Option<TextRange>,
}
```

### Completion

```rust
pub enum CompletionContext {
    /// After `->` — show divert targets.
    Divert,
    /// After `knot_name.` — show children of that knot.
    DottedPath { knot: String },
    /// Inside `{ }` — inline expression.
    InlineExpr,
    /// On a `~` logic line.
    Logic,
    /// Inside `( )` — function arguments.
    FunctionArgs,
    /// Default — show everything.
    General,
}

pub struct CompletionItem {
    /// Display label.
    pub label: String,
    /// Symbol kind for icon/sorting.
    pub kind: SymbolKind,
    /// Optional detail text (e.g., parameter list).
    pub detail: Option<String>,
    /// Text to insert if different from label.
    pub insert_text: Option<String>,
}

/// Scope context for filtering completions.
pub struct CursorScope {
    pub knot: Option<String>,
    pub stitch: Option<String>,
}
```

### Signature help

```rust
pub struct SignatureInfo {
    /// Full signature label, e.g., "my_func(x, ref y)".
    pub label: String,
    /// Documentation string, if available.
    pub documentation: Option<String>,
    /// Parameter labels for highlighting.
    pub parameters: Vec<ParamLabel>,
    /// Index of the currently active parameter.
    pub active_parameter: u32,
}

pub struct ParamLabel {
    pub label: String,
}
```

### Semantic tokens

```rust
/// A classified token with absolute position.
pub struct SemanticToken {
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
    pub token_type: TokenType,
    pub modifiers: TokenModifiers,
}

/// Semantic token classification.
#[repr(u32)]
pub enum TokenType {
    Namespace = 0,   // knots
    Function = 1,    // stitches, externals
    Variable = 2,    // variables
    String = 3,      // string content
    Number = 4,      // numeric literals
    Keyword = 5,     // VAR, CONST, LIST, etc.
    Operator = 6,    // ->, <-, ~, etc.
    Comment = 7,     // // and /* */
    Enum = 8,        // list names
    EnumMember = 9,  // list items
    Parameter = 10,  // function/knot params
    Decorator = 11,  // tags (#)
    Label = 12,      // labels, gather names
}

bitflags! {
    pub struct TokenModifiers: u32 {
        const DECLARATION = 1 << 0;
        const DEFINITION  = 1 << 1;
        const READONLY    = 1 << 2;
        const DEPRECATED  = 1 << 3;
    }
}

/// A delta-encoded token (for wire transmission).
pub struct DeltaToken {
    pub delta_line: u32,
    pub delta_start: u32,
    pub length: u32,
    pub token_type: u32,
    pub token_modifiers: u32,
}
```

**Rationale:** `TokenType` is an enum (not bare `u32`) for type safety within brink-ide. The `repr(u32)` ensures shells can cheaply convert to protocol-specific numeric indices. The delta encoding function is also in brink-ide since both LSP and web editors use the same encoding scheme.

### Document structure

```rust
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub detail: Option<String>,
    pub range: TextRange,
    pub children: Vec<DocumentSymbol>,
}

pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: FileId,
    pub range: TextRange,
}
```

### Line context

Per-line structural context for the editor's screenplay mode. Produced by `line_contexts()`, this replaces the regex-based `classifyLine` in TypeScript with authoritative data from the HIR and parse tree.

```rust
/// Per-line context combining syntactic classification, structural position,
/// and inline element ranges.
pub struct LineContext {
    /// What kind of line this is (syntactic classification).
    pub element: LineElement,
    /// Where this line sits in the weave structure (from HIR).
    pub weave: WeavePosition,
    /// Byte ranges of inline `# tag` annotations on this line.
    pub tags: Vec<(u32, u32)>,
    /// Byte range if this line is inside a `/* ... */` block comment.
    pub block_comment: Option<(u32, u32)>,
    /// Byte ranges of `[...]` choice bracket content on this line.
    pub brackets: Vec<(u32, u32)>,
    /// Byte ranges of `{expr}` inline expressions on this line.
    pub inline_exprs: Vec<(u32, u32)>,
}

/// Syntactic line classification. Identifies what kind of ink construct
/// occupies a source line, independent of its structural position.
pub enum LineElement {
    KnotHeader,
    StitchHeader,
    Narrative,
    Choice,
    Gather,
    Divert,
    Logic,
    VarDecl,
    Comment,
    Include,
    External,
    Tag,
    Blank,
}

/// Structural position within the weave hierarchy.
/// Depth + element type describe where the cursor sits in ink's
/// nested choice/gather tree.
pub struct WeavePosition {
    /// Weave nesting depth. 0 = top-level (outside any choice/gather).
    /// 1 = first-level choices, 2 = nested choices, etc.
    /// Canonical value from `ChoiceSet.depth` in the HIR.
    pub depth: u32,
    /// What kind of structural container this line is inside.
    pub element: WeaveElement,
}

/// The structural container type for a position in the weave hierarchy.
pub enum WeaveElement {
    /// Top-level content (knot/stitch body, outside any choice set).
    TopLevel,
    /// A choice line (`*` or `+`).
    ChoiceLine { sticky: bool },
    /// Body text inside a choice (the lines after the choice, before
    /// the next choice or gather at the same depth).
    ChoiceBody,
    /// Continuation after a gather (`-`). The gather itself and any
    /// content that follows it before the next structural break.
    GatherContinuation,
    /// Inside a branch of a conditional (`{condition: ... | ...}`).
    ConditionalBranch,
    /// Inside a branch of a sequence (`{&|~|!} ...`).
    SequenceBranch,
}
```

`LineElement` and `WeavePosition` are orthogonal. A line classified as `LineElement::Narrative` can have `WeaveElement::TopLevel` (plain narrative in a knot body), `WeaveElement::ChoiceBody` (narrative after a choice), or `WeaveElement::GatherContinuation` (narrative after a gather). The editor uses `LineElement` for visual styling (font, sigil treatment) and `WeavePosition` for structural operations (depth indentation, transition table, status bar display).

### Folding

```rust
pub enum FoldKind {
    Region,
    Comment,
    Imports,
}

pub struct FoldRange {
    pub start_line: u32,
    pub end_line: u32,
    pub kind: FoldKind,
    pub collapsed_text: Option<String>,
}
```

### Inlay hints

```rust
pub struct InlayHint {
    pub line: u32,
    pub col: u32,
    pub label: String,
    pub kind: InlayHintKind,
    pub padding_right: bool,
}

pub enum InlayHintKind {
    Parameter,
    Type,
}
```

### Code actions

```rust
pub struct CodeAction {
    pub title: String,
    pub kind: CodeActionKind,
    /// Opaque data for deferred resolution (e.g., knot name, action type).
    pub data: CodeActionData,
}

pub enum CodeActionKind {
    QuickFix,
    Refactor,
    Source,
}

pub enum CodeActionData {
    SortKnots,
    SortStitches { knot: String },
    FormatKnot { knot: String },
    FormatStitch { knot: String, stitch: String },
}

/// Result of resolving a code action.
pub struct CodeActionEdit {
    /// The full new source text after applying the action.
    pub new_source: String,
}
```

### Text edits

```rust
/// A text replacement within a single source string.
/// Uses byte-range positions (TextRange). The shell converts to line/col
/// via LineIndex for protocol-specific output.
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}
```

### Formatting

```rust
/// Result of formatting, expressed as a before/after diff.
pub struct FormatResult {
    /// The formatted source text.
    pub formatted: String,
    /// Whether the source was already formatted (no changes needed).
    pub unchanged: bool,
}
```

## API surface

All public functions are synchronous, take borrowed data, and return owned domain types.

### LineIndex

```rust
pub fn line_index(source: &str) -> LineIndex;
```

### Navigation

```rust
/// Find the definition of the symbol at the given byte offset.
pub fn goto_definition(
    analysis: &AnalysisResult,
    file_id: FileId,
    source: &str,
    offset: TextSize,
) -> Option<LocationResult>;

/// Find all references to the symbol at the given byte offset.
/// If `include_declaration` is true, includes the definition site.
pub fn find_references(
    analysis: &AnalysisResult,
    file_id: FileId,
    source: &str,
    offset: TextSize,
    include_declaration: bool,
) -> Vec<LocationResult>;

/// Check whether the symbol at `offset` can be renamed.
/// Returns the range of the symbol if rename is valid.
pub fn prepare_rename(
    analysis: &AnalysisResult,
    file_id: FileId,
    source: &str,
    offset: TextSize,
) -> Option<TextRange>;

/// Compute all edits needed to rename the symbol at `offset` to `new_name`.
pub fn rename(
    analysis: &AnalysisResult,
    file_id: FileId,
    source: &str,
    offset: TextSize,
    new_name: &str,
) -> Option<RenameResult>;
```

### Hover and signature help

```rust
/// Compute hover information for the symbol at `offset`.
pub fn hover(
    analysis: &AnalysisResult,
    file_id: FileId,
    source: &str,
    offset: TextSize,
    project_files: &[(FileId, String, String)],
) -> Option<HoverInfo>;

/// Find the function call context at `byte_offset` for signature help.
pub fn signature_help(
    analysis: &AnalysisResult,
    source: &str,
    byte_offset: usize,
) -> Option<SignatureInfo>;
```

### Completion

```rust
/// Detect the completion context at the given byte offset.
pub fn detect_completion_context(source: &str, byte_offset: usize) -> CompletionContext;

/// Determine the knot/stitch scope containing the cursor.
pub fn cursor_scope(source: &str, byte_offset: usize) -> CursorScope;

/// Check whether a symbol should be shown in the given completion context.
pub fn is_visible_in_context(
    ctx: &CompletionContext,
    info: &SymbolInfo,
    scope: &CursorScope,
) -> bool;

/// Compute completion items for the given context.
pub fn completions(
    analysis: &AnalysisResult,
    source: &str,
    byte_offset: usize,
) -> Vec<CompletionItem>;
```

### Semantic tokens

```rust
/// Compute absolute-position semantic tokens for a file.
pub fn semantic_tokens(
    source: &str,
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
) -> Vec<SemanticToken>;

/// Compute semantic tokens for a line range.
pub fn semantic_tokens_range(
    source: &str,
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    file_id: FileId,
    start_line: u32,
    end_line: u32,
) -> Vec<SemanticToken>;

/// Delta-encode a list of absolute-position tokens.
pub fn delta_encode(tokens: &[SemanticToken]) -> Vec<DeltaToken>;

/// The token type and modifier lists, for legend construction by the shell.
pub fn token_type_names() -> &'static [&'static str];
pub fn token_modifier_names() -> &'static [&'static str];
```

### Document structure

```rust
/// Compute document symbols from HIR.
pub fn document_symbols(
    hir: &HirFile,
    manifest: &SymbolManifest,
) -> Vec<DocumentSymbol>;

/// Search workspace symbols across all projects.
pub fn workspace_symbols(
    analyses: &[&AnalysisResult],
    query: &str,
) -> Vec<WorkspaceSymbol>;

/// Compute folding ranges from HIR.
pub fn folding_ranges(
    hir: &HirFile,
    source: &str,
) -> Vec<FoldRange>;
```

### Inlay hints

```rust
/// Compute parameter name inlay hints for a file.
pub fn inlay_hints(
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    source: &str,
    range: TextRange,
) -> Vec<InlayHint>;
```

### Code actions

```rust
/// Collect applicable code actions at the cursor position.
pub fn code_actions(
    source: &str,
    cursor_line: u32,
    cursor_col: u32,
) -> Vec<CodeAction>;

/// Resolve a code action, producing the full edit.
pub fn resolve_code_action(
    source: &str,
    action: &CodeActionData,
) -> Option<CodeActionEdit>;
```

### Formatting

```rust
/// Format an entire document.
pub fn format_document(source: &str) -> FormatResult;

/// Format a specific knot or stitch region.
pub fn format_region(
    source: &str,
    knot_name: &str,
    stitch_name: Option<&str>,
) -> FormatResult;
```

### Text utilities

```rust
/// Compute text edits between two source texts.
/// Returns edits with byte-range positions (TextRange). The shell converts
/// to line/col via LineIndex for protocol-specific output.
pub fn diff_to_edits(old: &str, new: &str) -> Vec<TextEdit>;

/// A text replacement within a single source string.
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

/// Extract the word at the given byte offset.
pub fn word_at_offset(source: &str, offset: TextSize) -> Option<&str>;

/// Get the TextRange of the word at the given byte offset.
pub fn word_range_at_offset(source: &str, offset: TextSize) -> Option<TextRange>;

/// Return hover markdown for a built-in function, or None.
pub fn builtin_hover_text(name: &str) -> Option<String>;
```

### Sorting

```rust
/// Sort knot definitions alphabetically.
pub fn sort_knots(source: &str) -> String;

/// Sort stitch definitions within a knot alphabetically.
pub fn sort_stitches(source: &str, knot_name: &str) -> String;
```

### Line context

```rust
/// Compute per-line structural context by walking the HIR tree top-down
/// and consulting the parse tree for inline element spans.
///
/// Returns one `LineContext` per source line (indexed by 0-based line number).
/// The implementation uses `AstPtr`/`SyntaxNodePtr` spans on HIR nodes to
/// determine which source lines each node covers, producing `WeavePosition`
/// from HIR structure and inline ranges from the `SyntaxNode` tree in a
/// single pass.
pub fn line_contexts(
    hir: &HirFile,
    source: &str,
    root: &SyntaxNode,
) -> Vec<LineContext>;
```

`cursor_scope` (knot/stitch scope for completion visibility) remains a separate function. It answers "what symbols are visible here?" while `line_contexts` answers "what structural position is this line in?" These are orthogonal queries serving different consumers.

## What stays in brink-lsp

The LSP crate becomes a thin shell. With `IdeSession`, the `Backend` no longer manages `ProjectDb` directly.

1. **`Backend` struct** — owns `Client`, `Arc<Mutex<IdeSession>>`, watch channels, `Notify`, generation counter.
2. **`LanguageServer` trait impl** — async handler dispatch. Each handler:
   - Extracts path from URI
   - Calls `IdeSession` query methods (which handle FileId resolution, data retrieval, and query delegation internally)
   - Converts brink-ide domain result to LSP type
3. **`analysis_loop`** — background analysis task with debouncing. Uses the `IdeSession` snapshot pattern: `lock -> update_source -> snapshot -> unlock -> analyze -> lock -> apply_analysis -> unlock`. Will use `brink-driver` for diagnostic collection (see migration plan).
4. **URI/path conversion** — `Url::to_file_path()`, `Url::from_file_path()`.
5. **LSP type conversion** — `convert.rs` keeps only the protocol-specific conversions (`SymbolKind -> lsp_types::SymbolKind`, `Severity -> DiagnosticSeverity`, `diagnostic_to_lsp`). `LineIndex` and `to_lsp_range`/`to_text_size` move to brink-ide.
6. **Multi-project diagnostic annotation** — LSP-specific UX (related information pointing to project roots).
7. **File watcher registration** — LSP protocol feature.
8. **Filesystem operations** — `load_file_from_disk`, `walk_and_load`, `chase_includes`.

### Conversion layer pattern

The LSP adapter converts brink-ide domain types to tower-lsp types. This is mechanical:

```rust
// In brink-lsp
fn hover_to_lsp(info: HoverInfo, idx: &LineIndex) -> Hover { .. }
fn completion_to_lsp(item: ide::CompletionItem) -> lsp_types::CompletionItem { .. }
fn location_to_lsp(loc: LocationResult, files: &ProjectFiles) -> Option<Location> { .. }
```

A future `brink-web` adapter would do the equivalent conversion to JsValue.

## Determinism

All brink-ide functions MUST produce deterministic output:

- Never iterate `HashMap` keys/values where order affects output. Use `BTreeMap` or sort.
- Workspace symbol search must sort results deterministically (existing code uses `HashSet` for deduplication — must be replaced with deterministic dedup).
- Completion items should be returned in a stable order (currently unordered from `symbols.values()`).

## Error handling

brink-ide functions return `Option<T>` when a query may not produce a result (e.g., no symbol at cursor position). They do not return `Result<T, E>` — there are no recoverable error conditions. If input data is missing or invalid, the function returns `None`.

## Testing strategy

brink-ide queries are directly testable without LSP infrastructure:

```rust
#[test]
fn goto_def_finds_knot() {
    let source = "=== target ===\nContent\n=== other ===\n-> target\n";
    let parse = brink_syntax::parse(source);
    let (hir, manifest, _) = brink_ir::hir::lower(parse.tree());
    let analysis = brink_analyzer::analyze(&[(FileId(0), &hir, &manifest)]);

    let offset = /* offset of "target" in "-> target" */;
    let result = brink_ide::goto_definition(&analysis, FileId(0), source, offset);
    assert!(result.is_some());
    // ... assert range matches the knot header
}
```

Existing tests in `backend.rs` (completion context detection, cursor scope, etc.) move directly to brink-ide as unit tests.

## Migration plan

The extraction is incremental — the LSP continues working throughout. Each step produces a working, committable state.

### Phase 1: Create brink-driver

1. Create `crates/internal/brink-driver/` with `Cargo.toml`
2. Move `discover()` logic from brink-db (include graph traversal, file discovery via callback)
3. Move `analyze()` orchestration (cross-file analysis that currently lives in both brink-db and the LSP's analysis_loop)
4. Move diagnostic collection + suppression + partitioning (currently duplicated between compiler `driver.rs` and LSP `analysis_loop`)
5. Move `resolve_include_path` from brink-db, fix to use string-based path resolution (`rfind('/')`) instead of `std::path::Path`
6. Move `compute_projects` (project computation from include relationships)
7. Simplify brink-db: remove `analyze()`, remove `discover()`, remove `compute_projects()`. brink-db becomes a pure per-file cache (parse, lower, store, query).
8. Rewrite `brink-compiler` to use brink-driver for orchestration
9. Rewrite LSP `analysis_loop` to use `brink-driver::collect_diagnostics`

### Phase 2: Create brink-ide

1. Create `crates/internal/brink-ide/` with `Cargo.toml`
2. Move `LineIndex` from brink-lsp `convert.rs` to brink-ide. Remove `tower_lsp::lsp_types` dependency from it.
3. Define domain types (`HoverInfo`, `CompletionItem`, `SemanticToken`, etc.)
4. Move `word_at_offset`, `word_range_at_offset`, `builtin_hover_text` (pure text utilities)
5. Move `find_call_context` (signature help helper)
6. Move `detect_completion_context`, `cursor_scope`, `is_visible_in_context` (completion helpers)
7. Move `find_def_at_offset` (definition lookup core logic)

### Phase 3: Migrate query functions one at a time

Each migration follows the same pattern:
1. Extract the core logic from the LSP handler into a brink-ide function
2. Update the LSP handler to call brink-ide + convert the result
3. Add unit tests in brink-ide
4. Commit

Order (by independence — simplest/most self-contained first):

1. **Formatting** — `format_document`, `format_region`, `sort_knots`, `sort_stitches`, `diff_to_edits`
2. **Document structure** — `document_symbols`, `folding_ranges`
3. **Semantic tokens** — `semantic_tokens`, `semantic_tokens_range`, `delta_encode` (move entire `semantic_tokens.rs` logic)
4. **Completion** — `completions` (already has helpers moved in Phase 2)
5. **Hover** — `hover`
6. **Signature help** — `signature_help`
7. **Inlay hints** — `inlay_hints`
8. **Navigation** — `goto_definition`, `find_references`
9. **Rename** — `prepare_rename`, `rename`
10. **Code actions** — `code_actions`, `resolve_code_action`
11. **Workspace symbols** — `workspace_symbols`

### Phase 4: Build brink-web

Out of scope for this spec. Covered in a future `brink-web-spec.md`.

## Crate metadata

```toml
[package]
name = "brink-ide"
version = "0.1.0"
edition = "2021"

[dependencies]
brink-db.workspace = true
brink-syntax.workspace = true
brink-ir.workspace = true
brink-analyzer.workspace = true
brink-fmt.workspace = true
bitflags.workspace = true
rowan.workspace = true

[dev-dependencies]
brink-syntax.workspace = true
brink-ir.workspace = true
brink-analyzer.workspace = true
```

brink-ide depends on brink-db for `IdeSession` (which owns a `ProjectDb`). The pure query functions do not use brink-db -- they take `&AnalysisResult`, `&HirFile`, etc. as arguments. brink-db is wasm-compatible, so this dependency does not affect the wasm target.

`bitflags` is used workspace-wide (already a dependency of brink-ir and brink-format), so brink-ide uses it for `TokenModifiers`.

## HIR change: ChoiceSet.depth

`line_contexts()` needs the weave depth of each choice set. The HIR's `ChoiceSet` struct gains a `depth` field populated during `fold_weave`:

```rust
// In brink-ir::hir::types
pub struct ChoiceSet {
    pub choices: Vec<Choice>,
    pub continuation: Block,
    pub context: ChoiceSetContext,
    /// Weave nesting depth. Set during `fold_weave` from the base_depth
    /// parameter. 1 = top-level choices, 2 = choices nested inside a
    /// choice body, etc.
    pub depth: u32,  // NEW
}
```

The depth is known during folding -- it is the `base_depth` parameter passed to `fold_weave_at_depth`. Storing it explicitly avoids requiring consumers to reconstruct depth by walking the HIR tree and counting nesting levels.

This is the canonical source of weave depth. `line_contexts()` reads `ChoiceSet.depth` rather than computing depth from tree structure.

## Replacing classifyLine

brink-studio's TypeScript editor currently classifies lines with a regex-based `classifyLine` function in `packages/brink-studio/src/editor/element-type.ts`. This function produces `LineInfo { type: ElementType, depth, sticky, standalone }` by pattern-matching against line text. It has known limitations:

- **Depth is wrong for choice body text.** `classifyLine` only counts sigils on the current line. Narrative text inside a choice body gets depth 0, but its structural depth is the enclosing choice's depth.
- **No structural awareness.** The regex cannot distinguish narrative in a knot body from narrative in a choice body from narrative after a gather. All are `ElementType.NarrativeText` with depth 0.
- **Block comment state is lost.** A line inside `/* ... */` may match as narrative or another element type.
- **Bracket matching is naive.** The bracket highlighting in `screenplay.ts` does single-character `[`/`]` scanning without understanding ink's bracket semantics.

`line_contexts()` replaces all of this with authoritative data from the HIR and parse tree. The migration:

1. **`elementTypeField` StateField** -- currently calls `classifyLine` per line on every doc change. Replaced by calling `EditorSession.line_contexts()` via wasm, which returns `Vec<LineContext>` with both `LineElement` (replacing `ElementType`) and `WeavePosition` (replacing the regex-computed depth).

2. **`screenplay.ts` line decorations** -- reads `LineElement` for CSS class, `WeavePosition.depth` for indentation. The sigil replacement widget reads `WeavePosition.element` to determine sigil type. `WeaveElement::ChoiceLine { sticky }` replaces the `sticky` field on `LineInfo`.

3. **`transitions.ts` state machine** -- reads `LineElement` for element matching, `WeavePosition.depth` for depth predicates, `WeaveElement` for context-dependent transitions (e.g., Enter after `ChoiceBody` vs Enter after `GatherContinuation`).

4. **`statusbar.ts` element label** -- reads `LineElement` for the label, `WeavePosition.depth` for the depth indicator, `WeaveElement::ChoiceLine { sticky }` for the sticky marker.

5. **`screenplay.ts` bracket highlighting** -- replaced by `LineContext.brackets`, which contains the authoritative byte ranges from the parse tree.

6. **Inline element styling** -- `LineContext.tags`, `LineContext.inline_exprs`, and `LineContext.block_comment` provide ranges for inline decorations that the regex classifier could not produce.

The `classifyLine` function and `ElementType` enum in TypeScript are deleted. `LineElement` (from Rust, serialized via wasm) is the single source of truth.

## Relationship to brink-driver

brink-driver handles *orchestration* (running the pipeline, collecting diagnostics, managing file discovery). brink-ide handles *queries* (answering questions about analyzed code). They are complementary:

- brink-driver produces the `AnalysisResult` that brink-ide queries
- brink-driver manages the diagnostic pipeline; brink-ide never touches diagnostics
- A shell (LSP, web) uses brink-driver to keep analysis up to date, and brink-ide to answer user queries against the latest analysis

Neither crate depends on the other.
