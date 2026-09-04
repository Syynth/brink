//! Bytecode backend: LIR → `StoryData`.

mod chunk;
mod container;
mod content;
mod debug_info;
mod expr;

pub use chunk::{ContainerChunk, NameRef, Relocation, UNRESOLVED_NAME_ID};
pub use debug_info::EmitOptions;

use std::collections::HashMap;

use brink_format::{
    AddressDef, AddressPath, ContainerDef, DefinitionId, ExternalFnDef, GlobalVarDef, LineContent,
    LineEntry, ListDef, ListItemDef, ListValue, MapKey, NameId, Opcode, OrderedMap, ScopeLineTable,
    ShapeId, StoryData, StructShapeDef, Value,
};
use brink_ir::lir;

/// A defect in the LIR fed to codegen — an invariant that a well-formed
/// `Program` is guaranteed to satisfy by earlier, non-suppressible compiler
/// stages, which codegen has no independent way to verify structurally
/// beyond this checkpoint. See #586: with #577's `Nop` degradation removed,
/// `container.rs`'s `LogicBreak`/`LogicContinue` handling had zero
/// codegen-level guard against a `loop_stack` that's empty — a future or
/// refactored LIR producer that ever emitted one outside a loop would
/// silently corrupt bytecode via an unpatched `Jump(0)` that looks
/// well-formed, rather than fail. This is the hard error that replaces
/// that silent corruption; today it can only fire on hand-assembled LIR
/// that bypasses `brink-ir::lir::lower` (which rejects this case at E057,
/// non-suppressibly, before a `Program` is ever produced).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    message: String,
}

impl CodegenError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Apply [`collapse_whitespace`] to a template part's literal text, recursing
/// into `Span { children }` so nested literals get the same treatment as
/// top-level ones (`<b>a  b</b>` must collapse the same as `a  b`).
/// `Slot`/`Select` carry no literal text of their own and pass through
/// unchanged.
fn collapse_whitespace_in_part(part: brink_format::LinePart) -> brink_format::LinePart {
    match part {
        brink_format::LinePart::Literal(s) => {
            brink_format::LinePart::Literal(collapse_whitespace(&s))
        }
        brink_format::LinePart::Span {
            name,
            attrs,
            children,
        } => brink_format::LinePart::Span {
            name,
            attrs,
            children: children
                .into_iter()
                .map(collapse_whitespace_in_part)
                .collect(),
        },
        other @ (brink_format::LinePart::Slot(_) | brink_format::LinePart::Select { .. }) => other,
    }
}

/// Collapse runs of consecutive spaces/tabs within `s` to a single space.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c == ' ' || c == '\t' {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            prev_ws = false;
            out.push(c);
        }
    }
    out
}

/// Compile a resolved LIR `Program` into `StoryData` for the runtime.
///
/// Returns `Err(CodegenError)` only for a defect in the LIR itself — see
/// [`CodegenError`]. A well-formed `Program` (the only kind
/// `brink-ir::lir::lower` ever hands back) always succeeds.
///
/// Equivalent to [`emit_with_options`] with [`EmitOptions::default()`] —
/// `emit_debug_info: false`, reproducing every byte this function has ever
/// emitted (the D6 byte-identical guarantee, `docs/debugger-spec.md` §1.2).
pub fn emit(program: &lir::Program) -> Result<StoryData, CodegenError> {
    emit_with_options(program, EmitOptions::default())
}

