//! Opaque source provenance for IR nodes (contract Q1(b), issue #1148).
//!
//! Replaces the ink-CST-welded `AstPtr<ast::X>` / `SyntaxNodePtr` /
//! `ContainerPtr` fields (`docs/hir-admission-contract.md` D1): every HIR
//! node carries a [`Provenance`] — file + range + a node-kind token — and
//! the pipeline treats it as plain data. Resolving provenance back to a
//! live syntax node is delegated to the *producing frontend* via
//! [`ProvenanceResolver`]; a headless compile never resolves provenance
//! (contract §4.3), which is precisely why native codegen can ship before
//! native IDE support.
//!
//! [`Provenance`] is deliberately **plain, publicly constructible data** —
//! no frontend handle, no tree reference, no private field. A value
//! reconstructed from serialized parts (e.g. a future debug-info section:
//! bytecode offset → stored `(file, range, token)` → resolver → live node)
//! is indistinguishable from the value the frontend originally stamped,
//! and resolvers must treat it identically. The module lives at the crate
//! root (not under `hir`) because LIR reuses the same type verbatim in a
//! later epic.
//!
//! The token ([`KindToken`]) has two halves with different visibility
//! contracts:
//!
//! - [`KindToken::class`] — a frontend-agnostic [`NodeClass`] with a
//!   **stable public `u16` repr**, the **only** part of the token the
//!   pipeline may interpret. It carries the former `ContainerPtr`
//!   variant-discrimination role (F-I#5, the #626 floating-stitch trap): a
//!   top-level `= stitch` promoted to knot status keeps
//!   [`NodeClass::Stitch`] while a real `== knot` carries
//!   [`NodeClass::Knot`]. B0.3's admission validator checks this class
//!   against the indexed `SymbolKind`.
//! - [`KindToken::raw`] — the producing frontend's own syntax-kind value,
//!   opaque to everyone but that frontend's resolver. The ink frontend
//!   stamps `SyntaxKind as u16`; the native frontend (B0.5+) will stamp
//!   its own kind space. The pipeline must never branch on `raw`, and its
//!   values are **not** stable across frontends or versions.
//!
//! All provenance types are `Copy + Eq + Hash` — ranges keep their dual
//! cache-poison/identity-key role (contract F-J: salsa early-cutoff
//! compares `HirFile`s structurally, provenance included).

use rowan::TextRange;

use crate::hir::FileId;

// ─── Node classes ───────────────────────────────────────────────────

