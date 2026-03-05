use brink_syntax::SyntaxKind;
use brink_syntax::ast::{self, AstNode, AstPtr};
use rowan::TextRange;

use crate::{
    AssignOp, Assignment, Block, BlockSequence, Choice, ChoiceSet, CondBranch, Conditional,
    ConstDecl, Content, ContentPart, DeclaredSymbol, Diagnostic, Divert, DivertPath, DivertTarget,
    Expr, ExternalDecl, FloatBits, Gather, HirFile, IncludeSite, InfixOp, InlineBranch, InlineCond,
    InlineSeq, Knot, ListDecl, ListMember, Name, Param, Path, PostfixOp, PrefixOp, RefKind, Return,
    SequenceType, Severity, Stitch, Stmt, StringExpr, StringPart, SymbolManifest, Tag, TempDecl,
    ThreadStart, TunnelCall, UnresolvedRef, VarDecl,
};

#[cfg(test)]
mod tests;

// ─── Public API ──────────────────────────────────────────────────────

pub fn lower(file: &ast::SourceFile) -> (HirFile, SymbolManifest, Vec<Diagnostic>) {
    let mut ctx = LowerCtx::new();
    let hir = ctx.lower_source_file(file);
    let LowerCtx {
        manifest,
        diagnostics,
    } = ctx;
    (hir, manifest, diagnostics)
}

/// Lower a single knot definition in isolation.
///
/// Returns `None` for the knot if the AST node is malformed (e.g. missing header).
pub fn lower_knot(knot: &ast::KnotDef) -> (Option<Knot>, SymbolManifest, Vec<Diagnostic>) {
    let mut ctx = LowerCtx::new();
    let result = ctx.lower_knot(knot);
    let LowerCtx {
        manifest,
        diagnostics,
    } = ctx;
    (result, manifest, diagnostics)
}

/// Lower only the top-level content and declarations of a file, skipping knots.
///
/// Useful for incremental analysis where knots are lowered separately.
pub fn lower_top_level(file: &ast::SourceFile) -> (Block, SymbolManifest, Vec<Diagnostic>) {
    let mut ctx = LowerCtx::new();

    // Lower declarations (registers symbols in manifest)
    let _variables: Vec<_> = file
        .var_decls()
        .filter_map(|v| ctx.lower_var_decl(&v))
        .collect();
    let _constants: Vec<_> = file
        .const_decls()
        .filter_map(|c| ctx.lower_const_decl(&c))
        .collect();
    let _lists: Vec<_> = file
        .list_decls()
        .filter_map(|l| ctx.lower_list_decl(&l))
        .collect();
    let _externals: Vec<_> = file
        .externals()
        .filter_map(|e| ctx.lower_external_decl(&e))
        .collect();

    let root_content = ctx.lower_body_children(file.syntax());

    let LowerCtx {
        manifest,
        diagnostics,
    } = ctx;
    (root_content, manifest, diagnostics)
}

// ─── Lowering context ────────────────────────────────────────────────

struct LowerCtx {
    diagnostics: Vec<Diagnostic>,
    manifest: SymbolManifest,
}

impl LowerCtx {
    fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
            manifest: SymbolManifest::default(),
        }
    }

    fn emit_error(&mut self, range: TextRange, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            range,
            message: message.into(),
            severity: Severity::Error,
        });
    }

    fn declare(&mut self, list: SymbolKind, name: &str, range: TextRange) {
        let sym = DeclaredSymbol {
            name: name.to_string(),
            range,
        };
        match list {
            SymbolKind::Knot => self.manifest.knots.push(sym),
            SymbolKind::Stitch => self.manifest.stitches.push(sym),
            SymbolKind::Variable => self.manifest.variables.push(sym),
            SymbolKind::List => self.manifest.lists.push(sym),
            SymbolKind::External => self.manifest.externals.push(sym),
        }
    }

    fn add_unresolved(&mut self, path: &str, range: TextRange, kind: RefKind) {
        self.manifest.unresolved.push(UnresolvedRef {
            path: path.to_string(),
            range,
            kind,
        });
    }
}

#[derive(Clone, Copy)]
enum SymbolKind {
    Knot,
    Stitch,
    Variable,
    List,
    External,
}

// ─── Helpers ─────────────────────────────────────────────────────────

fn make_name(text: impl Into<String>, range: TextRange) -> Name {
    Name {
        text: text.into(),
        range,
    }
}

fn name_from_ident(ident: &ast::Identifier) -> Option<Name> {
    let text = ident.name()?;
    Some(make_name(text, ident.syntax().text_range()))
}

