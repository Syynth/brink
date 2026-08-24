//! Range/file rebasing for per-knot incremental lowering (issue #3084,
//! `docs/per-knot-incremental-lowering-spec.md` §3 step 3).
//!
//! Per-segment lowering parses one segment's text in isolation, so every
//! position it produces — `Provenance` ranges, `Diagnostic` ranges, raw
//! `TextRange` fields — is segment-relative, and every `FileId` stamp is
//! the placeholder the segment memo was lowered under. [`Rebase`] shifts
//! a lowered product to its segment's current absolute offset and stamps
//! the real file, one add per position.
//!
//! **Completeness contract:** every impl below must touch every
//! positional field of its type. The oracle is the corpus equality gate
//! (`brink-db`'s assembled-vs-whole-file sweep): a missed field produces
//! a range mismatch against the whole-file lowering on the first corpus
//! file that exercises the construct, so omissions — including fields
//! added to `types.rs` later — fail loudly rather than shipping stale
//! positions. The impl bodies were generated mechanically from
//! `types.rs`'s field lists and reviewed by hand.

#![expect(
    clippy::single_match,
    clippy::match_wildcard_for_single_variants,
    clippy::match_same_arms,
    clippy::only_used_in_recursion,
    reason = "mechanically generated from types.rs field lists (see module \
               doc): uniform arm/impl shapes are the point — hand-shaping \
               them per lint would defeat regeneration, and the corpus \
               equality gate, not style, is the correctness check"
)]

use rowan::TextSize;

use crate::provenance::Provenance;
use crate::{Diagnostic, FileId};

use super::types::{
    ArrayLiteral, AssignOp, Assignment, AwaitStmt, Block, BlockStmt, Choice, ChoiceSet,
    ChoiceSetContext, ClaimHandlerDecl, CondBranch, CondKind, Conditional, ConstDecl, Content,
    ContentPart, ConventionAnnotation, ConventionAttachField, ConventionAttachSchema,
    ConventionMode, ConventionProjectionEntry, ConventionsProjection, CueSite, DispatchHandlerDecl,
    Divert, DivertPath, DivertTarget, EffectsAssertion, ElementAnnotation, ElementCapture,
    ElementDisposition, ElementKind, ElementMatch, ElseBranch, Expr, ExternalDecl, FieldAccessExpr,
    FnLiteral, ForStmt, HirFile, IfStmt, Import, ImportItem, IncludeSite, IndexExpr, InfixExpr,
    InfixOp, Knot, LambdaBody, LambdaExpr, ListDecl, ListMember, LogicBlock, LogicBlockScope,
    MapLiteral, ModuleDecl, Name, Param, Path, PostfixOp, PrefixOp, RangeExpr, RefArgExpr, Return,
    ReturnKind, SchemaTypeShape, Sequence, SequenceBranch, SpanAttr, SpanPart, Stitch, Stmt,
    StringExpr, StringPart, StructDecl, StructFieldDecl, StructLiteral, StyleAnnotation,
    StyleEntry, StyleToken, Tag, Tail, TempDecl, Terminator, ThreadStart, TunnelCall, TypeExpr,
    VarDecl, VisibilityDirective, WhileStmt,
};

/// Shift every position in `self` by `delta` and stamp `file` on every
/// file-carrying field. See the module doc's completeness contract.
pub trait Rebase {
    fn rebase(&mut self, delta: TextSize, file: FileId);
}

impl Rebase for rowan::TextRange {
    fn rebase(&mut self, delta: TextSize, _file: FileId) {
        *self += delta;
    }
}

impl Rebase for Provenance {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range += delta;
        self.file = file;
    }
}

impl Rebase for Diagnostic {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range += delta;
        self.file = file;
    }
}

impl Rebase for Name {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range.rebase(delta, file);
    }
}

impl Rebase for Path {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        for x in &mut *self.segments {
            x.rebase(delta, file);
        }
        self.range.rebase(delta, file);
    }
}

impl Rebase for Tag {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        for x in &mut *self.parts {
            x.rebase(delta, file);
        }
        self.ptr.rebase(delta, file);
    }
}