/// Frontend-agnostic class of the IR node a [`Provenance`] is stamped on.
///
/// This is an IR-level vocabulary, not a syntax-kind space: every frontend
/// maps its own grammar onto these classes when it stamps provenance. The
/// pipeline interprets **only** this class (today: the knot/stitch
/// container discrimination; B0.3 adds class ⇄ `SymbolKind` admission
/// checks). Per-frontend syntax kinds stay behind [`KindToken::raw`] and
/// the frontend's [`ProvenanceResolver`].
///
/// # Stable numeric namespace
///
/// Discriminants are a public, stable, append-only `u16` namespace
/// (convert with [`Self::as_u16`] / [`Self::from_u16`]): values are never
/// reused or renumbered once assigned, so downstream artifacts (e.g. a
/// debug-info projection into `brink-format`, which can never depend on
/// this crate) can store and reconstruct them without a hand-synced shadow
/// table. `0..=15` are reserved *generic* classes for producers that have
/// no more specific class; specific classes start at `16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum NodeClass {
    // ── Reserved generic classes (0..=15) ───────────────────────────
    /// Generic statement — reserved coarse fallback (not stamped by the
    /// ink lowering; exists for projections/producers without a specific
    /// class).
    Stmt = 0,
    /// Generic expression — reserved coarse fallback (see [`Self::Stmt`]).
    Expr = 1,

    // ── Specific classes (16..) — append-only, never reused ─────────
    // (Appending one also means adding its `from_u16` arm and bumping the
    // `node_class_u16_round_trips` sentinel to the new last variant.)
    /// A `Tag` attached to content.
    Tag = 16,
    /// A knot definition (`== knot`). A `hir::Knot` carries this class only
    /// when it originated from a real knot definition; see [`Self::Stitch`].
    Knot = 17,
    /// A stitch definition (`= stitch`) — including a top-level stitch
    /// *promoted* to `hir::Knot` status during lowering (the promoted node
    /// keeps `Stitch` class; this is the former `ContainerPtr::Stitch`
    /// discrimination, F-I#5).
    Stitch = 18,
    /// A `~ { … }` logic block.
    LogicBlock = 19,
    /// A `break` statement inside a logic block.
    Break = 20,
    /// A `continue` statement inside a logic block.
    Continue = 21,
    /// An `if` statement inside a logic block.
    If = 22,
    /// A `while` statement inside a logic block.
    While = 23,
    /// An `await` suspension point.
    Await = 24,
    /// A `for` statement inside a logic block.
    For = 25,
    /// A choice line (`*` / `+`).
    Choice = 26,
    /// A content (text output) line.
    Content = 27,
    /// A conditional block (multiline or promoted inline).
    Conditional = 28,
    /// A sequence block (stopping/cycle/once/shuffle).
    Sequence = 29,
    /// A divert (`-> target`).
    Divert = 30,
    /// A tunnel call (`-> target ->`).
    TunnelCall = 31,
    /// A thread start (`<- target`).
    ThreadStart = 32,
    /// A `~ return` statement.
    Return = 33,
    /// A `#fn(…)` function-value literal.
    FnLiteral = 34,
    /// A `ref lvalue` path-projection expression.
    RefArg = 35,
    /// A `Name#{…}` struct-construction literal.
    StructLiteral = 36,
    /// A `base.field` field access.
    FieldAccess = 37,
    /// A `#[…]` array literal.
    ArrayLiteral = 38,
    /// A `#{…}` map literal.
    MapLiteral = 39,
    /// A `base[index]` index expression.
    Index = 40,
    /// A `start..end` range literal.
    Range = 41,
    /// A `VAR` declaration.
    VarDecl = 42,
    /// A `CONST` declaration.
    ConstDecl = 43,
    /// A `~ temp` declaration.
    TempDecl = 44,
    /// An assignment statement.
    Assignment = 45,
    /// A `LIST` declaration.
    ListDecl = 46,
    /// A `STRUCT` declaration.
    StructDecl = 47,
    /// An `EXTERNAL` declaration.
    ExternalDecl = 48,
    /// An `INCLUDE` site.
    Include = 49,
    /// An infix (binary) operation — `lhs op rhs` (issue #1517).
    Infix = 50,
    /// One branch of a multiline/inline conditional (issue #404) — the
    /// branch's condition-plus-body span, distinct from the enclosing
    /// [`Self::Conditional`]'s whole-construct span. Lets a diagnostic or
    /// editor decoration (e.g. a fold run) anchor to a single `- else:`
    /// arm instead of the entire `{ ... }` block.
    ConditionalBranch = 51,
    /// One branch (alternative) of a sequence/alternation block (issue
    /// #404), mirroring [`Self::ConditionalBranch`] for `- ...` sequence
    /// arms and `|`-separated inline alternatives.
    SequenceBranch = 52,
    /// A `|x| …` lambda expression — the native surface's anonymous fn
    /// value (RULED 2026-07-19, issue #1685). Native-only: ink's grammar
    /// cannot spell a lambda.
    Lambda = 53,
    /// An inline markup span (`<name attr="v">…</name>`, issue #1716).
    /// Native-only: ink's grammar cannot spell markup. Stamped per-span so
    /// diagnostics (`E164`/`E165`, issue #1782) can point at the exact span
    /// rather than its enclosing content line.
    Span = 54,
}