fn path_full_name(path: &Path) -> String {
    path.segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

// ─── Phase 1: Top-level / containers ─────────────────────────────────

impl LowerCtx {
    fn lower_source_file(&mut self, file: &ast::SourceFile) -> HirFile {
        let variables: Vec<_> = file
            .var_decls()
            .filter_map(|v| self.lower_var_decl(&v))
            .collect();
        let constants: Vec<_> = file
            .const_decls()
            .filter_map(|c| self.lower_const_decl(&c))
            .collect();
        let lists: Vec<_> = file
            .list_decls()
            .filter_map(|l| self.lower_list_decl(&l))
            .collect();
        let externals: Vec<_> = file
            .externals()
            .filter_map(|e| self.lower_external_decl(&e))
            .collect();
        let includes: Vec<_> = file.includes().filter_map(|i| lower_include(&i)).collect();
        let knots: Vec<_> = file.knots().filter_map(|k| self.lower_knot(&k)).collect();
        let root_content = self.lower_body_children(file.syntax());

        HirFile {
            root_content,
            knots,
            variables,
            constants,
            lists,
            externals,
            includes,
        }
    }

    fn lower_knot(&mut self, knot: &ast::KnotDef) -> Option<Knot> {
        let header = knot.header()?;
        let name_text = header.name()?;
        let ident = header.identifier()?;
        let name = make_name(name_text.clone(), ident.syntax().text_range());

        self.declare(SymbolKind::Knot, &name_text, ident.syntax().text_range());

        let is_function = header.is_function();
        let params = lower_knot_params(header.params());

        let (body, stitches) = knot.body().map_or_else(
            || (Block::default(), Vec::new()),
            |b| self.lower_knot_body(&b, &name_text),
        );

        Some(Knot {
            ptr: AstPtr::new(knot),
            name,
            is_function,
            params,
            body,
            stitches,
        })
    }

    fn lower_knot_body(&mut self, body: &ast::KnotBody, knot_name: &str) -> (Block, Vec<Stitch>) {
        let stitches: Vec<_> = body
            .stitches()
            .filter_map(|s| self.lower_stitch(&s, knot_name))
            .collect();
        let block = self.lower_body_children(body.syntax());
        (block, stitches)
    }

    fn lower_stitch(&mut self, stitch: &ast::StitchDef, knot_name: &str) -> Option<Stitch> {
        let header = stitch.header()?;
        let name_text = header.name()?;
        let ident = header.identifier()?;
        let name = make_name(name_text.clone(), ident.syntax().text_range());

        let qualified = format!("{knot_name}.{name_text}");
        self.declare(SymbolKind::Stitch, &qualified, ident.syntax().text_range());

        let params = lower_knot_params(header.params());
        let body = stitch
            .body()
            .map_or_else(Block::default, |b| self.lower_body_children(b.syntax()));

        Some(Stitch {
            ptr: AstPtr::new(stitch),
            name,
            params,
            body,
        })
    }
}

fn lower_knot_params(params: Option<ast::KnotParams>) -> Vec<Param> {
    params
        .map(|p| p.params().filter_map(|pd| lower_param(&pd)).collect())
        .unwrap_or_default()
}

fn lower_param(p: &ast::KnotParamDecl) -> Option<Param> {
    let name = name_from_ident(&p.identifier()?)?;
    Some(Param {
        name,
        is_ref: p.is_ref(),
        is_divert: p.is_divert(),
    })
}

// ─── Phase 2: Declarations ──────────────────────────────────────────

impl LowerCtx {
    fn lower_var_decl(&mut self, v: &ast::VarDecl) -> Option<VarDecl> {
        let ident = v.identifier()?;
        let name = name_from_ident(&ident)?;
        self.declare(SymbolKind::Variable, &name.text, name.range);

        let value = v
            .value()
            .and_then(|e| self.lower_expr(&e))
            .unwrap_or_else(|| {
                self.emit_error(
                    v.syntax().text_range(),
                    "VAR declaration missing initializer",
                );
                Expr::Null
            });

        Some(VarDecl { name, value })
    }

    fn lower_const_decl(&mut self, c: &ast::ConstDecl) -> Option<ConstDecl> {
        let ident = c.identifier()?;
        let name = name_from_ident(&ident)?;
        self.declare(SymbolKind::Variable, &name.text, name.range);

        let value = c
            .value()
            .and_then(|e| self.lower_expr(&e))
            .unwrap_or_else(|| {
                self.emit_error(
                    c.syntax().text_range(),
                    "CONST declaration missing initializer",
                );
                Expr::Null
            });

        Some(ConstDecl { name, value })
    }

    fn lower_list_decl(&mut self, l: &ast::ListDecl) -> Option<ListDecl> {
        let ident = l.identifier()?;
        let name = name_from_ident(&ident)?;
        self.declare(SymbolKind::List, &name.text, name.range);

        let members = l
            .definition()
            .map(|def| {
                def.members()
                    .filter_map(|m| lower_list_member(&m))
                    .collect()
            })
            .unwrap_or_default();

        Some(ListDecl { name, members })
    }

    fn lower_external_decl(&mut self, e: &ast::ExternalDecl) -> Option<ExternalDecl> {
        let ident = e.identifier()?;
        let name = name_from_ident(&ident)?;
        self.declare(SymbolKind::External, &name.text, name.range);

        #[expect(
            clippy::cast_possible_truncation,
            reason = "external params won't exceed 255"
        )]
        let param_count = e.param_list().map_or(0, |pl| pl.params().count() as u8);

        Some(ExternalDecl { name, param_count })
    }
}