impl Rebase for HirFile {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.root_content.rebase(delta, file);
        for x in &mut *self.knots {
            x.rebase(delta, file);
        }
        for x in &mut *self.variables {
            x.rebase(delta, file);
        }
        for x in &mut *self.constants {
            x.rebase(delta, file);
        }
        for x in &mut *self.lists {
            x.rebase(delta, file);
        }
        for x in &mut *self.structs {
            x.rebase(delta, file);
        }
        for x in &mut *self.externals {
            x.rebase(delta, file);
        }
        for x in &mut *self.includes {
            x.rebase(delta, file);
        }
        if let Some(x) = self.module.as_mut() {
            x.rebase(delta, file);
        }
        for x in &mut *self.imports {
            x.rebase(delta, file);
        }
        for x in &mut *self.visibility {
            x.rebase(delta, file);
        }
        for x in &mut *self.was_directives {
            x.rebase(delta, file);
        }
        for x in &mut *self.element_matches {
            x.rebase(delta, file);
        }
        for x in &mut *self.cue_names {
            x.rebase(delta, file);
        }
        for x in &mut *self.claim_handlers {
            x.rebase(delta, file);
        }
        for x in &mut *self.dispatch_handlers {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ModuleDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range.rebase(delta, file);
    }
}

impl Rebase for Import {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.module_range.rebase(delta, file);
        for x in &mut *self.items {
            x.rebase(delta, file);
        }
        self.range.rebase(delta, file);
    }
}

impl Rebase for ImportItem {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range.rebase(delta, file);
    }
}

impl Rebase for VisibilityDirective {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range.rebase(delta, file);
    }
}

impl Rebase for EffectsAssertion {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range.rebase(delta, file);
    }
}

impl Rebase for ElementAnnotation {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range.rebase(delta, file);
    }
}

impl Rebase for ConventionAnnotation {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        if let Some(x) = self.attach.as_mut() {
            x.rebase(delta, file);
        }
        self.range.rebase(delta, file);
    }
}

impl Rebase for ElementKind {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for ElementCapture {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range.rebase(delta, file);
    }
}

impl Rebase for ElementDisposition {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for ElementMatch {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.line.rebase(delta, file);
        self.kind.rebase(delta, file);
        self.handler.rebase(delta, file);
        self.annotation.rebase(delta, file);
        for x in &mut *self.captures {
            x.rebase(delta, file);
        }
        self.disposition.rebase(delta, file);
        if let Some(x) = self.content.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.slug.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ClaimHandlerDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.name.rebase(delta, file);
        self.annotation.rebase(delta, file);
    }
}

impl Rebase for DispatchHandlerDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.name.rebase(delta, file);
        self.annotation.rebase(delta, file);
    }
}

impl Rebase for ConventionMode {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for SchemaTypeShape {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Generic { args, .. } => {
                for x in &mut *args {
                    x.rebase(delta, file);
                }
            }
            Self::Fn { params, ret, .. } => {
                for x in &mut *params {
                    x.rebase(delta, file);
                }
                ret.rebase(delta, file);
            }
            _ => {}
        }
    }
}

impl Rebase for ConventionAttachField {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ty.rebase(delta, file);
    }
}

impl Rebase for ConventionAttachSchema {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Resolved { fields, .. } => {
                for x in &mut *fields {
                    x.rebase(delta, file);
                }
            }
            _ => {}
        }
    }
}