impl NodeClass {
    /// The stable `u16` value of this class (see the type docs for the
    /// namespace rules).
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    /// Reconstruct a class from its stable `u16` value. `None` for values
    /// this crate version doesn't know (reserved-but-unassigned or newer).
    #[must_use]
    pub const fn from_u16(value: u16) -> Option<Self> {
        Some(match value {
            0 => Self::Stmt,
            1 => Self::Expr,
            16 => Self::Tag,
            17 => Self::Knot,
            18 => Self::Stitch,
            19 => Self::LogicBlock,
            20 => Self::Break,
            21 => Self::Continue,
            22 => Self::If,
            23 => Self::While,
            24 => Self::Await,
            25 => Self::For,
            26 => Self::Choice,
            27 => Self::Content,
            28 => Self::Conditional,
            29 => Self::Sequence,
            30 => Self::Divert,
            31 => Self::TunnelCall,
            32 => Self::ThreadStart,
            33 => Self::Return,
            34 => Self::FnLiteral,
            35 => Self::RefArg,
            36 => Self::StructLiteral,
            37 => Self::FieldAccess,
            38 => Self::ArrayLiteral,
            39 => Self::MapLiteral,
            40 => Self::Index,
            41 => Self::Range,
            42 => Self::VarDecl,
            43 => Self::ConstDecl,
            44 => Self::TempDecl,
            45 => Self::Assignment,
            46 => Self::ListDecl,
            47 => Self::StructDecl,
            48 => Self::ExternalDecl,
            49 => Self::Include,
            50 => Self::Infix,
            51 => Self::ConditionalBranch,
            52 => Self::SequenceBranch,
            53 => Self::Lambda,
            54 => Self::Span,
            _ => return None,
        })
    }
}

// ─── Kind token ─────────────────────────────────────────────────────

/// The node-kind token component of [`Provenance`].
///
/// See the module docs for the class/raw visibility split. The whole token
/// round-trips through a `u32` ([`Self::as_u32`] / [`Self::from_u32`]) for
/// storage in artifacts that cannot depend on this crate; only the class
/// half of that value is stable — `raw` is frontend-private.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KindToken {
    /// Frontend-agnostic node class — the only pipeline-interpretable half.
    pub class: NodeClass,
    /// Frontend-private raw syntax kind (ink: `SyntaxKind as u16`).
    /// [`Self::SYNTHETIC_RAW`] marks fabricated provenance that no
    /// frontend's resolver will ever resolve.
    pub raw: u16,
}

impl KindToken {
    /// Raw-kind value stamped on fabricated (synthesized/test) provenance.
    /// No frontend occupies this value, so synthetic provenance never
    /// resolves — the same posture as the retired `AstPtr::from_range`
    /// dummies (which stamped `SyntaxKind::ERROR`).
    pub const SYNTHETIC_RAW: u16 = u16::MAX;

    /// A token with the given class and the synthetic raw kind.
    #[must_use]
    pub const fn synthetic(class: NodeClass) -> Self {
        Self {
            class,
            raw: Self::SYNTHETIC_RAW,
        }
    }

    /// Pack the token into a `u32`: class in the high half, raw in the low.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        ((self.class.as_u16() as u32) << 16) | self.raw as u32
    }

    /// Unpack a token packed by [`Self::as_u32`]. `None` when the class
    /// half is unknown to this crate version.
    #[must_use]
    pub fn from_u32(value: u32) -> Option<Self> {
        // Both halves are lossless: `>> 16` and `& 0xFFFF` fit u16.
        let Ok(class) = u16::try_from(value >> 16) else {
            return None;
        };
        let Ok(raw) = u16::try_from(value & 0xFFFF) else {
            return None;
        };
        NodeClass::from_u16(class).map(|class| Self { class, raw })
    }
}

// ─── Provenance ─────────────────────────────────────────────────────

/// Opaque source provenance carried by every HIR node.
///
/// `file` + `range` locate the originating source text; `kind` carries the
/// node-kind token (see [`KindToken`]). The range is real source geometry —
/// diagnostic anchor, IDE geometry, and (for referencing expressions via
/// `Name`/`Path` ranges) resolution join key — and must be non-empty and
/// in-bounds for admission (contract §1.3, checked loudly from B0.3 on).
///
/// Plain data by design: construct it with [`Self::new`] from any source —
/// a frontend lowering, a test, or deserialized debug info. There is no
/// hidden state; two values with equal fields are the same provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Provenance {
    /// The source file this node was lowered from.
    pub file: FileId,
    /// The originating node's source range.
    pub range: TextRange,
    /// The node-kind token (class + frontend-private raw kind).
    pub kind: KindToken,
}