fn lower_list_member(m: &ast::ListMember) -> Option<ListMember> {
    let range = m.syntax().text_range();
    if let Some(on) = m.on_member() {
        let name_text = on.name()?;
        #[expect(clippy::cast_possible_truncation, reason = "list values fit in i32")]
        Some(ListMember {
            name: make_name(name_text, range),
            value: on.value().map(|v| v as i32),
            is_active: true,
        })
    } else if let Some(off) = m.off_member() {
        let name_text = off.name()?;
        #[expect(clippy::cast_possible_truncation, reason = "list values fit in i32")]
        Some(ListMember {
            name: make_name(name_text, range),
            value: off.value().map(|v| v as i32),
            is_active: false,
        })
    } else {
        None
    }
}

fn lower_include(inc: &ast::IncludeStmt) -> Option<IncludeSite> {
    Some(IncludeSite {
        file_path: inc.file_path()?.text(),
        ptr: AstPtr::new(inc),
    })
}

// ─── Phase 3: Expression lowering ───────────────────────────────────

impl LowerCtx {
    fn lower_expr(&mut self, expr: &ast::Expr) -> Option<Expr> {
        match expr {
            ast::Expr::IntegerLit(lit) =>
            {
                #[expect(clippy::cast_possible_truncation, reason = "ink integers are 32-bit")]
                Some(Expr::Int(lit.value()? as i32))
            }
            ast::Expr::FloatLit(lit) => Some(Expr::Float(FloatBits::from_f64(lit.value()?))),
            ast::Expr::BooleanLit(lit) => Some(Expr::Bool(lit.value()?)),
            ast::Expr::StringLit(lit) => Some(Expr::String(self.lower_string_lit(lit))),
            ast::Expr::Path(path) => {
                let p = lower_path(path);
                let full = path_full_name(&p);
                self.add_unresolved(&full, path.syntax().text_range(), RefKind::Variable);
                Some(Expr::Path(p))
            }
            ast::Expr::Prefix(pe) => {
                let op = lower_prefix_op(pe)?;
                let operand = pe.operand().and_then(|e| self.lower_expr(&e))?;
                Some(Expr::Prefix(op, Box::new(operand)))
            }
            ast::Expr::Infix(ie) => {
                let lhs = ie.lhs().and_then(|e| self.lower_expr(&e))?;
                let op = lower_infix_op(ie)?;
                let rhs = ie.rhs().and_then(|e| self.lower_expr(&e))?;
                Some(Expr::Infix(Box::new(lhs), op, Box::new(rhs)))
            }
            ast::Expr::Postfix(pe) => {
                let operand = pe.operand().and_then(|e| self.lower_expr(&e))?;
                let op = lower_postfix_op(pe)?;
                Some(Expr::Postfix(Box::new(operand), op))
            }
            ast::Expr::Paren(pe) => pe.inner().and_then(|e| self.lower_expr(&e)),
            ast::Expr::FunctionCall(fc) => {
                let ident = fc.identifier()?;
                let name_text = ident.name()?;
                let range = ident.syntax().text_range();
                let path = Path {
                    segments: vec![make_name(name_text.clone(), range)],
                    range,
                };
                self.add_unresolved(&name_text, range, RefKind::Function);
                let args: Vec<Expr> = fc
                    .arg_list()
                    .map(|al| al.args().filter_map(|a| self.lower_expr(&a)).collect())
                    .unwrap_or_default();
                Some(Expr::Call(path, args))
            }
            ast::Expr::DivertTarget(dt) => {
                let ast_path = dt.target()?;
                let path = lower_path(&ast_path);
                let full = path_full_name(&path);
                self.add_unresolved(&full, ast_path.syntax().text_range(), RefKind::Divert);
                Some(Expr::DivertTarget(path))
            }
            ast::Expr::ListExpr(le) => {
                let items: Vec<Path> = le.items().map(|p| lower_path(&p)).collect();
                for item in &items {
                    let full = path_full_name(item);
                    self.add_unresolved(&full, item.range, RefKind::List);
                }
                Some(Expr::ListLiteral(items))
            }
        }
    }

    fn lower_string_lit(&mut self, lit: &ast::StringLit) -> StringExpr {
        let mut parts = Vec::new();
        for child in lit.syntax().children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(tok) if tok.kind() != SyntaxKind::QUOTE => {
                    let text = tok.text().to_string();
                    if !text.is_empty() {
                        parts.push(StringPart::Literal(text));
                    }
                }
                rowan::NodeOrToken::Node(node) => {
                    if let Some(inline) = ast::InlineLogic::cast(node)
                        && let Some(expr) = inline
                            .inner_expression()
                            .and_then(|inner| inner.expr())
                            .and_then(|e| self.lower_expr(&e))
                    {
                        parts.push(StringPart::Interpolation(Box::new(expr)));
                    }
                }
                rowan::NodeOrToken::Token(_) => {}
            }
        }
        StringExpr { parts }
    }
}