impl Rebase for ConventionProjectionEntry {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.name.rebase(delta, file);
        self.mode.rebase(delta, file);
        self.disposition.rebase(delta, file);
        if let Some(x) = self.attach.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ConventionsProjection {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        for x in &mut *self.entries {
            x.rebase(delta, file);
        }
        for x in &mut *self.dispatch {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for CueSite {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.range.rebase(delta, file);
    }
}

impl Rebase for StyleToken {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for StyleEntry {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.value.rebase(delta, file);
        self.range.rebase(delta, file);
    }
}

impl Rebase for StyleAnnotation {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        for x in &mut *self.entries {
            x.rebase(delta, file);
        }
        self.range.rebase(delta, file);
    }
}

impl Rebase for Knot {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.name.rebase(delta, file);
        for x in &mut *self.params {
            x.rebase(delta, file);
        }
        self.body.rebase(delta, file);
        for x in &mut *self.stitches {
            x.rebase(delta, file);
        }
        if let Some(x) = self.effects_assertion.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.element_annotation.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.convention_annotation.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.style_annotation.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.return_type.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for Stitch {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.name.rebase(delta, file);
        for x in &mut *self.params {
            x.rebase(delta, file);
        }
        self.body.rebase(delta, file);
        if let Some(x) = self.effects_assertion.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.element_annotation.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.convention_annotation.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.style_annotation.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.return_type.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for Param {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.name.rebase(delta, file);
        if let Some(x) = self.annotation.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for TypeExpr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Named { range, .. } => {
                range.rebase(delta, file);
            }
            Self::Generic { args, range, .. } => {
                for x in &mut *args {
                    x.rebase(delta, file);
                }
                range.rebase(delta, file);
            }
            Self::Fn {
                params, ret, range, ..
            } => {
                for x in &mut *params {
                    x.rebase(delta, file);
                }
                ret.rebase(delta, file);
                range.rebase(delta, file);
            }
        }
    }
}

impl Rebase for Block {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        if let Some(x) = self.label.as_mut() {
            x.rebase(delta, file);
        }
        for x in &mut *self.stmts {
            x.rebase(delta, file);
        }
        self.tail.rebase(delta, file);
    }
}

impl Rebase for Tail {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Value(f0) => {
                f0.rebase(delta, file);
            }
            Self::Diverge(f0) => {
                f0.rebase(delta, file);
            }
            _ => {}
        }
    }
}

impl Rebase for Terminator {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Divert(f0) => {
                f0.rebase(delta, file);
            }
            Self::Return(f0) => {
                f0.rebase(delta, file);
            }
        }
    }
}

impl Rebase for Stmt {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Content(f0) => {
                f0.rebase(delta, file);
            }
            Self::Divert(f0) => {
                f0.rebase(delta, file);
            }
            Self::TunnelCall(f0) => {
                f0.rebase(delta, file);
            }
            Self::ThreadStart(f0) => {
                f0.rebase(delta, file);
            }
            Self::TempDecl(f0) => {
                f0.rebase(delta, file);
            }
            Self::Assignment(f0) => {
                f0.rebase(delta, file);
            }
            Self::Return(f0) => {
                f0.rebase(delta, file);
            }
            Self::ChoiceSet(f0) => {
                f0.rebase(delta, file);
            }
            Self::LabeledBlock(f0) => {
                f0.rebase(delta, file);
            }
            Self::Conditional(f0) => {
                f0.rebase(delta, file);
            }
            Self::Sequence(f0) => {
                f0.rebase(delta, file);
            }
            Self::ExprStmt(f0) => {
                f0.rebase(delta, file);
            }
            Self::LogicBlock(f0) => {
                f0.rebase(delta, file);
            }
            Self::Await(f0) => {
                f0.rebase(delta, file);
            }
            Self::AttachElement(f0) => {
                f0.rebase(delta, file);
            }
            _ => {}
        }
    }
}

impl Rebase for LogicBlock {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        for x in &mut *self.stmts {
            x.rebase(delta, file);
        }
        self.scope.rebase(delta, file);
    }
}

impl Rebase for LogicBlockScope {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for BlockStmt {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::TempDecl(f0) => {
                f0.rebase(delta, file);
            }
            Self::Assignment(f0) => {
                f0.rebase(delta, file);
            }
            Self::Return(f0) => {
                f0.rebase(delta, file);
            }
            Self::If(f0) => {
                f0.rebase(delta, file);
            }
            Self::While(f0) => {
                f0.rebase(delta, file);
            }
            Self::For(f0) => {
                f0.rebase(delta, file);
            }
            Self::Break(f0) => {
                f0.rebase(delta, file);
            }
            Self::Continue(f0) => {
                f0.rebase(delta, file);
            }
            Self::ExprStmt(f0) => {
                f0.rebase(delta, file);
            }
            Self::Await(f0) => {
                f0.rebase(delta, file);
            }
        }
    }
}