impl Provenance {
    /// Construct provenance from its parts.
    #[must_use]
    pub const fn new(file: FileId, range: TextRange, kind: KindToken) -> Self {
        Self { file, range, kind }
    }

    /// The source range this provenance points at.
    ///
    /// Named for continuity with the retired `AstPtr::text_range` /
    /// `SyntaxNodePtr::text_range` accessors so range-only consumers
    /// migrate mechanically.
    #[must_use]
    pub const fn text_range(&self) -> TextRange {
        self.range
    }

    /// The frontend-agnostic node class.
    #[must_use]
    pub const fn class(&self) -> NodeClass {
        self.kind.class
    }

    /// Fabricated provenance for synthesized or test-built nodes.
    ///
    /// Carries a real `range` (ranges stay identity keys even on synthetic
    /// nodes) but a file/raw pair no frontend claims, so it never resolves
    /// against any syntax tree.
    #[must_use]
    pub fn synthetic(class: NodeClass, range: TextRange) -> Self {
        Self {
            file: FileId(u32::MAX),
            range,
            kind: KindToken::synthetic(class),
        }
    }
}

// ─── Resolver seam ──────────────────────────────────────────────────

/// Frontend-supplied node resolution — the contract's provenance seam
/// (Q1(b), `docs/hir-admission-contract.md` D1/§4.3).
///
/// The pipeline stores only opaque [`Provenance`]; mapping it back to a
/// live syntax node is the producing frontend's job. IDE features that
/// need a live node (rename, extract) go through the frontend's resolver;
/// everything headless (analysis, LIR, codegen) consumes provenance as
/// data and never resolves it.
///
/// Resolution is keyed by the provenance **value** alone: a resolver must
/// accept any well-formed [`Provenance`] — including one reconstructed
/// from serialized parts it never minted — and answer `None` (a normal
/// answer, not an error) for anything foreign, synthetic, or stale.
///
/// The ink frontend's implementation is
/// [`crate::hir::InkProvenanceResolver`]; the native frontend (B0.5+)
/// supplies its own.
pub trait ProvenanceResolver {
    /// The frontend's live syntax-node type.
    type Node;

    /// Resolve `provenance` back to a live node.
    ///
    /// Returns `None` when the provenance belongs to another file or
    /// frontend, is synthetic, or is stale (the tree changed since it was
    /// stamped).
    fn resolve(&self, provenance: Provenance) -> Option<Self::Node>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rowan::{TextRange, TextSize};

    #[test]
    fn node_class_u16_round_trips() {
        for v in 0..=u16::MAX {
            if let Some(class) = NodeClass::from_u16(v) {
                assert_eq!(class.as_u16(), v);
            }
        }
        // One past the *last* assigned class is unknown. Appending a class
        // to the enum means bumping this name to the new last variant —
        // otherwise the sentinel starts naming an assigned value and this
        // assertion fails.
        assert_eq!(NodeClass::from_u16(NodeClass::Span.as_u16() + 1), None);
        assert_eq!(NodeClass::from_u16(2), None, "generic range is reserved");
    }

    #[test]
    fn kind_token_u32_round_trips() {
        let token = KindToken {
            class: NodeClass::Stitch,
            raw: 137,
        };
        assert_eq!(KindToken::from_u32(token.as_u32()), Some(token));
        // Unknown class half → None.
        assert_eq!(KindToken::from_u32(0xFFFF_0000), None);
    }

    #[test]
    fn provenance_is_plain_reconstructible_data() {
        let range = TextRange::new(TextSize::new(3), TextSize::new(9));
        let original = Provenance::new(
            FileId(7),
            range,
            KindToken {
                class: NodeClass::Knot,
                raw: 42,
            },
        );
        // Reconstruct from serialized-style parts.
        let rebuilt = Provenance::new(
            FileId(original.file.0),
            original.range,
            KindToken::from_u32(original.kind.as_u32()).unwrap(),
        );
        assert_eq!(original, rebuilt);
    }
}