fn lower_path(path: &ast::Path) -> Path {
    let segments: Vec<Name> = path
        .segments()
        .map(|tok| make_name(tok.text().to_string(), tok.text_range()))
        .collect();
    Path {
        segments,
        range: path.syntax().text_range(),
    }
}

fn lower_prefix_op(pe: &ast::PrefixExpr) -> Option<PrefixOp> {
    let tok = pe.op_token()?;
    match tok.kind() {
        SyntaxKind::MINUS => Some(PrefixOp::Negate),
        SyntaxKind::BANG | SyntaxKind::KW_NOT => Some(PrefixOp::Not),
        _ => None,
    }
}

fn lower_infix_op(ie: &ast::InfixExpr) -> Option<InfixOp> {
    let tok = ie.op_token()?;
    match tok.kind() {
        SyntaxKind::PLUS => Some(InfixOp::Add),
        SyntaxKind::MINUS => Some(InfixOp::Sub),
        SyntaxKind::STAR => Some(InfixOp::Mul),
        SyntaxKind::SLASH => Some(InfixOp::Div),
        SyntaxKind::PERCENT | SyntaxKind::KW_MOD => Some(InfixOp::Mod),
        SyntaxKind::CARET => Some(InfixOp::Pow),
        SyntaxKind::EQ_EQ => Some(InfixOp::Eq),
        SyntaxKind::BANG_EQ => Some(InfixOp::NotEq),
        SyntaxKind::LT => Some(InfixOp::Lt),
        SyntaxKind::GT => Some(InfixOp::Gt),
        SyntaxKind::LT_EQ => Some(InfixOp::LtEq),
        SyntaxKind::GT_EQ => Some(InfixOp::GtEq),
        SyntaxKind::KW_AND | SyntaxKind::AMP_AMP => Some(InfixOp::And),
        SyntaxKind::KW_OR | SyntaxKind::PIPE => Some(InfixOp::Or),
        SyntaxKind::KW_HAS | SyntaxKind::QUESTION => Some(InfixOp::Has),
        SyntaxKind::KW_HASNT | SyntaxKind::BANG_QUESTION => Some(InfixOp::HasNot),
        _ => None,
    }
}

fn lower_postfix_op(pe: &ast::PostfixExpr) -> Option<PostfixOp> {
    let tok = pe.op_token()?;
    match tok.kind() {
        SyntaxKind::PLUS => Some(PostfixOp::Increment),
        SyntaxKind::MINUS => Some(PostfixOp::Decrement),
        _ => None,
    }
}

// ─── Phase 4: Content lowering ──────────────────────────────────────

impl LowerCtx {
    fn lower_content_line(&mut self, line: &ast::ContentLine) -> Option<Stmt> {
        let parts = line
            .mixed_content()
            .map_or_else(Vec::new, |mc| self.lower_mixed_content(&mc));
        let tags = lower_tags(line.tags());

        // If this line has only a divert (no content), emit a divert statement
        if parts.is_empty() && tags.is_empty() {
            return line.divert().and_then(|d| self.lower_divert_node(&d));
        }

        Some(Stmt::Content(Content { parts, tags }))
    }

    fn lower_mixed_content(&mut self, mc: &ast::MixedContent) -> Vec<ContentPart> {
        self.lower_content_node_children(mc.syntax())
    }

    fn lower_inline_logic(&mut self, inline: &ast::InlineLogic, parts: &mut Vec<ContentPart>) {
        if let Some(inner) = inline.inner_expression() {
            if let Some(expr) = inner.expr().and_then(|e| self.lower_expr(&e)) {
                parts.push(ContentPart::Interpolation(expr));
            }
            return;
        }

        if let Some(cond) = inline.conditional() {
            if let Some(ic) = self.lower_inline_conditional(&cond) {
                parts.push(ContentPart::InlineConditional(ic));
            }
            return;
        }

        if let Some(seq) = inline.sequence() {
            if let Some(is) = self.lower_inline_sequence(&seq) {
                parts.push(ContentPart::InlineSequence(is));
            }
            return;
        }

        if let Some(imp) = inline.implicit_sequence() {
            let branches: Vec<Vec<ContentPart>> = imp
                .branches()
                .map(|b| self.lower_content_node_children(b.syntax()))
                .collect();
            parts.push(ContentPart::InlineSequence(InlineSeq {
                kind: SequenceType::STOPPING,
                branches,
            }));
        }
    }