impl Rebase for IfStmt {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.condition.rebase(delta, file);
        if let Some(x) = self.binding.as_mut() {
            x.rebase(delta, file);
        }
        for x in &mut *self.body {
            x.rebase(delta, file);
        }
        if let Some(x) = self.else_branch.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ElseBranch {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::ElseIf(f0) => {
                f0.rebase(delta, file);
            }
            Self::Else(f0) => {
                for x in &mut *f0 {
                    x.rebase(delta, file);
                }
            }
        }
    }
}

impl Rebase for WhileStmt {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.condition.rebase(delta, file);
        if let Some(x) = self.binding.as_mut() {
            x.rebase(delta, file);
        }
        for x in &mut *self.body {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for AwaitStmt {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        if let Some(x) = self.condition.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ForStmt {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.var_name.rebase(delta, file);
        if let Some(x) = self.val_name.as_mut() {
            x.rebase(delta, file);
        }
        self.iterable.rebase(delta, file);
        for x in &mut *self.body {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ChoiceSetContext {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for ChoiceSet {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        for x in &mut *self.choices {
            x.rebase(delta, file);
        }
        self.continuation.rebase(delta, file);
        self.context.rebase(delta, file);
    }
}

impl Rebase for Choice {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        if let Some(x) = self.label.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.condition.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.binding.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.start_content.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.bracket_content.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.inner_content.as_mut() {
            x.rebase(delta, file);
        }
        for x in &mut *self.tags {
            x.rebase(delta, file);
        }
        self.body.rebase(delta, file);
    }
}

impl Rebase for Content {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        if let Some(x) = self.ptr.as_mut() {
            x.rebase(delta, file);
        }
        for x in &mut *self.parts {
            x.rebase(delta, file);
        }
        for x in &mut *self.tags {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ContentPart {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Interpolation(f0) => {
                f0.rebase(delta, file);
            }
            Self::InlineConditional(f0) => {
                f0.rebase(delta, file);
            }
            Self::InlineSequence(f0) => {
                f0.rebase(delta, file);
            }
            Self::Span(f0) => {
                f0.rebase(delta, file);
            }
            _ => {}
        }
    }
}

impl Rebase for SpanPart {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        for x in &mut *self.attrs {
            x.rebase(delta, file);
        }
        for x in &mut *self.children {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for SpanAttr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
    }
}

impl Rebase for CondKind {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Switch(f0) => {
                f0.rebase(delta, file);
            }
            _ => {}
        }
    }
}

impl Rebase for Conditional {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.kind.rebase(delta, file);
        for x in &mut *self.branches {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for CondBranch {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        if let Some(x) = self.condition.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.binding.as_mut() {
            x.rebase(delta, file);
        }
        self.body.rebase(delta, file);
    }
}

impl Rebase for Sequence {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        for x in &mut *self.branches {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for SequenceBranch {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.body.rebase(delta, file);
    }
}

impl Rebase for Divert {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        if let Some(x) = self.ptr.as_mut() {
            x.rebase(delta, file);
        }
        self.target.rebase(delta, file);
    }
}

impl Rebase for TunnelCall {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        for x in &mut *self.targets {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ThreadStart {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.target.rebase(delta, file);
    }
}

impl Rebase for DivertTarget {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.path.rebase(delta, file);
        for x in &mut *self.args {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for DivertPath {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Path(f0) => {
                f0.rebase(delta, file);
            }
            _ => {}
        }
    }
}

impl Rebase for Return {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        if let Some(x) = self.ptr.as_mut() {
            x.rebase(delta, file);
        }
        self.kind.rebase(delta, file);
        if let Some(x) = self.value.as_mut() {
            x.rebase(delta, file);
        }
        for x in &mut *self.onwards_args {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ReturnKind {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for Expr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::String(f0) => {
                f0.rebase(delta, file);
            }
            Self::Path(f0) => {
                f0.rebase(delta, file);
            }
            Self::DivertTarget(f0) => {
                f0.rebase(delta, file);
            }
            Self::ListLiteral(f0) => {
                for x in &mut *f0 {
                    x.rebase(delta, file);
                }
            }
            Self::Prefix(f0, f1) => {
                f0.rebase(delta, file);
                f1.rebase(delta, file);
            }
            Self::Infix(f0) => {
                f0.rebase(delta, file);
            }
            Self::Postfix(f0, f1) => {
                f0.rebase(delta, file);
                f1.rebase(delta, file);
            }
            Self::Call(f0, f1) => {
                f0.rebase(delta, file);
                for x in &mut *f1 {
                    x.rebase(delta, file);
                }
            }
            Self::ArrayLiteral(f0) => {
                f0.rebase(delta, file);
            }
            Self::MapLiteral(f0) => {
                f0.rebase(delta, file);
            }
            Self::Index(f0) => {
                f0.rebase(delta, file);
            }
            Self::Range(f0) => {
                f0.rebase(delta, file);
            }
            Self::StructLiteral(f0) => {
                f0.rebase(delta, file);
            }
            Self::FieldAccess(f0) => {
                f0.rebase(delta, file);
            }
            Self::FnLiteral(f0) => {
                f0.rebase(delta, file);
            }
            Self::Lambda(f0) => {
                f0.rebase(delta, file);
            }
            Self::RefArg(f0) => {
                f0.rebase(delta, file);
            }
            Self::Fragment(f0) => {
                for x in &mut *f0 {
                    x.rebase(delta, file);
                }
            }
            _ => {}
        }
    }
}

impl Rebase for FnLiteral {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.target.rebase(delta, file);
        for x in &mut *self.args {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for LambdaExpr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        for x in &mut *self.params {
            x.rebase(delta, file);
        }
        if let Some(x) = self.return_type.as_mut() {
            x.rebase(delta, file);
        }
        self.body.rebase(delta, file);
    }
}

impl Rebase for LambdaBody {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Expr(f0) => {
                f0.rebase(delta, file);
            }
            Self::Block { stmts, tail, .. } => {
                for x in &mut *stmts {
                    x.rebase(delta, file);
                }
                if let Some(x) = tail.as_mut() {
                    x.rebase(delta, file);
                }
            }
        }
    }
}

impl Rebase for RefArgExpr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.operand.rebase(delta, file);
    }
}

