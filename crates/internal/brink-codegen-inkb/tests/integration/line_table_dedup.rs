#![allow(clippy::unwrap_used, clippy::panic)]

//! Line-table deduplication at emission (`docs/intl-spec.md` §"Line-table
//! deduplication"): a line authored more than once in one scope is one
//! translation unit. `add_line` / `add_template_line` used to append one
//! entry per occurrence site while `intern_string`, two functions away,
//! already deduplicated its neighbours; `TheIntercept` shipped `Lie` as
//! twenty-seven separate units.
//!
//! The one place dedup must NOT apply is a variant run (#3273): the runtime
//! finds a leaf at `base + combo`, so the run's entries stay consecutive and
//! one-per-leaf even when two leaves read the same, and nothing later merges
//! into them.

use brink_format::{
    CountingFlags, DefinitionId, DefinitionTag, LineContent, LinePart, Opcode, SlotInfo,
};
use brink_ir::lir;

fn id(n: u64) -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, n)
}

fn prov() -> brink_ir::Provenance {
    brink_ir::Provenance::synthetic(brink_ir::NodeClass::Stmt, rowan::TextRange::empty(0.into()))
}

fn plain(text: &str, source_hash: u64) -> lir::ContentEmission {
    lir::ContentEmission {
        line: lir::RecognizedLine::Plain(text.to_string()),
        metadata: lir::LineMetadata {
            source_hash,
            slot_info: Vec::new(),
            source_location: None,
        },
        tags: Vec::new(),
    }
}

fn template(parts: Vec<LinePart>, slot_names: &[&str]) -> lir::ContentEmission {
    lir::ContentEmission {
        line: lir::RecognizedLine::Template {
            parts,
            slot_exprs: Vec::new(),
        },
        metadata: lir::LineMetadata {
            source_hash: 0,
            slot_info: slot_names
                .iter()
                .enumerate()
                .map(|(i, n)| SlotInfo {
                    index: u8::try_from(i).unwrap(),
                    name: (*n).to_string(),
                })
                .collect(),
            source_location: None,
        },
        tags: Vec::new(),
    }
}

fn line(e: lir::ContentEmission) -> lir::Stmt {
    lir::Stmt::new(lir::StmtKind::EmitLine(e), prov())
}

fn eol() -> lir::Stmt {
    lir::Stmt::new(lir::StmtKind::EndOfLine, prov())
}

fn container(cid: DefinitionId, kind: lir::ContainerKind, body: Vec<lir::Stmt>) -> lir::Container {
    lir::Container {
        id: cid,
        provenance: prov(),
        name: None,
        kind,
        params: Vec::new(),
        body,
        children: Vec::new(),
        counting_flags: CountingFlags::VISITS,
        temp_slot_count: 0,
        labeled: false,
        inline: false,
        is_function: false,
        local: false,
    }
}

fn program(body: Vec<lir::Stmt>, children: Vec<lir::Container>) -> lir::Program {
    let mut root = container(id(1), lir::ContainerKind::Root, body);
    root.children = children;
    lir::Program {
        root,
        globals: Vec::new(),
        lists: Vec::new(),
        list_items: Vec::new(),
        externals: Vec::new(),
        name_table: Vec::new(),
        struct_shapes: Vec::new(),
        private_defs: Vec::new(),
        aliases: Vec::new(),
        file_paths: std::collections::BTreeMap::new(),
    }
}

fn emit_line_ops(story: &brink_format::StoryData) -> Vec<(u16, u8)> {
    let root = story.containers.iter().find(|c| c.id == id(1)).unwrap();
    let mut offset = 0;
    let mut out = Vec::new();
    while offset < root.bytecode.len() {
        if let Opcode::EmitLine(idx, slots) = Opcode::decode(&root.bytecode, &mut offset).unwrap() {
            out.push((idx, slots));
        }
    }
    out
}

fn lines(story: &brink_format::StoryData) -> &[brink_format::LineEntry] {
    let table = story
        .line_tables
        .iter()
        .find(|t| t.scope_id == id(1))
        .unwrap();
    &table.lines
}

#[test]
fn a_line_authored_twice_in_one_scope_is_one_entry() {
    let story = brink_codegen_inkb::emit(&program(
        vec![
            line(plain("Lie", 0xA)),
            eol(),
            line(plain("Evade", 0xB)),
            eol(),
            line(plain("Lie", 0xC)),
            eol(),
        ],
        Vec::new(),
    ))
    .unwrap();
    let table = lines(&story);
    assert_eq!(table.len(), 2, "{table:?}");
    assert_eq!(table[0].content, LineContent::Plain("Lie".into()));
    assert_eq!(
        table[0].source_hash, 0xA,
        "the first occurrence supplies the unit's hash"
    );
    assert_eq!(
        emit_line_ops(&story),
        vec![(0, 0), (1, 0), (0, 0)],
        "both `Lie` sites emit the same index"
    );
}

#[test]
fn whitespace_is_collapsed_before_the_key_is_compared() {
    let story = brink_codegen_inkb::emit(&program(
        vec![line(plain("a  b", 1)), eol(), line(plain("a b", 2)), eol()],
        Vec::new(),
    ))
    .unwrap();
    assert_eq!(lines(&story).len(), 1);
}

#[test]
fn templates_dedup_on_parts_and_slot_names_together() {
    let parts = || {
        vec![
            LinePart::Literal("Hello ".into()),
            LinePart::Slot(0),
            LinePart::Literal("!".into()),
        ]
    };
    let story = brink_codegen_inkb::emit(&program(
        vec![
            line(template(parts(), &["name"])),
            eol(),
            line(template(parts(), &["name"])),
            eol(),
            line(template(parts(), &["title"])),
            eol(),
        ],
        Vec::new(),
    ))
    .unwrap();
    let table = lines(&story);
    assert_eq!(
        table.len(),
        2,
        "same parts + same slot names merge; a different slot name is a different unit"
    );
    assert_eq!(emit_line_ops(&story), vec![(0, 0), (0, 0), (1, 0)]);
}

#[test]
fn variant_runs_stay_one_entry_per_leaf_and_are_not_merge_targets() {
    let alt = id(7);
    let variants = lir::VariantLineEmission {
        alts: vec![lir::VariantAltEmission {
            container_id: alt,
            kind: brink_ir::SequenceType::STOPPING,
            branch_count: 2,
        }],
        dims: vec![2],
        variants: vec![plain("Hi", 1), plain("Hi", 2)],
    };
    let story = brink_codegen_inkb::emit(&program(
        vec![
            lir::Stmt::new(lir::StmtKind::EmitLineVariants(variants.clone()), prov()),
            lir::Stmt::new(lir::StmtKind::EmitLineVariants(variants), prov()),
            line(plain("Hi", 3)),
            eol(),
        ],
        vec![container(alt, lir::ContainerKind::Sequence, Vec::new())],
    ))
    .unwrap();
    let table = lines(&story);
    assert_eq!(
        table.len(),
        5,
        "two runs of two identical leaves each (4 entries, never merged) plus one \
         plain `Hi` that must not point into a run: {table:?}"
    );
    let bases: Vec<u32> = story.line_variant_groups.iter().map(|g| g.base).collect();
    assert_eq!(bases, vec![0, 2], "runs are consecutive and intact");
    assert_eq!(table[4].source_hash, 3, "the plain line is its own entry");
}