    fn lower_inline_conditional(&mut self, cond: &ast::ConditionalWithExpr) -> Option<InlineCond> {
        let condition = cond.condition().and_then(|e| self.lower_expr(&e))?;
        let mut branches = Vec::new();

        if let Some(body) = cond.branchless_body() {
            let content = self.lower_content_node_children(body.syntax());
            branches.push(InlineBranch {
                condition: Some(condition.clone()),
                content,
            });
            if let Some(else_branch) = body.else_branch()
                && let Some(branch_content) = else_branch.branch()
            {
                branches.push(InlineBranch {
                    condition: None,
                    content: self.lower_content_node_children(branch_content.syntax()),
                });
            }
            return Some(InlineCond { branches });
        }

        if let Some(inline_branches) = cond.inline_branches() {
            let mut first = true;
            for b in inline_branches.branches() {
                let content = self.lower_content_node_children(b.syntax());
                let cond_expr = if first {
                    first = false;
                    Some(condition.clone())
                } else {
                    None
                };
                branches.push(InlineBranch {
                    condition: cond_expr,
                    content,
                });
            }
            return Some(InlineCond { branches });
        }

        if let Some(ml_branches) = cond.multiline_branches() {
            branches.push(InlineBranch {
                condition: Some(condition),
                content: Vec::new(),
            });
            for b in ml_branches.branches() {
                let cond_expr = if b.is_else() {
                    None
                } else {
                    b.condition().and_then(|e| self.lower_expr(&e))
                };
                let content = b.body().map_or_else(Vec::new, |body| {
                    self.lower_content_node_children(body.syntax())
                });
                branches.push(InlineBranch {
                    condition: cond_expr,
                    content,
                });
            }
            return Some(InlineCond { branches });
        }

        None
    }

    fn lower_inline_sequence(&mut self, seq: &ast::SequenceWithAnnotation) -> Option<InlineSeq> {
        let kind = lower_sequence_type(seq);

        let branches = if let Some(inline_branches) = seq.inline_branches() {
            inline_branches
                .branches()
                .map(|b| self.lower_content_node_children(b.syntax()))
                .collect()
        } else if let Some(ml_branches) = seq.multiline_branches() {
            ml_branches
                .branches()
                .map(|b| {
                    b.body().map_or_else(Vec::new, |body| {
                        self.lower_content_node_children(body.syntax())
                    })
                })
                .collect()
        } else {
            return None;
        };

        Some(InlineSeq { kind, branches })
    }

    fn lower_content_node_children(&mut self, node: &brink_syntax::SyntaxNode) -> Vec<ContentPart> {
        let mut parts = Vec::new();
        for child in node.children_with_tokens() {
            if let rowan::NodeOrToken::Node(child_node) = child {
                match child_node.kind() {
                    SyntaxKind::TEXT => {
                        let text = child_node.text().to_string();
                        if !text.is_empty() {
                            parts.push(ContentPart::Text(text));
                        }
                    }
                    SyntaxKind::GLUE_NODE => parts.push(ContentPart::Glue),
                    SyntaxKind::ESCAPE => {
                        let text = child_node.text().to_string();
                        if text.len() > 1 {
                            parts.push(ContentPart::Text(text[1..].to_string()));
                        }
                    }
                    SyntaxKind::INLINE_LOGIC => {
                        if let Some(inline) = ast::InlineLogic::cast(child_node) {
                            self.lower_inline_logic(&inline, &mut parts);
                        }
                    }
                    _ => {}
                }
            }
        }
        parts
    }
}

fn lower_sequence_type(seq: &ast::SequenceWithAnnotation) -> SequenceType {
    let mut kind = SequenceType::empty();

    if let Some(sym) = seq.symbol_annotation() {
        if sym.amp_token().is_some() {
            kind |= SequenceType::CYCLE;
        }
        if sym.bang_token().is_some() {
            kind |= SequenceType::ONCE;
        }
        if sym.tilde_token().is_some() {
            kind |= SequenceType::SHUFFLE;
        }
        if sym.dollar_token().is_some() {
            kind |= SequenceType::STOPPING;
        }
    }

    if let Some(word) = seq.word_annotation() {
        if word.stopping_kw().is_some() {
            kind |= SequenceType::STOPPING;
        }
        if word.cycle_kw().is_some() {
            kind |= SequenceType::CYCLE;
        }
        if word.shuffle_kw().is_some() {
            kind |= SequenceType::SHUFFLE;
        }
        if word.once_kw().is_some() {
            kind |= SequenceType::ONCE;
        }
    }

    if kind.is_empty() {
        SequenceType::STOPPING
    } else {
        kind
    }
}

fn lower_tags(tags: Option<ast::Tags>) -> Vec<Tag> {
    tags.map_or_else(Vec::new, |t| {
        t.tags()
            .map(|tag| Tag {
                text: tag.text(),
                ptr: AstPtr::new(&tag),
            })
            .collect()
    })
}

// ─── Phase 5: Control flow ──────────────────────────────────────────

impl LowerCtx {
    fn lower_divert_node(&mut self, node: &ast::DivertNode) -> Option<Stmt> {
        if let Some(thread) = node.thread_start() {
            return Some(Stmt::ThreadStart(self.lower_thread_target(&thread)?));
        }

        if let Some(tunnel) = node.tunnel_call() {
            let targets: Vec<DivertTarget> = tunnel
                .targets()
                .filter_map(|t| self.lower_divert_target_with_args(&t))
                .collect();
            if !targets.is_empty() {
                return Some(Stmt::TunnelCall(TunnelCall { targets }));
            }
            return None;
        }

        if let Some(tunnel_onwards) = node.tunnel_onwards() {
            let mut targets: Vec<DivertTarget> = tunnel_onwards
                .targets()
                .filter_map(|t| self.lower_divert_target_with_args(&t))
                .collect();
            if let Some(tc) = tunnel_onwards.tunnel_call() {
                targets.extend(
                    tc.targets()
                        .filter_map(|t| self.lower_divert_target_with_args(&t)),
                );
            }
            if !targets.is_empty() {
                return Some(Stmt::TunnelCall(TunnelCall { targets }));
            }
            return None;
        }

        if let Some(simple) = node.simple_divert() {
            let targets: Vec<DivertTarget> = simple
                .targets()
                .filter_map(|t| self.lower_divert_target_with_args(&t))
                .collect();
            return match targets.len() {
                1 => Some(Stmt::Divert(Divert {
                    target: targets.into_iter().next()?,
                })),
                n if n > 1 => Some(Stmt::TunnelCall(TunnelCall { targets })),
                _ => None,
            };
        }

        None
    }