impl Rebase for StructLiteral {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.shape.rebase(delta, file);
    }
}

impl Rebase for FieldAccessExpr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.base.rebase(delta, file);
        self.field.rebase(delta, file);
    }
}

impl Rebase for ArrayLiteral {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        for x in &mut *self.elements {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for MapLiteral {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
    }
}

impl Rebase for InfixExpr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.lhs.rebase(delta, file);
        self.op.rebase(delta, file);
        self.rhs.rebase(delta, file);
    }
}

impl Rebase for IndexExpr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.base.rebase(delta, file);
        self.index.rebase(delta, file);
    }
}

impl Rebase for RangeExpr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.start.rebase(delta, file);
        self.end.rebase(delta, file);
    }
}

impl Rebase for StringExpr {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        for x in &mut *self.parts {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for StringPart {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        match self {
            Self::Interpolation(f0) => {
                f0.rebase(delta, file);
            }
            _ => {}
        }
    }
}

impl Rebase for PrefixOp {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for PostfixOp {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for InfixOp {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for VarDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.name.rebase(delta, file);
        self.value.rebase(delta, file);
        if let Some(x) = self.annotation.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ConstDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.name.rebase(delta, file);
        self.value.rebase(delta, file);
        if let Some(x) = self.annotation.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for TempDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.name.rebase(delta, file);
        if let Some(x) = self.value.as_mut() {
            x.rebase(delta, file);
        }
        if let Some(x) = self.annotation.as_mut() {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for Assignment {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.target.rebase(delta, file);
        self.op.rebase(delta, file);
        self.value.rebase(delta, file);
    }
}

impl Rebase for AssignOp {
    fn rebase(&mut self, _delta: TextSize, _file: FileId) {}
}

impl Rebase for ListDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.name.rebase(delta, file);
        for x in &mut *self.members {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for ListMember {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.name.rebase(delta, file);
    }
}

impl Rebase for StructDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.name.rebase(delta, file);
        for x in &mut *self.fields {
            x.rebase(delta, file);
        }
    }
}

impl Rebase for StructFieldDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.name.rebase(delta, file);
        self.ty.rebase(delta, file);
    }
}

impl Rebase for ExternalDecl {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
        self.name.rebase(delta, file);
    }
}

impl Rebase for IncludeSite {
    fn rebase(&mut self, delta: TextSize, file: FileId) {
        self.ptr.rebase(delta, file);
    }
}