/// [`emit`] with explicit [`EmitOptions`] — the D6 (`docs/debugger-spec.md`
/// §2) entry point. `options.emit_debug_info` gates the `DebugInfo` section
/// (`SectionKind::DebugInfo`, tag `0x11`): `false` takes the exact same code
/// path `emit` always has, byte-for-byte; `true` additionally records
/// `(bytecode_offset, source_range)` pairs during the same container walk
/// and attaches them as `StoryData::debug_info`.
pub fn emit_with_options(
    program: &lir::Program,
    options: EmitOptions<'_>,
) -> Result<StoryData, CodegenError> {
    let mut state = EmitState {
        chunks: Vec::new(),
        addresses: Vec::new(),
        definition_id_first_seen: HashMap::new(),
        address_paths: Vec::new(),
        scope_line_tables: HashMap::new(),
        line_variant_groups: Vec::new(),
        list_literals: Vec::new(),
        literal_pool: Vec::new(),
        name_table: program.name_table.clone(),
        name_index: HashMap::new(),
        errors: Vec::new(),
        debug: options
            .emit_debug_info
            .then(debug_info::DebugCollector::new),
    };

    // Build the name index from the existing name table for dedup.
    for (i, name) in state.name_table.iter().enumerate() {
        #[expect(clippy::cast_possible_truncation)]
        state.name_index.insert(name.clone(), NameId(i as u16));
    }

    // Walk the container tree depth-first.
    // Root is always a scope — its scope_id is its own id; its author scope
    // path is empty.
    walk_container(&program.root, "", "", program.root.id, &mut state);

    // D6 (`docs/debugger-spec.md` §2, issue #3184): finished here, before
    // the error check below, not folded into the final `Ok(StoryData {..})`
    // the way it originally was — `DebugCollector::finish` can itself push
    // a `CodegenError` (an interned `FileId` unresolvable in
    // `program.file_paths`, #3219 review), and `state.errors` is only ever
    // inspected at the single early-return point right after this. Folding
    // it in later would let that error go unchecked, since nothing after
    // this point looks at `state.errors` again.
    let debug_info = state
        .debug
        .take()
        .map(|d| d.finish(program, options.debug_sources, &mut state.errors));

    if let Some(first) = core::mem::take(&mut state.errors).into_iter().next() {
        return Err(first);
    }

    // Build globals, lists, externals.
    let variables = build_globals(&program.globals, &mut state);
    let list_defs = build_list_defs(&program.lists);
    let list_items = build_list_items(&program.list_items);
    let externals = build_externals(&program.externals);
    let struct_shapes = build_struct_shapes(&program.struct_shapes);

    // Convert scope line tables to a sorted Vec<ScopeLineTable>.
    // #3273: deterministic group order, mirroring the line-table sort.
    let mut line_variant_groups = state.line_variant_groups;
    line_variant_groups.sort_by_key(|g| (g.scope_id.to_raw(), g.base));

    let mut line_tables: Vec<ScopeLineTable> = state
        .scope_line_tables
        .into_iter()
        .map(|(scope_id, lines)| ScopeLineTable { scope_id, lines })
        .collect();
    line_tables.sort_by_key(|lt| lt.scope_id.to_raw());

    // Link phase (FG-4b): resolve each chunk's symbolic name-reference
    // relocations against the now-fully-assembled name table and patch the
    // placeholder operands in place. The name index is complete — every
    // `PushString` symbol was interned as its relocation was recorded (see
    // `ContainerEmitter::emit_push_string`) — so a miss is a real defect,
    // surfaced by `ContainerChunk::link` rather than silently dropped.
    let name_index = &state.name_index;
    let containers = state
        .chunks
        .into_iter()
        .map(|c| c.link(|s| name_index.get(s).copied()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(StoryData {
        containers,
        line_tables,
        variables,
        list_defs,
        list_items,
        externals,
        addresses: state.addresses,
        address_paths: state.address_paths,
        name_table: state.name_table,
        list_literals: state.list_literals,
        literal_pool: state.literal_pool,
        struct_shapes,
        // M-2b: the compiler-computed private-definition set rides straight
        // through — the LIR already sorted it deterministically.
        private_defs: program.private_defs.clone(),
        alias_table: program.aliases.clone(),
        // T2-3 `EffectRows`: codegen has no analyzer access, so it emits an
        // empty table here. The `story_data` db query populates the real rows
        // from `effects_query` after this `emit` (the one canonical codegen
        // site) — see `docs/effects-spec.md` §11.
        effect_rows: Vec::new(),
        // FS-3 `FrameShapes` (`docs/flow-suspension-spec.md` §4/§11): the E052
        // `await` lowering fence stands, so no `await` reaches codegen and no
        // frame shapes are synthesized. Emitted empty; first population rides
        // the continuation-splitting codegen when the fence drops (FS-3r).
        frame_shapes: Vec::new(),
        // D6 `DebugInfo` (`docs/debugger-spec.md` §2, tag 0x11): `None`
        // unless `options.emit_debug_info` was set — the section is
        // omitted entirely from `.inkb` in that case, so a release compile
        // stays byte-identical (§1.2 ship policy). Computed above, before
        // the error check — see that comment for why.
        debug_info,
        line_variant_groups,
        source_checksum: 0,
    })
}

/// TM-4c: `lir::StructShapeDef` → `brink_format::StructShapeDef`, id order
/// preserved (`lir::lower::structs::struct_shape_defs` already hands back a
/// `Vec` ordered by `ShapeId`, so this is a 1:1 field mapping, not a sort).
fn build_struct_shapes(shapes: &[lir::StructShapeDef]) -> Vec<StructShapeDef> {
    shapes
        .iter()
        .map(|s| StructShapeDef {
            id: ShapeId(s.id),
            name: s.name,
            fields: s.fields.clone(),
        })
        .collect()
}

// ─── Emission state ─────────────────────────────────────────────────

struct EmitState {
    /// Per-container codegen chunks (FG-4b): each holds a `ContainerDef`
    /// whose bytecode carries [`UNRESOLVED_NAME_ID`] placeholders at every
    /// name-reference site, plus the symbolic relocation table the link
    /// phase in [`emit`] resolves. Pushed in container-walk order.
    chunks: Vec<ContainerChunk>,
    addresses: Vec<AddressDef>,
    /// #1673 codegen-boundary uniqueness guard: every emitted container's
    /// `DefinitionId` mapped to the (inklecate-style) path it was first
    /// seen at. A well-formed `Program` assigns every container a distinct
    /// id; nothing upstream of codegen independently re-verified that
    /// before this guard existed, and the #1504 collision reached the
    /// runtime silently — the linker's address map is last-write-wins, so
    /// a duplicate id made a player-picked choice run the *other*
    /// container's body. See [`walk_container`]'s check against this map.
    definition_id_first_seen: HashMap<DefinitionId, String>,
    /// Qualified-path → target table (scope containers + author labels).
    address_paths: Vec<AddressPath>,
    /// Scope-shared line tables: `scope_id` → accumulated line entries.
    scope_line_tables: HashMap<DefinitionId, Vec<LineEntry>>,
    /// #3273: variant-group records accumulated as `EmitLineVariants`
    /// statements register their line-table runs. Sorted before assembly
    /// for deterministic output.
    line_variant_groups: Vec<brink_format::LineVariantGroup>,
    list_literals: Vec<ListValue>,
    /// The T1b `LiteralPool` (`docs/format-v4-rfc.md` §2), built up as
    /// `PushLiteral` sites are emitted. Content-hash-dedup isn't needed for
    /// correctness (structural equality dedup below is exact); a linear
    /// scan is fine at game-corpus literal-pool sizes.
    literal_pool: Vec<Value>,
    name_table: Vec<String>,
    name_index: HashMap<String, NameId>,
    /// Codegen-level defects found during the tree walk (see
    /// [`CodegenError`]) — accumulated the same way `brink-ir`'s LIR
    /// lowering accumulates diagnostics (`ctx.diagnostics.push`), checked
    /// once after the whole walk finishes rather than threading a
    /// `Result` through every recursive emitter call. Bounded by the size
    /// of the `Program` being walked, same as every other `Vec` here.
    errors: Vec<CodegenError>,
    /// D6 (`docs/debugger-spec.md` §2, issue #3184) debug-info recording —
    /// `None` unless `EmitOptions::emit_debug_info` was set. Kept as an
    /// `Option` rather than an always-present, conditionally-populated
    /// collector so the container walk pays nothing (no branch, no
    /// allocation) on the default `emit()` path — the byte-identical
    /// guarantee this section's whole design depends on.
    debug: Option<debug_info::DebugCollector>,
}

/// Intern a string into a story name table, deduping against entries already
/// present. Shared by both codegen phases: the container walk
/// ([`ContainerEmitter::intern_string`]) and the post-walk build phase
/// ([`const_to_value`]), which hold the name table/index by different paths
/// but need identical dedup semantics.
fn intern_into(
    name_table: &mut Vec<String>,
    name_index: &mut HashMap<String, NameId>,
    s: &str,
) -> NameId {
    if let Some(&id) = name_index.get(s) {
        return id;
    }
    #[expect(clippy::cast_possible_truncation)]
    let id = NameId(name_table.len() as u16);
    name_table.push(s.to_string());
    name_index.insert(s.to_string(), id);
    id
}

// ─── Container emitter ──────────────────────────────────────────────

struct ContainerEmitter<'a> {
    bytecode: Vec<u8>,
    scope_line_table: &'a mut Vec<LineEntry>,
    /// The scope whose line table this emitter appends to — the
    /// `scope_id` a variant-group record (#3273) is keyed by.
    scope_id: DefinitionId,
    /// #3273: shared with [`EmitState::line_variant_groups`].
    line_variant_groups: &'a mut Vec<brink_format::LineVariantGroup>,
    list_literals: &'a mut Vec<ListValue>,
    literal_pool: &'a mut Vec<Value>,
    state_name_table: &'a mut Vec<String>,
    state_name_index: &'a mut HashMap<String, NameId>,
    in_conditional_branch: bool,
    /// Issue #3508: set while a choice's DISPLAY text is being emitted. Line
    /// entries added then keep their whitespace runs verbatim — ink presents
    /// a choice's text as the evaluated string trimmed at the ends only
    /// (`a  0` stays `a  0`), while an output line is collapsed on render
    /// (`CleanOutputWhitespace`). brink collapses at compile time instead
    /// ([`collapse_whitespace`] in [`Self::add_line_with_hash`]), which is
    /// observably the same for output lines and was wrong for choice text.
    in_choice_display: bool,
    /// Stack of open T1b `LogicWhile` loops (innermost last) — targets for
    /// `break`/`continue` jump patching. Empty outside any loop.
    loop_stack: Vec<LoopCtx>,
    /// Shared with every other `ContainerEmitter` created during the same
    /// `emit()` call (see `EmitState::errors`).
    errors: &'a mut Vec<CodegenError>,
    /// FG-4b symbolic name-reference patch sites into this container's
    /// `bytecode`. Populated by [`Self::emit_push_string`]; drained into the
    /// container's [`ContainerChunk`] by `walk_container`.
    relocations: Vec<Relocation>,
    /// D6 (`docs/debugger-spec.md` §2.2, issue #3184 review): `Some` for the
    /// whole life of this emitter exactly when `state.debug` is `Some` —
    /// `walk_container` seeds it (with the params-prologue entry, if any)
    /// right after construction and takes it back out when this container's
    /// bytecode is done. Recording happens at a single point,
    /// [`Self::record_debug_entry`], called from every body-statement walk —
    /// top-level *and* nested (`Conditional`/`Sequence`/`LogicWhile` branch
    /// bodies) — so a statement inside a branch gets an entry the same way a
    /// top-level one does (#3219 review: nested statements previously got
    /// none). `None` on the default `emit()` path costs nothing beyond the
    /// tag check itself.
    debug_entries: Option<Vec<debug_info::RawDebugEntry>>,
}

/// Jump-patch bookkeeping for one open `LogicWhile` (innermost = top of
/// `ContainerEmitter::loop_stack`).
struct LoopCtx {
    /// `break` sites — patched to land just after the whole loop.
    break_patches: Vec<usize>,
    /// `continue` sites — patched to land at the start of `post` (the
    /// backward jump to `condition` for a plain `while`, since `post` is
    /// empty then).
    continue_patches: Vec<usize>,
}

impl<'a> ContainerEmitter<'a> {
    fn new(state: &'a mut EmitState, scope_id: DefinitionId) -> Self {
        let scope_line_table = state.scope_line_tables.entry(scope_id).or_default();
        Self {
            bytecode: Vec::new(),
            scope_line_table,
            scope_id,
            line_variant_groups: &mut state.line_variant_groups,
            list_literals: &mut state.list_literals,
            literal_pool: &mut state.literal_pool,
            state_name_table: &mut state.name_table,
            state_name_index: &mut state.name_index,
            in_conditional_branch: false,
            in_choice_display: false,
            loop_stack: Vec::new(),
            errors: &mut state.errors,
            relocations: Vec::new(),
            debug_entries: None,
        }
    }

    /// Emit a `PushString` whose `NameId` operand is left *symbolic*
    /// (FG-4b): the string is interned into the story name table now — so
    /// the table's contents and ordering are byte-for-byte identical to
    /// eager resolution — but the bytecode carries an [`UNRESOLVED_NAME_ID`]
    /// placeholder and a [`Relocation`] recording the symbolic reference.
    /// The link phase in [`emit`] resolves the symbol against the assembled
    /// name table and patches the operand (see [`chunk`]).
    fn emit_push_string(&mut self, text: &str) {
        // Intern eagerly to fix this string's position in the name table:
        // the link phase looks the same string up, so the patched operand
        // equals what pre-chunk codegen wrote inline. The assigned id is
        // deliberately not baked into the chunk — the chunk stays symbolic.
        self.intern_string(text);
        self.emit(Opcode::PushString(UNRESOLVED_NAME_ID));
        #[expect(clippy::cast_possible_truncation)]
        let offset = (self.bytecode.len() - 2) as u32;
        self.relocations.push(Relocation {
            offset,
            name: NameRef::Symbol(text.to_string()),
        });
    }

    #[expect(clippy::needless_pass_by_value)]
    fn emit(&mut self, op: Opcode) {
        op.encode(&mut self.bytecode);
    }

    /// `source_location` is an explicit, required parameter — not a
    /// convenience default of `None` — precisely because that default was
    /// the issue #3181 bug: every caller must say what it knows (a real
    /// location threaded from `hir::Content::ptr`, or `None` with a reason
    /// at the call site) rather than one caller's ignorance silently
    /// becoming every caller's answer.
    fn add_line(
        &mut self,
        text: &str,
        source_location: Option<brink_format::SourceLocation>,
    ) -> u16 {
        self.add_line_with_hash(
            text,
            brink_format::content_hash(text),
            Vec::new(),
            source_location,
        )
    }

    #[expect(clippy::cast_possible_truncation)]
    fn add_line_with_hash(
        &mut self,
        text: &str,
        source_hash: u64,
        slot_info: Vec<brink_format::SlotInfo>,
        source_location: Option<brink_format::SourceLocation>,
    ) -> u16 {
        let idx = self.scope_line_table.len() as u16;
        let text = if self.in_choice_display {
            text.to_owned()
        } else {
            collapse_whitespace(text)
        };
        let content = LineContent::Plain(text);
        let flags = brink_format::LineFlags::from_content(&content);
        self.scope_line_table.push(LineEntry {
            content,
            flags,
            source_hash,
            audio_ref: None,
            slot_info,
            source_location,
        });
        idx
    }

    #[expect(clippy::cast_possible_truncation)]
    fn add_template_line(
        &mut self,
        parts: brink_format::LineTemplate,
        source_hash: u64,
        slot_info: Vec<brink_format::SlotInfo>,
        source_location: Option<brink_format::SourceLocation>,
    ) -> u16 {
        let idx = self.scope_line_table.len() as u16;
        let parts = if self.in_choice_display {
            parts
        } else {
            parts.into_iter().map(collapse_whitespace_in_part).collect()
        };
        let content = LineContent::Template(parts);
        let flags = brink_format::LineFlags::from_content(&content);
        self.scope_line_table.push(LineEntry {
            content,
            flags,
            source_hash,
            audio_ref: None,
            slot_info,
            source_location,
        });
        idx
    }

    fn intern_string(&mut self, s: &str) -> NameId {
        if let Some(&id) = self.state_name_index.get(s) {
            return id;
        }
        #[expect(clippy::cast_possible_truncation)]
        let id = NameId(self.state_name_table.len() as u16);
        self.state_name_table.push(s.to_string());
        self.state_name_index.insert(s.to_string(), id);
        id
    }

    /// Emit a jump-like instruction with a placeholder offset.
    /// Returns the byte position of the i32 offset field for later patching.
    #[expect(clippy::needless_pass_by_value)]
    fn emit_jump_placeholder(&mut self, op: Opcode) -> usize {
        op.encode(&mut self.bytecode);
        // The i32 offset occupies the last 4 bytes of the encoded instruction.
        self.bytecode.len() - 4
    }

    /// Patch a previously emitted jump offset to point to the current position.
    /// The offset is relative: bytes from end of the jump instruction to current pos.
    fn patch_jump(&mut self, offset_pos: usize) {
        let target = self.bytecode.len();
        // The jump instruction ends right after the i32 field (offset_pos + 4).
        let instruction_end = offset_pos + 4;
        #[expect(clippy::cast_possible_wrap)]
        #[expect(clippy::cast_possible_truncation)]
        let relative = (target - instruction_end) as i32;
        let bytes = relative.to_le_bytes();
        self.bytecode[offset_pos..offset_pos + 4].copy_from_slice(&bytes);
    }

    /// D6 (`docs/debugger-spec.md` §2.2, issue #3184 review — nested
    /// statements previously got no debug entries): record one
    /// [`debug_info::RawDebugEntry`] for `stmt` at this container's current
    /// bytecode length, a no-op when `self.debug_entries` is `None` (the
    /// default `emit()` path). Called from [`Self::emit_body`] for *every*
    /// statement it walks — top-level and nested (`Conditional`/`Sequence`/
    /// `LogicWhile` branch bodies all route back through `emit_body`) — so
    /// entries come out already sorted ascending by construction: they are
    /// pushed in emission order, and bytecode length only grows.
    /// `prologue_end` is always `false` from this call site; only
    /// `walk_container`'s dedicated top-level pass (`emit_body_top_level`)
    /// ever sets it `true`, since the prologue-end marker (§2.4) is a
    /// per-container concept, not a per-branch one.
    fn record_debug_entry(&mut self, stmt: &lir::Stmt, prologue_end: bool) {
        if self.debug_entries.is_none() {
            return;
        }
        #[expect(clippy::cast_possible_truncation)]
        let offset = self.bytecode.len() as u32;
        if let Some(entries) = self.debug_entries.as_mut() {
            entries.push(debug_info::RawDebugEntry {
                offset,
                provenance: stmt.provenance,
                prologue_end,
            });
        }
    }
}

// ─── Container tree walk ────────────────────────────────────────────

/// Returns `true` if the container kind is a lexical scope (root, knot, stitch).
fn is_scope_kind(kind: lir::ContainerKind) -> bool {
    matches!(
        kind,
        lir::ContainerKind::Root | lir::ContainerKind::Knot | lir::ContainerKind::Stitch
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "single linear emit sequence; splitting would obscure the order"
)]
fn walk_container(
    container: &lir::Container,
    path: &str,
    scope_author_path: &str,
    scope_id: DefinitionId,
    state: &mut EmitState,
) {
    // #1673 codegen-boundary uniqueness guard: two containers must never
    // share a `DefinitionId`. This should be structurally impossible — every
    // id is a content-pure hash minted once per container during LIR
    // lowering — but the #1504 collision proved it *can* happen (unqualified
    // anonymous scope paths across files) and, when it does, the failure is
    // silent all the way to the player: the linker's address map (and this
    // walk's own `state.chunks`/`state.addresses`) is last-write-wins, so
    // the second container to reach this point quietly overwrites the
    // first's entry instead of erroring. Checked once per container walked,
    // O(1) amortized against the `HashMap` insert every other per-container
    // table here already pays — cheap by design (see #1673).
    if let Some(prior_path) = state
        .definition_id_first_seen
        .insert(container.id, path.to_string())
    {
        state.errors.push(CodegenError::new(format!(
            "duplicate DefinitionId {} assigned to two different containers, at paths {prior_path:?} and {path:?} — every container must have a unique DefinitionId (#1673); this collision would otherwise reach the runtime silently and produce wrong player-visible output, as it did in #1504",
            container.id
        )));
    }

    // The author-facing path of the nearest enclosing scope. For a scope
    // container this is its own `path` (scope paths never get inklecate's
    // implicit `0.` stitch prefix); non-scope containers inherit it. Used to
    // qualify author labels (matching the analyzer's `qualify_label`),
    // independent of the inklecate `path` used for `path_hash`.
    let this_scope_path = if is_scope_kind(container.kind) {
        path
    } else {
        scope_author_path
    };

    // D6 (`docs/debugger-spec.md` §2.2/§2.4): read before `state` is
    // (re)borrowed by `ContainerEmitter::new` below — `emitter` holds a
    // mutable borrow of `*state` for its whole lifetime, so `state.debug`
    // must be read here, not after. `raw_entries` stays empty (no
    // allocation) on the default `emit()` path.
    let debug_enabled = state.debug.is_some();
    let mut raw_entries: Vec<debug_info::RawDebugEntry> = Vec::new();
    if debug_enabled && !container.params.is_empty() {
        // The leading parameter-binding `DeclareTemp`s below are real
        // prologue bytecode with no `lir::Stmt` of their own (they're
        // emitted as bare opcodes, never through `emit_stmt`), so that
        // offset-0 span needs its own entry, recorded here before they're
        // emitted, using the *container's* own provenance (there is no
        // statement to point at). Never the prologue-end landing point
        // itself (`prologue_end: false`) — `prologue_end_index` below picks
        // that.
        raw_entries.push(debug_info::RawDebugEntry {
            offset: 0,
            provenance: container.provenance,
            prologue_end: false,
        });
    }

    // §2.4: which statement in `container.body` (by position) is the
    // landing point past this container's prologue bytecode — the leading
    // param `DeclareTemp`s above, plus, for a choice-target body, its
    // leading `ChoiceOutput` statement, which is *also* prologue bytecode:
    // a breakpoint on a choice target must land past the choice's own
    // output being emitted, not on it (#3219 review — the naive `i == 0`
    // this replaced flagged the `ChoiceOutput` statement itself whenever it
    // was `body[0]`, which it always is when present). `None` when there is
    // no statement to flag: an empty body, or a choice-target body
    // containing only the `ChoiceOutput` — `walk_container` pushes its own
    // synthetic coverage entry for that case below, once the real offset
    // past all prologue bytecode is known.
    let leading_choice_output = matches!(
        container.body.first().map(|stmt| &stmt.kind),
        Some(lir::StmtKind::ChoiceOutput { .. })
    );
    let prologue_end_index = if leading_choice_output {
        (container.body.len() > 1).then_some(1)
    } else {
        (!container.body.is_empty()).then_some(0)
    };

    // D7 (`docs/debugger-spec.md` §3, issue #3185): this container's own
    // `LocalsTable` rows — one per declared parameter (bound by the bare
    // `DeclareTemp` opcodes emitted below, no `lir::Stmt`/source range of
    // their own) plus one per top-level `~ temp` declaration in this
    // container's own body (a nested child container's `DeclareTemp`s are
    // recorded when *it* is walked, into *its own* table — §2.2's
    // container-lockstep framing, not this container's). `raw_locals` stays
    // empty (no allocation) on the default `emit()` path.
    let mut raw_locals: Vec<debug_info::RawLocal> = Vec::new();
    if debug_enabled {
        for param in &container.params {
            raw_locals.push(debug_info::RawLocal {
                slot: param.slot,
                name: param.name,
                declaring_range: None,
                synthetic: false,
            });
        }
        for stmt in &container.body {
            if let lir::StmtKind::DeclareTemp {
                slot,
                name,
                synthetic,
                ..
            } = &stmt.kind
            {
                raw_locals.push(debug_info::RawLocal {
                    slot: *slot,
                    name: *name,
                    declaring_range: Some(stmt.provenance),
                    synthetic: *synthetic,
                });
            }
        }
    }

    // Emit this container's bytecode.
    let mut emitter = ContainerEmitter::new(state, scope_id);
    if debug_enabled {
        emitter.debug_entries = Some(raw_entries);
    }

    // Branch containers (conditional or sequence) suppress `Done` after
    // ChoiceSets. Choices inside branches form part of a larger logical
    // ChoiceSet in the parent — the runtime auto-presents pending choices
    // on frame/container exhaustion (no explicit Done needed).
    if container.kind == lir::ContainerKind::ConditionalBranch
        || container.kind == lir::ContainerKind::SequenceBranch
    {
        emitter.in_conditional_branch = true;
    }

    // Emit DeclareTemp for each parameter (pops args from eval stack into
    // temp slots). Reverse order: caller pushes first arg first, so last
    // arg is on top of the stack and gets popped first.
    for param in container.params.iter().rev() {
        emitter.emit(Opcode::DeclareTemp(param.slot));
    }

    // Unconditional: `emit_body_top_level` (and everything it recurses
    // into) only *records* when `emitter.debug_entries` is `Some` — see
    // `ContainerEmitter::record_debug_entry` — so this is exactly
    // `emit_body`'s old plain behavior byte-for-byte on the default
    // (`debug_enabled == false`) path, with no separate branch needed here.
    emitter.emit_body_top_level(&container.body, prologue_end_index);

    let raw_entries = if debug_enabled {
        let mut entries = emitter.debug_entries.take().unwrap_or_default();
        if prologue_end_index.is_none() {
            // Coverage guarantee (§2.4): even when no statement was flagged
            // above — an empty body, or a choice-target body containing
            // only the `ChoiceOutput` — this container still needs an entry
            // covering its post-prologue offset, so the floor-lookup binary
            // search never runs off the end of the table.
            // `emitter.bytecode.len()` here is exactly that offset:
            // `emit_body_top_level` has already finished, so it's past the
            // param `DeclareTemp`s and, if present, the `ChoiceOutput`'s own
            // bytecode.
            #[expect(clippy::cast_possible_truncation)]
            entries.push(debug_info::RawDebugEntry {
                offset: emitter.bytecode.len() as u32,
                provenance: container.provenance,
                prologue_end: true,
            });
        }
        entries
    } else {
        Vec::new()
    };

    let path_hash: i32 = path.chars().map(|c| c as i32).sum();

    // Scope-owning containers get a human-readable name for the intl pipeline.
    let name = if is_scope_kind(container.kind) {
        Some(emitter.intern_string(path))
    } else {
        None
    };

    // Qualified author path → this container, for find_address. Scope
    // containers are addressable by their scope path; author-labeled
    // gathers/choices by `{enclosing_scope}.{label}`. Interned now while the
    // emitter is alive; the entry is pushed after the emitter is consumed.
    let address_path_id: Option<NameId> = if is_scope_kind(container.kind) {
        name
    } else if container.labeled {
        let label = container.name.as_deref().unwrap_or("_anon");
        let qualified = if this_scope_path.is_empty() {
            label.to_string()
        } else {
            format!("{this_scope_path}.{label}")
        };
        Some(emitter.intern_string(&qualified))
    } else {
        None
    };

    // Take the symbolic relocation table before the emitter's bytecode is
    // moved into the def; the chunk carries both (FG-4b).
    let relocations = core::mem::take(&mut emitter.relocations);
    let def = ContainerDef {
        id: container.id,
        scope_id,
        name,
        bytecode: emitter.bytecode,
        counting_flags: container.counting_flags,
        path_hash,
        // Declared-parameter count for arity-checking a host-directed entry
        // (`choose_path_string_with_args`) / `call_function`. Saturates — a
        // knot with >255 params is absurd and never legitimately occurs.
        param_count: u8::try_from(container.params.len()).unwrap_or(u8::MAX),
        // Per-param name/mode metadata (T1c, #700): carried so the runtime can
        // validate a rehydrated function value against the current signature.
        // The LIR param `NameId`s are valid in the story name table (it is
        // cloned from `program.name_table` — see `emit`), so they are used
        // as-is.
        params: container
            .params
            .iter()
            .map(|p| brink_format::ParamMeta {
                name: p.name,
                is_ref: p.is_ref,
            })
            .collect(),
        local: container.local,
    };
    state.chunks.push(ContainerChunk { def, relocations });

    // D6: pushed in the same order as `state.chunks` above — lockstep with
    // the eventual `StoryData::containers`, matching §2.2's `container_idx`
    // contract. A no-op (`state.debug` is `None`) on the default path.
    if let Some(debug) = state.debug.as_mut() {
        debug.push_container(raw_entries, raw_locals);
    }

    // Primary address: every container is addressable by its own id.
    state.addresses.push(AddressDef {
        id: container.id,
        container_id: container.id,
        byte_offset: 0,
    });

    // Record the qualified-path → container mapping for find_address.
    if let Some(path_id) = address_path_id {
        state.address_paths.push(AddressPath {
            path: path_id,
            target: container.id,
        });
    }

    // Recurse into children.
    for child in &container.children {
        let child_name = child.name.as_deref().unwrap_or("_anon");

        // Compute the path segment for this child, applying inklecate-compatible
        // naming rules so that path_hash values match for shuffle RNG seeding.
        // Inklecate-compatible path naming rules so path_hash values match
        // for shuffle RNG seeding. Only non-function knots get the implicit
        // stitch ".0" prefix (functions store children directly).
        let needs_stitch_prefix = container.kind == lir::ContainerKind::Knot
            && !container.is_function
            && child.kind != lir::ContainerKind::Stitch;

        let segment = if needs_stitch_prefix && child.kind == lir::ContainerKind::Sequence {
            // Rule 1+2: stitch prefix + rename "s-N" → "N"
            let n = child_name.strip_prefix("s-").unwrap_or(child_name);
            format!("0.{n}")
        } else if needs_stitch_prefix {
            // Rule 1: just add stitch prefix
            format!("0.{child_name}")
        } else if child.kind == lir::ContainerKind::Sequence {
            // Rule 2: Sequence wrappers elsewhere: rename "s-N" → "N"
            child_name
                .strip_prefix("s-")
                .unwrap_or(child_name)
                .to_string()
        } else if container.kind == lir::ContainerKind::Sequence
            && child.kind == lir::ContainerKind::SequenceBranch
        {
            // Rule 3: Sequence branches: rename "N" → "sN"
            format!("s{child_name}")
        } else {
            child_name.to_string()
        };

        let child_path = if path.is_empty() {
            segment
        } else {
            format!("{path}.{segment}")
        };
        // If this child is a scope (knot, stitch, root), it starts a new scope.
        // Otherwise it inherits the parent's scope.
        let child_scope_id = if is_scope_kind(child.kind) {
            child.id
        } else {
            scope_id
        };
        // A scope child's author path is its own (author-form) path; other
        // children inherit the nearest enclosing scope's author path.
        let child_scope_author_path: &str = if is_scope_kind(child.kind) {
            &child_path
        } else {
            this_scope_path
        };
        walk_container(
            child,
            &child_path,
            child_scope_author_path,
            child_scope_id,
            state,
        );
    }
}

// ─── Top-level definition builders ─────────────────────────────────

fn build_globals(globals: &[lir::GlobalDef], state: &mut EmitState) -> Vec<GlobalVarDef> {
    globals
        .iter()
        .map(|g| GlobalVarDef {
            id: g.id,
            name: g.name,
            value_type: const_value_type(&g.default),
            default_value: const_to_value(&g.default, &mut state.name_table, &mut state.name_index),
            mutable: g.mutable,
            local: g.local,
        })
        .collect()
}

fn build_list_defs(lists: &[lir::ListDef]) -> Vec<ListDef> {
    lists
        .iter()
        .map(|l| ListDef {
            id: l.id,
            name: l.name,
            items: l.items.clone(),
        })
        .collect()
}

fn build_list_items(items: &[lir::ListItemDef]) -> Vec<ListItemDef> {
    items
        .iter()
        .map(|i| ListItemDef {
            id: i.id,
            origin: i.origin,
            ordinal: i.ordinal,
            name: i.name,
        })
        .collect()
}

fn build_externals(externals: &[lir::ExternalDef]) -> Vec<ExternalFnDef> {
    externals
        .iter()
        .map(|e| ExternalFnDef {
            id: e.id,
            name: e.name,
            arg_count: e.arg_count,
            fallback: e.fallback,
        })
        .collect()
}

fn const_value_type(v: &lir::ConstValue) -> brink_format::ValueType {
    match v {
        lir::ConstValue::Int(_) => brink_format::ValueType::Int,
        lir::ConstValue::Float(_) => brink_format::ValueType::Float,
        lir::ConstValue::Bool(_) => brink_format::ValueType::Bool,
        lir::ConstValue::String(_) => brink_format::ValueType::String,
        lir::ConstValue::List { .. } => brink_format::ValueType::List,
        lir::ConstValue::DivertTarget(_) => brink_format::ValueType::DivertTarget,
        lir::ConstValue::Null => brink_format::ValueType::Null,
        lir::ConstValue::Array(_) => brink_format::ValueType::Array,
        lir::ConstValue::Map(_) => brink_format::ValueType::Map,
        lir::ConstValue::Record { .. } => brink_format::ValueType::Record,
        lir::ConstValue::FnRef(_) => brink_format::ValueType::FnRef,
        lir::ConstValue::Closure { .. } => brink_format::ValueType::Closure,
    }
}

fn const_to_value(
    v: &lir::ConstValue,
    name_table: &mut Vec<String>,
    name_index: &mut HashMap<String, NameId>,
) -> Value {
    match v {
        lir::ConstValue::Int(n) => Value::Int(*n),
        lir::ConstValue::Float(f) => Value::Float(*f),
        lir::ConstValue::Bool(b) => Value::Bool(*b),
        lir::ConstValue::String(s) => Value::String(s.clone().into()),
        lir::ConstValue::Null => Value::Null,
        lir::ConstValue::DivertTarget(id) => Value::DivertTarget(*id),
        lir::ConstValue::List { items, origins } => Value::List(
            ListValue {
                items: items.clone(),
                origins: origins.clone(),
            }
            .into(),
        ),
        lir::ConstValue::Array(items) => Value::array(
            items
                .iter()
                .map(|i| const_to_value(i, name_table, name_index))
                .collect(),
        ),
        lir::ConstValue::Map(entries) => {
            let mut map = OrderedMap::with_capacity(entries.len());
            for (k, v) in entries {
                let val = const_to_value(v, name_table, name_index);
                map.insert(const_map_key_to_value(k), val);
            }
            Value::map(map)
        }
        // A record baked into a declaration default (#1530). `fields` is
        // already in the shape's declaration order — the same order
        // `RecordNew` pushes and `Value::Record` stores — so there is
        // nothing left to reorder here.
        lir::ConstValue::Record { shape_id, fields } => Value::record(
            brink_format::ShapeId(*shape_id),
            fields
                .iter()
                .map(|f| const_to_value(f, name_table, name_index))
                .collect(),
        ),
        // Function values baked into a declaration default (T1c, #700). The
        // param name is interned (deduped) into the story name table so it
        // resolves to the same string the target container's `params` table
        // carries — the runtime rehydration check compares the two by name.
        lir::ConstValue::FnRef(target) => Value::FnRef(*target),
        lir::ConstValue::Closure { target, env } => {
            let env = env
                .iter()
                .map(|e| match e {
                    lir::ConstClosureEntry::Val { name, value } => {
                        let payload = const_to_value(value, name_table, name_index);
                        brink_format::ClosureEnvEntry {
                            name: intern_into(name_table, name_index, name),
                            is_ref: false,
                            payload,
                        }
                    }
                    lir::ConstClosureEntry::Ref { name, cell } => brink_format::ClosureEnvEntry {
                        name: intern_into(name_table, name_index, name),
                        is_ref: true,
                        payload: Value::VariablePointer(*cell),
                    },
                })
                .collect();
            Value::closure(*target, env)
        }
    }
}

fn const_map_key_to_value(k: &lir::ConstMapKey) -> MapKey {
    match k {
        lir::ConstMapKey::Int(n) => MapKey::Int(*n),
        lir::ConstMapKey::Str(s) => MapKey::Str(s.clone().into()),
        lir::ConstMapKey::Bool(b) => MapKey::Bool(*b),
    }
}