    fn lower_thread_target(&mut self, thread: &ast::ThreadStart) -> Option<ThreadStart> {
        let ast_path = thread.target()?;
        let path = lower_path(&ast_path);
        let full = path_full_name(&path);
        self.add_unresolved(&full, ast_path.syntax().text_range(), RefKind::Divert);

        let args: Vec<Expr> = thread
            .arg_list()
            .map(|al| al.args().filter_map(|a| self.lower_expr(&a)).collect())
            .unwrap_or_default();

        Some(ThreadStart {
            target: DivertTarget {
                path: DivertPath::Path(path),
                args,
            },
        })
    }

    fn lower_divert_target_with_args(
        &mut self,
        t: &ast::DivertTargetWithArgs,
    ) -> Option<DivertTarget> {
        let path = self.lower_divert_path(t)?;
        let args: Vec<Expr> = t
            .arg_list()
            .map(|al| al.args().filter_map(|a| self.lower_expr(&a)).collect())
            .unwrap_or_default();
        Some(DivertTarget { path, args })
    }

    fn lower_divert_path(&mut self, t: &ast::DivertTargetWithArgs) -> Option<DivertPath> {
        if t.done_kw().is_some() {
            return Some(DivertPath::Done);
        }
        if t.end_kw().is_some() {
            return Some(DivertPath::End);
        }
        let ast_path = t.path()?;
        let path = lower_path(&ast_path);
        let full = path_full_name(&path);
        self.add_unresolved(&full, ast_path.syntax().text_range(), RefKind::Divert);
        Some(DivertPath::Path(path))
    }

    fn lower_logic_line(&mut self, line: &ast::LogicLine) -> Option<Stmt> {
        if let Some(ret) = line.return_stmt() {
            return Some(Stmt::Return(Return {
                value: ret.value().and_then(|e| self.lower_expr(&e)),
            }));
        }

        if let Some(temp) = line.temp_decl() {
            let name = name_from_ident(&temp.identifier()?)?;
            let value = temp.value().and_then(|e| self.lower_expr(&e));
            return Some(Stmt::TempDecl(TempDecl { name, value }));
        }

        if let Some(assign) = line.assignment() {
            let target = assign.target().and_then(|e| self.lower_expr(&e))?;
            let value = assign.value().and_then(|e| self.lower_expr(&e))?;
            let op = assign
                .op_token()
                .map_or(AssignOp::Set, |tok| match tok.kind() {
                    SyntaxKind::PLUS_EQ => AssignOp::Add,
                    SyntaxKind::MINUS_EQ => AssignOp::Sub,
                    _ => AssignOp::Set,
                });
            return Some(Stmt::Assignment(Assignment { target, op, value }));
        }

        // Bare expression statement: ~ expr
        for child in line.syntax().children() {
            if let Some(expr) = ast::Expr::cast(child)
                && let Some(e) = self.lower_expr(&expr)
            {
                return Some(Stmt::ExprStmt(e));
            }
        }

        None
    }
}

// ─── Phase 6: Choice and gather lowering ────────────────────────────

impl LowerCtx {
    fn lower_choice(&mut self, choice: &ast::Choice) -> Option<Choice> {
        let bullets = choice.bullets()?;
        let is_sticky = bullets.is_sticky();

        let label = choice
            .label()
            .and_then(|l| name_from_ident(&l.identifier()?));

        let is_fallback = choice.start_content().is_none()
            && choice.bracket_content().is_none()
            && choice.inner_content().is_none();

        let condition = choice
            .conditions()
            .next()
            .and_then(|c| c.expr())
            .and_then(|e| self.lower_expr(&e));

        let start_content = choice.start_content().map(|sc| Content {
            parts: self.lower_content_node_children(sc.syntax()),
            tags: Vec::new(),
        });

        let bracket_content = choice.bracket_content().map(|bc| Content {
            parts: self.lower_content_node_children(bc.syntax()),
            tags: Vec::new(),
        });

        let inner_content = choice.inner_content().map(|ic| Content {
            parts: self.lower_content_node_children(ic.syntax()),
            tags: Vec::new(),
        });

        let divert = choice.divert().and_then(|d| {
            let target = d
                .simple_divert()?
                .targets()
                .next()
                .and_then(|t| self.lower_divert_target_with_args(&t))?;
            Some(Divert { target })
        });

        let tags = lower_tags(choice.tags());
        let body = self.lower_choice_body(choice);

        Some(Choice {
            ptr: AstPtr::new(choice),
            is_sticky,
            is_fallback,
            label,
            condition,
            start_content,
            bracket_content,
            inner_content,
            divert,
            tags,
            body,
        })
    }

    fn lower_choice_body(&mut self, choice: &ast::Choice) -> Block {
        let mut stmts = Vec::new();
        for child in choice.syntax().children() {
            self.lower_body_child(child, &mut stmts);
        }
        Block { stmts }
    }

    fn lower_body_child(&mut self, child: brink_syntax::SyntaxNode, out: &mut Vec<Stmt>) {
        match child.kind() {
            SyntaxKind::CONTENT_LINE => {
                if let Some(cl) = ast::ContentLine::cast(child) {
                    let stmt = self.lower_content_line(&cl);
                    let was_content = matches!(&stmt, Some(Stmt::Content(_)));
                    if let Some(s) = stmt {
                        out.push(s);
                    }
                    if was_content
                        && let Some(dn) = cl.divert()
                        && let Some(s) = self.lower_divert_node(&dn)
                    {
                        out.push(s);
                    }
                }
            }
            SyntaxKind::LOGIC_LINE => {
                if let Some(ll) = ast::LogicLine::cast(child)
                    && let Some(stmt) = self.lower_logic_line(&ll)
                {
                    out.push(stmt);
                }
            }
            SyntaxKind::DIVERT_NODE => {
                if let Some(dn) = ast::DivertNode::cast(child)
                    && let Some(stmt) = self.lower_divert_node(&dn)
                {
                    out.push(stmt);
                }
            }
            SyntaxKind::INLINE_LOGIC => {
                if let Some(il) = ast::InlineLogic::cast(child)
                    && let Some(stmt) = self.lower_multiline_block_from_inline(&il)
                {
                    out.push(stmt);
                }
            }
            _ => {}
        }
    }

    fn lower_gather(&mut self, gather: &ast::Gather) -> Gather {
        let label = gather
            .label()
            .and_then(|l| name_from_ident(&l.identifier()?));

        let content = gather.mixed_content().map(|mc| Content {
            parts: self.lower_mixed_content(&mc),
            tags: Vec::new(),
        });

        let divert = gather.divert().and_then(|d| {
            let target = d
                .simple_divert()?
                .targets()
                .next()
                .and_then(|t| self.lower_divert_target_with_args(&t))?;
            Some(Divert { target })
        });

        let tags = lower_tags(gather.tags());

        Gather {
            ptr: AstPtr::new(gather),
            label,
            content,
            divert,
            tags,
            body: Block::default(),
        }
    }
}

// ─── Phase 7: Body assembly and weave folding ───────────────────────

pub enum WeaveItem {
    Choice { choice: Choice },
    Gather { gather: Gather },
    Stmt(Stmt),
}

impl LowerCtx {
    fn lower_body_children(&mut self, parent: &brink_syntax::SyntaxNode) -> Block {
        let mut items = Vec::new();

        for child in parent.children() {
            match child.kind() {
                SyntaxKind::CONTENT_LINE => {
                    if let Some(cl) = ast::ContentLine::cast(child.clone()) {
                        let stmt = self.lower_content_line(&cl);
                        let was_content = matches!(&stmt, Some(Stmt::Content(_)));
                        if let Some(s) = stmt {
                            items.push(WeaveItem::Stmt(s));
                        }
                        if was_content
                            && let Some(dn) = cl.divert()
                            && let Some(s) = self.lower_divert_node(&dn)
                        {
                            items.push(WeaveItem::Stmt(s));
                        }
                    }
                }
                SyntaxKind::LOGIC_LINE => {
                    if let Some(ll) = ast::LogicLine::cast(child)
                        && let Some(stmt) = self.lower_logic_line(&ll)
                    {
                        items.push(WeaveItem::Stmt(stmt));
                    }
                }
                SyntaxKind::TAG_LINE => {
                    if let Some(tl) = ast::TagLine::cast(child) {
                        let tags = lower_tags(tl.tags());
                        if !tags.is_empty() {
                            items.push(WeaveItem::Stmt(Stmt::Content(Content {
                                parts: Vec::new(),
                                tags,
                            })));
                        }
                    }
                }
                SyntaxKind::CHOICE => {
                    if let Some(c) = ast::Choice::cast(child)
                        && let Some(choice) = self.lower_choice(&c)
                    {
                        items.push(WeaveItem::Choice { choice });
                    }
                }
                SyntaxKind::GATHER => {
                    if let Some(g) = ast::Gather::cast(child) {
                        items.push(WeaveItem::Gather {
                            gather: self.lower_gather(&g),
                        });
                    }
                }
                SyntaxKind::INLINE_LOGIC => {
                    if let Some(il) = ast::InlineLogic::cast(child)
                        && let Some(stmt) = self.lower_multiline_block_from_inline(&il)
                    {
                        items.push(WeaveItem::Stmt(stmt));
                    }
                }
                SyntaxKind::DIVERT_NODE => {
                    if let Some(dn) = ast::DivertNode::cast(child)
                        && let Some(stmt) = self.lower_divert_node(&dn)
                    {
                        items.push(WeaveItem::Stmt(stmt));
                    }
                }
                _ => {}
            }
        }

        fold_weave(items)
    }

    fn lower_multiline_block_from_inline(&mut self, inline: &ast::InlineLogic) -> Option<Stmt> {
        if let Some(ml_cond) = inline.multiline_conditional() {
            return Some(Stmt::Conditional(
                self.lower_multiline_conditional(&ml_cond),
            ));
        }

        if let Some(cond) = inline.conditional()
            && cond.multiline_branches().is_some()
        {
            return Some(Stmt::Conditional(self.lower_block_conditional(&cond)));
        }

        if let Some(seq) = inline.sequence()
            && seq.multiline_branches().is_some()
        {
            return Some(Stmt::Sequence(self.lower_block_sequence(&seq)));
        }

        None
    }

    fn lower_multiline_conditional(&mut self, mc: &ast::MultilineConditional) -> Conditional {
        let branches = mc
            .branches()
            .map(|b| {
                let condition = if b.is_else() {
                    None
                } else {
                    b.condition().and_then(|e| self.lower_expr(&e))
                };
                let body = b.body().map_or_else(Block::default, |body| {
                    self.lower_body_children(body.syntax())
                });
                CondBranch { condition, body }
            })
            .collect();
        Conditional { branches }
    }

    fn lower_block_conditional(&mut self, cond: &ast::ConditionalWithExpr) -> Conditional {
        let outer_cond = cond.condition().and_then(|e| self.lower_expr(&e));
        let mut branches = Vec::new();

        if let Some(ml) = cond.multiline_branches() {
            for b in ml.branches() {
                let condition = if b.is_else() {
                    None
                } else {
                    b.condition().and_then(|e| self.lower_expr(&e))
                };
                let body = b.body().map_or_else(Block::default, |body| {
                    self.lower_body_children(body.syntax())
                });
                branches.push(CondBranch { condition, body });
            }
        }

        if branches.is_empty()
            && let Some(c) = outer_cond
        {
            branches.push(CondBranch {
                condition: Some(c),
                body: Block::default(),
            });
        }

        Conditional { branches }
    }

    fn lower_block_sequence(&mut self, seq: &ast::SequenceWithAnnotation) -> BlockSequence {
        let kind = lower_sequence_type(seq);
        let branches = seq.multiline_branches().map_or_else(Vec::new, |ml| {
            ml.branches()
                .map(|b| {
                    b.body().map_or_else(Block::default, |body| {
                        self.lower_body_children(body.syntax())
                    })
                })
                .collect()
        });
        BlockSequence { kind, branches }
    }
}

// ─── Weave folding ──────────────────────────────────────────────────

pub fn fold_weave(items: Vec<WeaveItem>) -> Block {
    let mut stmts = Vec::new();
    let mut choice_acc: Vec<Choice> = Vec::new();
    let mut pending_gather: Option<Gather> = None;

    for item in items {
        match item {
            WeaveItem::Stmt(stmt) => {
                if let Some(ref mut gather) = pending_gather {
                    gather.body.stmts.push(stmt);
                } else {
                    stmts.push(stmt);
                }
            }
            WeaveItem::Choice { choice } => {
                if pending_gather.is_some() {
                    emit_choice_set(&mut stmts, &mut choice_acc, &mut pending_gather);
                }
                choice_acc.push(choice);
            }
            WeaveItem::Gather { gather } => {
                if choice_acc.is_empty() && pending_gather.is_none() {
                    // Standalone gather
                    emit_standalone_gather(&mut stmts, gather);
                } else if pending_gather.is_some() {
                    emit_choice_set(&mut stmts, &mut choice_acc, &mut pending_gather);
                    emit_standalone_gather(&mut stmts, gather);
                } else {
                    pending_gather = Some(gather);
                }
            }
        }
    }

    emit_choice_set(&mut stmts, &mut choice_acc, &mut pending_gather);
    Block { stmts }
}

fn emit_choice_set(
    stmts: &mut Vec<Stmt>,
    choice_acc: &mut Vec<Choice>,
    pending_gather: &mut Option<Gather>,
) {
    if choice_acc.is_empty() && pending_gather.is_none() {
        return;
    }

    if !choice_acc.is_empty() {
        let choices = std::mem::take(choice_acc);
        let gather = pending_gather.take();
        stmts.push(Stmt::ChoiceSet(ChoiceSet { choices, gather }));
    } else if let Some(g) = pending_gather.take() {
        emit_standalone_gather(stmts, g);
    }
}

fn emit_standalone_gather(stmts: &mut Vec<Stmt>, gather: Gather) {
    if let Some(content) = gather.content
        && (!content.parts.is_empty() || !gather.tags.is_empty())
    {
        stmts.push(Stmt::Content(Content {
            parts: content.parts,
            tags: gather.tags,
        }));
    }
    stmts.extend(gather.body.stmts);
}
