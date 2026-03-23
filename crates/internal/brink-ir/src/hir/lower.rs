use crate::symbols::LocalSymbol;
use brink_syntax::SyntaxKind;
use brink_syntax::ast::{self, AstNode, AstPtr, SyntaxNodePtr};
use rowan::TextRange;

use crate::{
    AssignOp, Assignment, Block, Choice, ChoiceSet, ChoiceSetContext, CondBranch, CondKind,
    Conditional, ConstDecl, ContainerPtr, Content, ContentPart, DeclaredSymbol, Diagnostic,
    DiagnosticCode, Divert, DivertPath, DivertTarget, Expr, ExternalDecl, FileId, FloatBits,
    HirFile, IncludeSite, InfixOp, Knot, ListDecl, ListMember, Name, Param, Path, PostfixOp,
    PrefixOp, RefKind, Return, Scope, Sequence, SequenceType, Stitch, Stmt, StringExpr, StringPart,
    SymbolManifest, Tag, TempDecl, ThreadStart, TunnelCall, UnresolvedRef, VarDecl,
};

#[cfg(test)]
mod tests;

// ─── Public API ──────────────────────────────────────────────────────

pub fn lower(
    file_id: FileId,
    file: &ast::SourceFile,
) -> (HirFile, SymbolManifest, Vec<Diagnostic>) {
    let mut ctx = LowerCtx::new(file_id);
    let hir = ctx.lower_source_file(file);
    (hir, ctx.manifest, ctx.diagnostics)
}

/// Lower a single knot definition in isolation.
///
/// Returns `None` for the knot if the AST node is malformed (e.g. missing header).
pub fn lower_knot(
    file_id: FileId,
    knot: &ast::KnotDef,
) -> (Option<Knot>, SymbolManifest, Vec<Diagnostic>) {
    let mut ctx = LowerCtx::new(file_id);
    let result = ctx.lower_knot(knot);
    (result, ctx.manifest, ctx.diagnostics)
}

/// Lower only the top-level content and declarations of a file, skipping knots.
///
/// Useful for incremental analysis where knots are lowered separately.
pub fn lower_top_level(
    file_id: FileId,
    file: &ast::SourceFile,
) -> (Block, Vec<Knot>, SymbolManifest, Vec<Diagnostic>) {
    let mut ctx = LowerCtx::new(file_id);

    // Lower declarations (registers symbols in manifest).
    // Walk descendants — VAR/CONST/LIST are global regardless of nesting.
    let _variables: Vec<_> = file
        .syntax()
        .descendants()
        .filter_map(ast::VarDecl::cast)
        .filter_map(|v| ctx.lower_var_decl(&v))
        .collect();
    let _constants: Vec<_> = file
        .syntax()
        .descendants()
        .filter_map(ast::ConstDecl::cast)
        .filter_map(|c| ctx.lower_const_decl(&c))
        .collect();
    let _lists: Vec<_> = file
        .syntax()
        .descendants()
        .filter_map(ast::ListDecl::cast)
        .filter_map(|l| ctx.lower_list_decl(&l))
        .collect();
    let _externals: Vec<_> = file
        .externals()
        .filter_map(|e| ctx.lower_external_decl(&e))
        .collect();
    // Top-level stitches (no parent knot) — promoted to knots.
    let top_level_knots: Vec<_> = file
        .stitches()
        .filter_map(|stitch| ctx.lower_top_level_stitch(&stitch))
        .collect();

    let root_content = ctx.lower_body_children(file.syntax());

    (root_content, top_level_knots, ctx.manifest, ctx.diagnostics)
}

// ─── Lowering context ────────────────────────────────────────────────

struct LowerCtx {
    file_id: FileId,
    diagnostics: Vec<Diagnostic>,
    manifest: SymbolManifest,
    current_knot: Option<String>,
    current_stitch: Option<String>,
}

impl LowerCtx {
    fn new(file_id: FileId) -> Self {
        Self {
            file_id,
            diagnostics: Vec::new(),
            manifest: SymbolManifest::default(),
            current_knot: None,
            current_stitch: None,
        }
    }

    fn current_scope(&self) -> Scope {
        Scope {
            knot: self.current_knot.clone(),
            stitch: self.current_stitch.clone(),
        }
    }

    fn qualify_label(&self, label: &str) -> String {
        match (&self.current_knot, &self.current_stitch) {
            (Some(knot), Some(stitch)) => format!("{knot}.{stitch}.{label}"),
            (Some(knot), None) => format!("{knot}.{label}"),
            _ => label.to_string(),
        }
    }

    fn emit(&mut self, range: TextRange, code: DiagnosticCode) {
        self.diagnostics.push(Diagnostic {
            file: self.file_id,
            range,
            message: code.title().to_string(),
            code,
        });
    }

    fn declare(&mut self, list: SymbolKind, name: &str, range: TextRange) {
        self.declare_with(list, name, range, Vec::new(), None);
    }

    fn declare_with(
        &mut self,
        list: SymbolKind,
        name: &str,
        range: TextRange,
        params: Vec<crate::ParamInfo>,
        detail: Option<String>,
    ) {
        let sym = DeclaredSymbol {
            name: name.to_string(),
            range,
            params,
            detail,
        };
        match list {
            SymbolKind::Knot => self.manifest.knots.push(sym),
            SymbolKind::Stitch => self.manifest.stitches.push(sym),
            SymbolKind::Variable => self.manifest.variables.push(sym),
            SymbolKind::Constant => self.manifest.constants.push(sym),
            SymbolKind::List => self.manifest.lists.push(sym),
            SymbolKind::External => self.manifest.externals.push(sym),
            SymbolKind::Label => self.manifest.labels.push(sym),
            SymbolKind::ListItem => self.manifest.list_items.push(sym),
        }
    }

    fn add_unresolved(&mut self, path: &str, range: TextRange, kind: RefKind) {
        self.add_unresolved_with_args(path, range, kind, None);
    }

    fn add_unresolved_with_args(
        &mut self,
        path: &str,
        range: TextRange,
        kind: RefKind,
        arg_count: Option<usize>,
    ) {
        if path.is_empty() {
            return;
        }
        self.manifest.unresolved.push(UnresolvedRef {
            path: path.to_string(),
            range,
            kind,
            scope: self.current_scope(),
            arg_count,
        });
    }
}

#[derive(Clone, Copy)]
enum SymbolKind {
    Knot,
    Stitch,
    Variable,
    Constant,
    List,
    External,
    Label,
    ListItem,
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
        // In ink, VAR/CONST/LIST are always global regardless of where they
        // appear (even inside knot/stitch bodies). Walk the entire tree to
        // collect them all, matching the reference compiler's hoisting.
        let variables: Vec<_> = file
            .syntax()
            .descendants()
            .filter_map(ast::VarDecl::cast)
            .filter_map(|v| self.lower_var_decl(&v))
            .collect();
        let constants: Vec<_> = file
            .syntax()
            .descendants()
            .filter_map(ast::ConstDecl::cast)
            .filter_map(|c| self.lower_const_decl(&c))
            .collect();
        let lists: Vec<_> = file
            .syntax()
            .descendants()
            .filter_map(ast::ListDecl::cast)
            .filter_map(|l| self.lower_list_decl(&l))
            .collect();
        let externals: Vec<_> = file
            .externals()
            .filter_map(|e| self.lower_external_decl(&e))
            .collect();
        let includes: Vec<_> = file
            .includes()
            .filter_map(|i| self.lower_include(&i))
            .collect();
        let mut knots: Vec<_> = file.knots().filter_map(|k| self.lower_knot(&k)).collect();
        // Top-level stitches (no parent knot) — promoted to knots.
        for stitch in file.stitches() {
            if let Some(knot) = self.lower_top_level_stitch(&stitch) {
                knots.push(knot);
            }
        }
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
        let header = knot.header().or_else(|| {
            self.emit(knot.syntax().text_range(), DiagnosticCode::E001);
            None
        })?;
        let ident = header.identifier().or_else(|| {
            self.emit(knot.syntax().text_range(), DiagnosticCode::E001);
            None
        })?;
        let name_text = header.name().or_else(|| {
            self.emit(knot.syntax().text_range(), DiagnosticCode::E001);
            None
        })?;
        let name = make_name(name_text.clone(), ident.syntax().text_range());

        let is_function = header.is_function();
        let params = self.lower_knot_params(header.params());

        let param_infos: Vec<crate::ParamInfo> = params
            .iter()
            .map(|p| crate::ParamInfo {
                name: p.name.text.clone(),
                is_ref: p.is_ref,
                is_divert: p.is_divert,
            })
            .collect();
        let detail = if is_function {
            Some("function".to_owned())
        } else {
            None
        };
        self.declare_with(
            SymbolKind::Knot,
            &name_text,
            ident.syntax().text_range(),
            param_infos,
            detail,
        );

        self.current_knot = Some(name_text.clone());
        for p in &params {
            self.manifest.locals.push(LocalSymbol {
                name: p.name.text.clone(),
                range: p.name.range,
                scope: self.current_scope(),
                kind: crate::SymbolKind::Param,
                param_detail: Some(crate::ParamInfo {
                    name: p.name.text.clone(),
                    is_ref: p.is_ref,
                    is_divert: p.is_divert,
                }),
            });
        }
        let (body, stitches) = knot.body().map_or_else(
            || (Block::default(), Vec::new()),
            |b| self.lower_knot_body(&b, &name_text),
        );
        self.current_knot = None;
        self.current_stitch = None;

        Some(Knot {
            ptr: ContainerPtr::Knot(AstPtr::new(knot)),
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
        let mut block = self.lower_body_children(body.syntax());

        // First-stitch auto-enter: if the knot body has no content and the
        // first stitch has no parameters, insert an implicit divert to it.
        if block.stmts.is_empty()
            && let Some(first) = stitches.first()
            && first.params.is_empty()
        {
            // Register the synthetic divert as an unresolved reference so
            // the analyzer can resolve it.
            self.add_unresolved(&first.name.text, first.name.range, RefKind::Divert);
            block.stmts.push(Stmt::Divert(Divert {
                ptr: None,
                target: DivertTarget {
                    path: DivertPath::Path(Path {
                        segments: vec![Name {
                            text: first.name.text.clone(),
                            range: first.name.range,
                        }],
                        range: first.name.range,
                    }),
                    args: Vec::new(),
                },
            }));
        }

        (block, stitches)
    }

    /// Lower a top-level stitch (no parent knot) — promoted to knot status
    /// so it becomes a named container at root level.
    fn lower_top_level_stitch(&mut self, stitch: &ast::StitchDef) -> Option<Knot> {
        let header = stitch.header()?;
        let ident = header.identifier()?;
        let name_text = header.name()?;
        let name = make_name(name_text.clone(), ident.syntax().text_range());

        let params = self.lower_knot_params(header.params());
        let param_infos: Vec<crate::ParamInfo> = params
            .iter()
            .map(|p| crate::ParamInfo {
                name: p.name.text.clone(),
                is_ref: p.is_ref,
                is_divert: p.is_divert,
            })
            .collect();
        self.declare_with(
            SymbolKind::Stitch,
            &name_text,
            ident.syntax().text_range(),
            param_infos,
            None,
        );

        self.current_knot = Some(name_text.clone());
        for p in &params {
            self.manifest.locals.push(LocalSymbol {
                name: p.name.text.clone(),
                range: p.name.range,
                scope: self.current_scope(),
                kind: crate::SymbolKind::Param,
                param_detail: Some(crate::ParamInfo {
                    name: p.name.text.clone(),
                    is_ref: p.is_ref,
                    is_divert: p.is_divert,
                }),
            });
        }
        let body = stitch
            .body()
            .map_or_else(Block::default, |b| self.lower_body_children(b.syntax()));
        self.current_knot = None;

        Some(Knot {
            ptr: ContainerPtr::Stitch(AstPtr::new(stitch)),
            name,
            is_function: false,
            params,
            body,
            stitches: Vec::new(),
        })
    }

    fn lower_stitch(&mut self, stitch: &ast::StitchDef, knot_name: &str) -> Option<Stitch> {
        let header = stitch.header().or_else(|| {
            self.emit(stitch.syntax().text_range(), DiagnosticCode::E002);
            None
        })?;
        let ident = header.identifier().or_else(|| {
            self.emit(stitch.syntax().text_range(), DiagnosticCode::E002);
            None
        })?;
        let name_text = header.name().or_else(|| {
            self.emit(stitch.syntax().text_range(), DiagnosticCode::E002);
            None
        })?;
        let name = make_name(name_text.clone(), ident.syntax().text_range());

        let qualified = format!("{knot_name}.{name_text}");

        self.current_stitch = Some(name_text.clone());
        let params = self.lower_knot_params(header.params());
        let param_infos: Vec<crate::ParamInfo> = params
            .iter()
            .map(|p| crate::ParamInfo {
                name: p.name.text.clone(),
                is_ref: p.is_ref,
                is_divert: p.is_divert,
            })
            .collect();
        self.declare_with(
            SymbolKind::Stitch,
            &qualified,
            ident.syntax().text_range(),
            param_infos,
            None,
        );
        for p in &params {
            self.manifest.locals.push(LocalSymbol {
                name: p.name.text.clone(),
                range: p.name.range,
                scope: self.current_scope(),
                kind: crate::SymbolKind::Param,
                param_detail: Some(crate::ParamInfo {
                    name: p.name.text.clone(),
                    is_ref: p.is_ref,
                    is_divert: p.is_divert,
                }),
            });
        }
        let body = stitch
            .body()
            .map_or_else(Block::default, |b| self.lower_body_children(b.syntax()));
        self.current_stitch = None;

        Some(Stitch {
            ptr: AstPtr::new(stitch),
            name,
            params,
            body,
        })
    }
}

impl LowerCtx {
    fn lower_knot_params(&mut self, params: Option<ast::KnotParams>) -> Vec<Param> {
        params
            .map(|p| p.params().filter_map(|pd| self.lower_param(&pd)).collect())
            .unwrap_or_default()
    }

    fn lower_param(&mut self, p: &ast::KnotParamDecl) -> Option<Param> {
        let ident = p.identifier().or_else(|| {
            self.emit(p.syntax().text_range(), DiagnosticCode::E003);
            None
        })?;
        let name = name_from_ident(&ident).or_else(|| {
            self.emit(p.syntax().text_range(), DiagnosticCode::E003);
            None
        })?;
        Some(Param {
            name,
            is_ref: p.is_ref(),
            is_divert: p.is_divert(),
        })
    }
}

// ─── Phase 2: Declarations ──────────────────────────────────────────

impl LowerCtx {
    fn lower_var_decl(&mut self, v: &ast::VarDecl) -> Option<VarDecl> {
        let ident = v.identifier().or_else(|| {
            self.emit(v.syntax().text_range(), DiagnosticCode::E004);
            None
        })?;
        let name = name_from_ident(&ident).or_else(|| {
            self.emit(v.syntax().text_range(), DiagnosticCode::E004);
            None
        })?;
        self.declare(SymbolKind::Variable, &name.text, name.range);

        let value = v
            .value()
            .and_then(|e| self.lower_expr(&e))
            .unwrap_or_else(|| {
                self.emit(v.syntax().text_range(), DiagnosticCode::E005);
                Expr::Null
            });

        Some(VarDecl {
            ptr: AstPtr::new(v),
            name,
            value,
        })
    }

    fn lower_const_decl(&mut self, c: &ast::ConstDecl) -> Option<ConstDecl> {
        let ident = c.identifier().or_else(|| {
            self.emit(c.syntax().text_range(), DiagnosticCode::E006);
            None
        })?;
        let name = name_from_ident(&ident).or_else(|| {
            self.emit(c.syntax().text_range(), DiagnosticCode::E006);
            None
        })?;
        self.declare(SymbolKind::Constant, &name.text, name.range);

        let value = c
            .value()
            .and_then(|e| self.lower_expr(&e))
            .unwrap_or_else(|| {
                self.emit(c.syntax().text_range(), DiagnosticCode::E007);
                Expr::Null
            });

        Some(ConstDecl {
            ptr: AstPtr::new(c),
            name,
            value,
        })
    }

    fn lower_list_decl(&mut self, l: &ast::ListDecl) -> Option<ListDecl> {
        let ident = l.identifier().or_else(|| {
            self.emit(l.syntax().text_range(), DiagnosticCode::E008);
            None
        })?;
        let name = name_from_ident(&ident).or_else(|| {
            self.emit(l.syntax().text_range(), DiagnosticCode::E008);
            None
        })?;
        let list_name_text = name.text.clone();
        self.declare(SymbolKind::List, &list_name_text, name.range);

        let members: Vec<ListMember> = l
            .definition()
            .map(|def| {
                def.members()
                    .filter_map(|m| self.lower_list_member(&m))
                    .collect()
            })
            .unwrap_or_default();

        for member in &members {
            let qualified = format!("{list_name_text}.{}", member.name.text);
            self.declare(SymbolKind::ListItem, &qualified, member.name.range);
        }

        Some(ListDecl {
            ptr: AstPtr::new(l),
            name,
            members,
        })
    }

    fn lower_external_decl(&mut self, e: &ast::ExternalDecl) -> Option<ExternalDecl> {
        let ident = e.identifier().or_else(|| {
            self.emit(e.syntax().text_range(), DiagnosticCode::E010);
            None
        })?;
        let name = name_from_ident(&ident).or_else(|| {
            self.emit(e.syntax().text_range(), DiagnosticCode::E010);
            None
        })?;

        let param_infos: Vec<crate::ParamInfo> = e
            .param_list()
            .into_iter()
            .flat_map(|pl| pl.params().collect::<Vec<_>>())
            .filter_map(|p| {
                p.name().map(|n| crate::ParamInfo {
                    name: n,
                    is_ref: false,
                    is_divert: false,
                })
            })
            .collect();

        self.declare_with(
            SymbolKind::External,
            &name.text,
            name.range,
            param_infos,
            None,
        );

        #[expect(
            clippy::cast_possible_truncation,
            reason = "external params won't exceed 255"
        )]
        let param_count = e.param_list().map_or(0, |pl| pl.params().count() as u8);

        Some(ExternalDecl {
            ptr: AstPtr::new(e),
            name,
            param_count,
        })
    }
}

impl LowerCtx {
    fn lower_list_member(&mut self, m: &ast::ListMember) -> Option<ListMember> {
        let range = m.syntax().text_range();
        if let Some(on) = m.on_member() {
            let name_text = on.name().or_else(|| {
                self.emit(range, DiagnosticCode::E009);
                None
            })?;
            #[expect(clippy::cast_possible_truncation, reason = "list values fit in i32")]
            Some(ListMember {
                name: make_name(name_text, range),
                value: on.value().map(|v| v as i32),
                is_active: true,
            })
        } else if let Some(off) = m.off_member() {
            let name_text = off.name().or_else(|| {
                self.emit(range, DiagnosticCode::E009);
                None
            })?;
            #[expect(clippy::cast_possible_truncation, reason = "list values fit in i32")]
            Some(ListMember {
                name: make_name(name_text, range),
                value: off.value().map(|v| v as i32),
                is_active: false,
            })
        } else {
            self.emit(range, DiagnosticCode::E009);
            None
        }
    }
}

impl LowerCtx {
    fn lower_include(&mut self, inc: &ast::IncludeStmt) -> Option<IncludeSite> {
        let file_path = inc.file_path().or_else(|| {
            self.emit(inc.syntax().text_range(), DiagnosticCode::E011);
            None
        })?;
        let raw = file_path.text();
        // Strip surrounding quotes if present — ink's syntax is `INCLUDE path`,
        // but users sometimes write `INCLUDE "path"`.
        let cleaned = raw
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(&raw);
        Some(IncludeSite {
            file_path: cleaned.to_owned(),
            ptr: AstPtr::new(inc),
        })
    }
}

// ─── Phase 3: Expression lowering ───────────────────────────────────

impl LowerCtx {
    #[expect(clippy::too_many_lines, reason = "match arms are individually simple")]
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
                let range = pe.syntax().text_range();
                let op = lower_prefix_op(pe).or_else(|| {
                    self.emit(range, DiagnosticCode::E016);
                    None
                })?;
                let operand = pe.operand().and_then(|e| self.lower_expr(&e)).or_else(|| {
                    self.emit(range, DiagnosticCode::E015);
                    None
                })?;
                Some(Expr::Prefix(op, Box::new(operand)))
            }
            ast::Expr::Infix(ie) => {
                let range = ie.syntax().text_range();
                let lhs = ie.lhs().and_then(|e| self.lower_expr(&e)).or_else(|| {
                    self.emit(range, DiagnosticCode::E015);
                    None
                })?;
                let op = lower_infix_op(ie).or_else(|| {
                    self.emit(range, DiagnosticCode::E016);
                    None
                })?;
                let rhs = ie.rhs().and_then(|e| self.lower_expr(&e)).or_else(|| {
                    self.emit(range, DiagnosticCode::E015);
                    None
                })?;
                Some(Expr::Infix(Box::new(lhs), op, Box::new(rhs)))
            }
            ast::Expr::Postfix(pe) => {
                let range = pe.syntax().text_range();
                let operand = pe.operand().and_then(|e| self.lower_expr(&e)).or_else(|| {
                    self.emit(range, DiagnosticCode::E015);
                    None
                })?;
                let op = lower_postfix_op(pe).or_else(|| {
                    self.emit(range, DiagnosticCode::E016);
                    None
                })?;
                Some(Expr::Postfix(Box::new(operand), op))
            }
            ast::Expr::Paren(pe) => pe.inner().and_then(|e| self.lower_expr(&e)),
            ast::Expr::FunctionCall(fc) => {
                let range = fc.syntax().text_range();
                let ident = fc.identifier().or_else(|| {
                    self.emit(range, DiagnosticCode::E017);
                    None
                })?;
                let name_text = ident.name().or_else(|| {
                    self.emit(range, DiagnosticCode::E017);
                    None
                })?;
                let range = ident.syntax().text_range();
                let path = Path {
                    segments: vec![make_name(name_text.clone(), range)],
                    range,
                };
                let args: Vec<Expr> = fc
                    .arg_list()
                    .map(|al| al.args().filter_map(|a| self.lower_expr(&a)).collect())
                    .unwrap_or_default();
                self.add_unresolved_with_args(
                    &name_text,
                    range,
                    RefKind::Function,
                    Some(args.len()),
                );
                Some(Expr::Call(path, args))
            }
            ast::Expr::DivertTarget(dt) => {
                let ast_path = dt.target().or_else(|| {
                    self.emit(dt.syntax().text_range(), DiagnosticCode::E018);
                    None
                })?;
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
        SyntaxKind::CARET => Some(InfixOp::Intersect),
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
        let tags = lower_tags(line.tags(), self);

        // If this line has only a divert (no content), emit a divert statement
        if parts.is_empty() && tags.is_empty() {
            return line.divert().and_then(|d| self.lower_divert_node(&d));
        }

        Some(Stmt::Content(Content {
            ptr: Some(SyntaxNodePtr::from_node(line.syntax())),
            parts,
            tags,
        }))
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
            let branches: Vec<Block> = imp
                .branches()
                .map(|b| self.wrap_content_as_block(b.syntax()))
                .collect();
            parts.push(ContentPart::InlineSequence(Sequence {
                ptr: SyntaxNodePtr::from_node(imp.syntax()),
                kind: SequenceType::STOPPING,
                branches,
            }));
        }
    }

    fn lower_inline_conditional(&mut self, cond: &ast::ConditionalWithExpr) -> Option<Conditional> {
        let ptr = SyntaxNodePtr::from_node(cond.syntax());
        let condition = cond
            .condition()
            .and_then(|e| self.lower_expr(&e))
            .or_else(|| {
                self.emit(cond.syntax().text_range(), DiagnosticCode::E020);
                None
            })?;

        Some(self.lower_conditional_with_expr(cond, &condition, ptr))
    }

    /// Unified lowering for `ConditionalWithExpr` — handles all patterns:
    /// branchless body, inline branches, multiline branches, or bare condition.
    fn lower_conditional_with_expr(
        &mut self,
        cond: &ast::ConditionalWithExpr,
        condition: &Expr,
        ptr: SyntaxNodePtr,
    ) -> Conditional {
        let mut branches = Vec::new();

        if let Some(body) = cond.branchless_body() {
            let block = self.lower_branchless_body(&body);
            branches.push(CondBranch {
                condition: Some(condition.clone()),
                body: block,
            });
            if let Some(else_branch) = body.else_branch()
                && let Some(ml_branch) = else_branch.branch()
            {
                let else_body = ml_branch
                    .body()
                    .map_or_else(Block::default, |body| self.lower_branch_body(body.syntax()));
                branches.push(CondBranch {
                    condition: None,
                    body: else_body,
                });
            }
            return Conditional {
                ptr,
                kind: CondKind::InitialCondition,
                branches,
            };
        }

        if let Some(inline_branches) = cond.inline_branches() {
            let mut first = true;
            for b in inline_branches.branches() {
                let cond_expr = if first {
                    first = false;
                    Some(condition.clone())
                } else {
                    None
                };
                branches.push(CondBranch {
                    condition: cond_expr,
                    body: self.wrap_content_as_block(b.syntax()),
                });
            }
            return Conditional {
                ptr,
                kind: CondKind::InitialCondition,
                branches,
            };
        }

        if let Some(ml_branches) = cond.multiline_branches() {
            // Check if this is a true switch (all non-else branches have
            // their own conditions, e.g. `{ x: - 1: ... - 2: ... }`) or a
            // branchless conditional with gather-style body markers
            // (e.g. `{ x == 4: - body - else: other }`).
            let all_have_conditions = ml_branches
                .branches()
                .all(|b| b.is_else() || b.condition().is_some());

            for b in ml_branches.branches() {
                let cond_expr = if b.is_else() {
                    None
                } else {
                    b.condition().and_then(|e| self.lower_expr(&e))
                };
                let body = b
                    .body()
                    .map_or_else(Block::default, |body| self.lower_branch_body(body.syntax()));
                branches.push(CondBranch {
                    condition: cond_expr,
                    body,
                });
            }

            let kind = if all_have_conditions {
                CondKind::Switch(condition.clone())
            } else {
                // Not a switch — treat the initial expression as the
                // condition for the first branch (body content).
                // Prepend it to the first branch that has no condition.
                if let Some(first_no_cond) = branches.iter_mut().find(|b| b.condition.is_none()) {
                    first_no_cond.condition = Some(condition.clone());
                }
                CondKind::InitialCondition
            };

            return Conditional {
                ptr,
                kind,
                branches,
            };
        }

        // Fallback: bare condition, no body
        branches.push(CondBranch {
            condition: Some(condition.clone()),
            body: Block::default(),
        });
        Conditional {
            ptr,
            kind: CondKind::InitialCondition,
            branches,
        }
    }

    /// Lower a `BranchlessCondBody` to a `Block`.
    ///
    /// Children are a mix of block-level (`LOGIC_LINE`, `INLINE_LOGIC`, `DIVERT_NODE`)
    /// and content-level (`TEXT`, `GLUE_NODE`, `ESCAPE`). We accumulate content parts
    /// and flush them as `Stmt::Content` when a block-level node is hit or at end.
    #[expect(clippy::too_many_lines)]
    fn lower_branchless_body(&mut self, body: &ast::BranchlessCondBody) -> Block {
        let mut stmts = Vec::new();
        let mut parts = Vec::new();
        // Track whether this body spans multiple lines. Inline single-line
        // bodies (e.g. `{x: text}`) have no NEWLINE children, and their
        // trailing newline is provided by the enclosing content line.
        let mut is_multiline = false;

        for child in body.syntax().children_with_tokens() {
            match child.kind() {
                SyntaxKind::ELSE_BRANCH => {
                    // Stop — caller handles the else branch separately
                    break;
                }
                SyntaxKind::CONTENT_LINE => {
                    if let Some(cl) = child.into_node().and_then(ast::ContentLine::cast) {
                        let line_parts = cl
                            .mixed_content()
                            .map_or_else(Vec::new, |mc| self.lower_mixed_content(&mc));
                        parts.extend(line_parts);
                        let tags = lower_tags(cl.tags(), self);
                        let has_content = !parts.is_empty() || !tags.is_empty();
                        if has_content {
                            stmts.push(Stmt::Content(Content {
                                ptr: None,
                                parts: std::mem::take(&mut parts),
                                tags,
                            }));
                        }
                        if let Some(dn) = cl.divert()
                            && let Some(s) = self.lower_divert_node(&dn)
                        {
                            stmts.push(s);
                        }
                    }
                }
                SyntaxKind::LOGIC_LINE => {
                    flush_content_parts(&mut parts, &mut stmts);
                    if let Some(ll) = child.into_node().and_then(ast::LogicLine::cast)
                        && let Some(stmt) = self.lower_logic_line(&ll)
                    {
                        stmts.push(stmt);
                    }
                }
                SyntaxKind::DIVERT_NODE => {
                    flush_content_parts(&mut parts, &mut stmts);
                    if let Some(dn) = child.into_node().and_then(ast::DivertNode::cast)
                        && let Some(stmt) = self.lower_divert_node(&dn)
                    {
                        stmts.push(stmt);
                    }
                }
                SyntaxKind::INLINE_LOGIC => {
                    if let Some(il) = child.into_node().and_then(ast::InlineLogic::cast) {
                        // Check if this is a multiline block first
                        if let Some(stmt) = self.lower_multiline_block_from_inline(&il) {
                            flush_content_parts(&mut parts, &mut stmts);
                            stmts.push(stmt);
                        } else {
                            self.lower_inline_logic(&il, &mut parts);
                        }
                    }
                }
                SyntaxKind::TEXT => {
                    let text = child.to_string();
                    if !text.is_empty() {
                        parts.push(ContentPart::Text(text));
                    }
                }
                SyntaxKind::NEWLINE => {
                    is_multiline = true;
                    if !parts.is_empty() {
                        let ends_glue = content_ends_with_glue(&parts);
                        flush_content_parts(&mut parts, &mut stmts);
                        if !ends_glue {
                            stmts.push(Stmt::EndOfLine);
                        }
                    } else if stmts.last().is_some_and(|s| matches!(s, Stmt::Content(_))) {
                        // A CONTENT_LINE already flushed content and cleared
                        // parts. Emit the EndOfLine that the newline represents.
                        stmts.push(Stmt::EndOfLine);
                    }
                }
                SyntaxKind::GLUE_NODE => parts.push(ContentPart::Glue),
                SyntaxKind::ESCAPE => {
                    let text = child.to_string();
                    if text.len() > 1 {
                        parts.push(ContentPart::Text(text[1..].to_string()));
                    }
                }
                SyntaxKind::CHOICE => {
                    flush_content_parts(&mut parts, &mut stmts);
                    if let Some(c) = child.into_node().and_then(ast::Choice::cast)
                        && let Some(choice) = self.lower_choice(&c)
                    {
                        stmts.push(Stmt::ChoiceSet(Box::new(ChoiceSet {
                            choices: vec![choice],
                            continuation: Block::default(),
                            context: ChoiceSetContext::Inline,
                            depth: 0,
                        })));
                    }
                }
                // Tokens (whitespace, punctuation, keywords) are expected
                // from children_with_tokens() iteration.
                other if other.is_token() => {}
                other => {
                    debug_assert!(
                        false,
                        "unexpected node SyntaxKind in lower_branchless_body: {other:?}"
                    );
                }
            }
        }
        flush_content_parts(&mut parts, &mut stmts);

        // In multiline branchless bodies, if the loop ended at ELSE_BRANCH
        // the NEWLINE was absorbed into the ELSE_BRANCH node and never
        // triggered EndOfLine. Emit it now for the trailing content.
        if is_multiline && stmts.last().is_some_and(|s| matches!(s, Stmt::Content(_))) {
            stmts.push(Stmt::EndOfLine);
        }

        // In a branchless body like `{true: + A choice \n body \n -> END}`,
        // "body" and "-> END" are siblings of CHOICE in the CST, not children.
        // They end up as trailing stmts after the ChoiceSet — unreachable past
        // `done`. Move them into the last choice's body so they execute.
        move_trailing_stmts_into_choice_body(&mut stmts);

        Block { label: None, stmts }
    }

    /// Lower a `MultilineBranchBody` to a `Block`.
    ///
    /// Branch bodies contain a mix of block-level (`LOGIC_LINE`, `INLINE_LOGIC`,
    /// `CHOICE`, `DIVERT_NODE`) and content-level (`TEXT`, `GLUE_NODE`, `ESCAPE`) nodes.
    /// We accumulate content parts and flush them when block-level nodes appear.
    #[expect(clippy::too_many_lines)]
    fn lower_branch_body(&mut self, body: &brink_syntax::SyntaxNode) -> Block {
        let mut stmts = Vec::new();
        let mut parts = Vec::new();
        // Track whitespace between content-producing nodes (e.g. `{x} {y}`).
        let mut pending_ws: Option<String> = None;
        let mut seen_content = false;

        for child in body.children_with_tokens() {
            // Capture whitespace tokens between content nodes.
            if let rowan::NodeOrToken::Token(ref token) = child {
                if token.kind() == SyntaxKind::NEWLINE {
                    // Newline token → flush content and emit EndOfLine.
                    if !parts.is_empty() {
                        let ends_glue = content_ends_with_glue(&parts);
                        flush_content_parts(&mut parts, &mut stmts);
                        if !ends_glue {
                            stmts.push(Stmt::EndOfLine);
                        }
                    }
                    // Reset: whitespace after newline is indentation, not content.
                    seen_content = false;
                    pending_ws = None;
                } else if seen_content && token.kind() == SyntaxKind::WHITESPACE {
                    let text = token.text().to_string();
                    if let Some(ref mut ws) = pending_ws {
                        ws.push_str(&text);
                    } else {
                        pending_ws = Some(text);
                    }
                }
                continue;
            }
            let rowan::NodeOrToken::Node(child) = child else {
                continue;
            };
            // Flush pending whitespace before content-producing nodes.
            // INLINE_LOGIC is handled separately below — it may be block-level
            // (conditional/sequence) rather than content-level (value interpolation).
            if matches!(
                child.kind(),
                SyntaxKind::TEXT | SyntaxKind::GLUE_NODE | SyntaxKind::ESCAPE
            ) {
                if let Some(ws) = pending_ws.take() {
                    parts.push(ContentPart::Text(ws));
                }
                seen_content = true;
            } else if child.kind() != SyntaxKind::INLINE_LOGIC {
                pending_ws = None;
            }
            match child.kind() {
                SyntaxKind::CONTENT_LINE => {
                    if let Some(cl) = ast::ContentLine::cast(child) {
                        // Check multiline block promotion
                        if let Some(mc) = cl.mixed_content()
                            && let Some(il) = mc.inline_logics().next()
                            && let Some(stmt) = self.lower_multiline_block_from_inline(&il)
                        {
                            flush_content_parts(&mut parts, &mut stmts);
                            stmts.push(stmt);
                            continue;
                        }
                        let line_parts = cl
                            .mixed_content()
                            .map_or_else(Vec::new, |mc| self.lower_mixed_content(&mc));
                        parts.extend(line_parts);
                        let tags = lower_tags(cl.tags(), self);
                        let has_divert = cl.divert().is_some();
                        let ends_glue = content_ends_with_glue(&parts);
                        if !parts.is_empty() || !tags.is_empty() {
                            stmts.push(Stmt::Content(Content {
                                ptr: None,
                                parts: std::mem::take(&mut parts),
                                tags,
                            }));
                        }
                        if let Some(dn) = cl.divert()
                            && let Some(s) = self.lower_divert_node(&dn)
                        {
                            stmts.push(s);
                        }
                        if !has_divert && !ends_glue {
                            stmts.push(Stmt::EndOfLine);
                        }
                    }
                }
                SyntaxKind::LOGIC_LINE => {
                    flush_content_parts(&mut parts, &mut stmts);
                    if let Some(ll) = ast::LogicLine::cast(child)
                        && let Some(stmt) = self.lower_logic_line(&ll)
                    {
                        stmts.push(stmt);
                    }
                }
                SyntaxKind::DIVERT_NODE => {
                    flush_content_parts(&mut parts, &mut stmts);
                    if let Some(dn) = ast::DivertNode::cast(child)
                        && let Some(stmt) = self.lower_divert_node(&dn)
                    {
                        stmts.push(stmt);
                    }
                }
                SyntaxKind::INLINE_LOGIC => {
                    if let Some(il) = ast::InlineLogic::cast(child) {
                        if let Some(stmt) = self.lower_multiline_block_from_inline(&il) {
                            // Block-level inline logic — discard pending whitespace.
                            pending_ws = None;
                            flush_content_parts(&mut parts, &mut stmts);
                            stmts.push(stmt);
                        } else {
                            // Content-level inline logic — flush whitespace first.
                            if let Some(ws) = pending_ws.take() {
                                parts.push(ContentPart::Text(ws));
                            }
                            seen_content = true;
                            self.lower_inline_logic(&il, &mut parts);
                        }
                    }
                }
                SyntaxKind::TEXT => {
                    let text = child.text().to_string();
                    if !text.is_empty() {
                        parts.push(ContentPart::Text(text));
                    }
                }
                SyntaxKind::NEWLINE => {
                    // Newline after text content → flush and emit EndOfLine
                    if !parts.is_empty() {
                        let ends_glue = content_ends_with_glue(&parts);
                        flush_content_parts(&mut parts, &mut stmts);
                        if !ends_glue {
                            stmts.push(Stmt::EndOfLine);
                        }
                    }
                    // Reset: whitespace after newline is indentation, not content.
                    seen_content = false;
                    pending_ws = None;
                }
                SyntaxKind::GLUE_NODE => parts.push(ContentPart::Glue),
                SyntaxKind::ESCAPE => {
                    let text = child.text().to_string();
                    if text.len() > 1 {
                        parts.push(ContentPart::Text(text[1..].to_string()));
                    }
                }
                SyntaxKind::CHOICE => {
                    flush_content_parts(&mut parts, &mut stmts);
                    if let Some(c) = ast::Choice::cast(child)
                        && let Some(choice) = self.lower_choice(&c)
                    {
                        // Choices inside branch bodies need to be captured
                        // but don't participate in weave folding at this level
                        stmts.push(Stmt::ChoiceSet(Box::new(ChoiceSet {
                            choices: vec![choice],
                            continuation: Block::default(),
                            context: ChoiceSetContext::Inline,
                            depth: 0,
                        })));
                    }
                }
                // Tokens (whitespace, punctuation, keywords) are expected
                // from children_with_tokens() iteration.
                other if other.is_token() => {}
                other => {
                    debug_assert!(
                        false,
                        "unexpected node SyntaxKind in lower_branch_body: {other:?}"
                    );
                }
            }
        }
        if !parts.is_empty() {
            let ends_glue = content_ends_with_glue(&parts);
            flush_content_parts(&mut parts, &mut stmts);
            if !ends_glue {
                stmts.push(Stmt::EndOfLine);
            }
        }

        // In a branch body like `{true: + A choice \n body \n -> END}`,
        // "body" and "-> END" are siblings of CHOICE in the CST, not children.
        // They end up as trailing stmts after the ChoiceSet — unreachable past
        // `done`. Move them into the last choice's body so they execute.
        move_trailing_stmts_into_choice_body(&mut stmts);

        Block { label: None, stmts }
    }

    /// Wrap content-level children as a single-statement Block (for inline branches).
    fn wrap_content_as_block(&mut self, node: &brink_syntax::SyntaxNode) -> Block {
        let parts = self.lower_content_node_children(node);

        // Check for a DIVERT_NODE child (e.g. `{cond:->target}`) —
        // `lower_content_node_children` skips these, so we handle them here.
        let divert_stmt = node
            .children()
            .find_map(ast::DivertNode::cast)
            .and_then(|dn| self.lower_divert_node(&dn));

        // Extract tags from TAGS child node (e.g. `{red #red|...}`).
        let tags = lower_tags(node.children().find_map(ast::Tags::cast), self);

        let mut stmts = Vec::new();
        if !parts.is_empty() || !tags.is_empty() {
            stmts.push(Stmt::Content(Content {
                ptr: None,
                parts,
                tags,
            }));
        }
        if let Some(d) = divert_stmt {
            stmts.push(d);
        }
        if stmts.is_empty() {
            return Block::default();
        }
        Block { label: None, stmts }
    }

    fn lower_inline_sequence(&mut self, seq: &ast::SequenceWithAnnotation) -> Option<Sequence> {
        let kind = lower_sequence_type(seq);

        let branches = if let Some(inline_branches) = seq.inline_branches() {
            inline_branches
                .branches()
                .map(|b| self.wrap_content_as_block(b.syntax()))
                .collect()
        } else if let Some(ml_branches) = seq.multiline_branches() {
            ml_branches
                .branches()
                .map(|b| {
                    b.body()
                        .map_or_else(Block::default, |body| self.lower_branch_body(body.syntax()))
                })
                .collect()
        } else {
            self.emit(seq.syntax().text_range(), DiagnosticCode::E021);
            return None;
        };

        Some(Sequence {
            ptr: SyntaxNodePtr::from_node(seq.syntax()),
            kind,
            branches,
        })
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
                    // DIVERT_NODE and TAGS appear as siblings in
                    // MIXED_CONTENT — handled by the caller.
                    SyntaxKind::DIVERT_NODE | SyntaxKind::TAGS => {}
                    other => {
                        debug_assert!(
                            false,
                            "unexpected node SyntaxKind in lower_content_node_children: {other:?}"
                        );
                    }
                }
            }
        }
        parts
    }
}

fn content_ends_with_glue(parts: &[ContentPart]) -> bool {
    matches!(parts.last(), Some(ContentPart::Glue))
}

/// When a choice appears inside a conditional branch body, trailing stmts
/// (content, diverts) are siblings of the CHOICE in the CST, not children.
/// They end up after the `ChoiceSet` and are unreachable past `done`. Move them
/// into the last choice's body so they execute when the choice is taken.
fn move_trailing_stmts_into_choice_body(stmts: &mut Vec<Stmt>) {
    if let Some(choice_set_pos) = stmts.iter().rposition(|s| matches!(s, Stmt::ChoiceSet(_)))
        && choice_set_pos < stmts.len() - 1
    {
        let trailing: Vec<Stmt> = stmts.drain(choice_set_pos + 1..).collect();
        if let Stmt::ChoiceSet(cs) = &mut stmts[choice_set_pos]
            && let Some(choice) = cs.choices.last_mut()
        {
            choice.body.stmts.extend(trailing);
        }
    }
}

fn flush_content_parts(parts: &mut Vec<ContentPart>, stmts: &mut Vec<Stmt>) {
    if !parts.is_empty() {
        stmts.push(Stmt::Content(Content {
            ptr: None,
            parts: std::mem::take(parts),
            tags: Vec::new(),
        }));
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

fn lower_tags(tags: Option<ast::Tags>, ctx: &mut LowerCtx) -> Vec<Tag> {
    tags.map_or_else(Vec::new, |t| {
        t.tags().map(|tag| lower_tag(&tag, ctx)).collect()
    })
}

fn lower_tag(tag: &ast::Tag, ctx: &mut LowerCtx) -> Tag {
    use brink_syntax::SyntaxKind::HASH;

    let mut parts = Vec::new();
    let mut text_buf = String::new();
    let mut first = true;

    for child in tag.syntax().children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(tok) => {
                if first && tok.kind() == HASH {
                    first = false;
                    continue; // skip leading #
                }
                first = false;
                text_buf.push_str(tok.text());
            }
            rowan::NodeOrToken::Node(node) => {
                first = false;
                if node.kind() == SyntaxKind::INLINE_LOGIC {
                    // Flush accumulated text
                    if !text_buf.is_empty() {
                        parts.push(ContentPart::Text(std::mem::take(&mut text_buf)));
                    }
                    if let Some(inline) = ast::InlineLogic::cast(node) {
                        ctx.lower_inline_logic(&inline, &mut parts);
                    }
                }
            }
        }
    }
    // Flush remaining text, trim trailing whitespace
    let remaining = text_buf.trim_end().to_string();
    if !remaining.is_empty() {
        parts.push(ContentPart::Text(remaining));
    }
    // Trim leading whitespace on first text part
    if let Some(ContentPart::Text(t)) = parts.first_mut() {
        *t = t.trim_start().to_string();
        if t.is_empty() {
            parts.remove(0);
        }
    }

    Tag {
        parts,
        ptr: AstPtr::new(tag),
    }
}

// ─── Phase 5: Control flow ──────────────────────────────────────────

impl LowerCtx {
    fn lower_divert_node(&mut self, node: &ast::DivertNode) -> Option<Stmt> {
        if let Some(thread) = node.thread_start() {
            if let Some(ts) = self.lower_thread_target(&thread) {
                return Some(Stmt::ThreadStart(ts));
            }
            self.emit(node.syntax().text_range(), DiagnosticCode::E013);
            return None;
        }

        if let Some(tunnel) = node.tunnel_call() {
            let targets: Vec<DivertTarget> = tunnel
                .targets()
                .filter_map(|t| self.lower_divert_target_with_args(&t))
                .collect();
            if !targets.is_empty() {
                return Some(Stmt::TunnelCall(TunnelCall {
                    ptr: AstPtr::new(node),
                    targets,
                }));
            }
            self.emit(node.syntax().text_range(), DiagnosticCode::E012);
            return None;
        }

        if let Some(tunnel_onwards) = node.tunnel_onwards() {
            let onwards_targets: Vec<DivertTarget> = tunnel_onwards
                .targets()
                .filter_map(|t| self.lower_divert_target_with_args(&t))
                .collect();

            if let Some(tc) = tunnel_onwards.tunnel_call() {
                // `->-> A -> B` — chained tunnel call through onwards target
                let mut targets = onwards_targets;
                targets.extend(
                    tc.targets()
                        .filter_map(|t| self.lower_divert_target_with_args(&t)),
                );
                if !targets.is_empty() {
                    return Some(Stmt::TunnelCall(TunnelCall {
                        ptr: AstPtr::new(node),
                        targets,
                    }));
                }
            } else if let Some(target) = onwards_targets.into_iter().next() {
                // `->-> B` — tunnel return with divert override.
                // Push the target as a DivertTarget expression so the runtime
                // redirects to B instead of the original tunnel return address.
                match &target.path {
                    DivertPath::Path(path) => {
                        let value = Some(Expr::DivertTarget(path.clone()));
                        return Some(Stmt::Return(Return {
                            ptr: None,
                            value,
                            onwards_args: target.args,
                        }));
                    }
                    DivertPath::Done => {
                        return Some(Stmt::Divert(Divert {
                            ptr: Some(SyntaxNodePtr::from_node(node.syntax())),
                            target: DivertTarget {
                                path: DivertPath::Done,
                                args: Vec::new(),
                            },
                        }));
                    }
                    DivertPath::End => {
                        return Some(Stmt::Divert(Divert {
                            ptr: Some(SyntaxNodePtr::from_node(node.syntax())),
                            target: DivertTarget {
                                path: DivertPath::End,
                                args: Vec::new(),
                            },
                        }));
                    }
                }
            }

            // Bare `->->` with no targets — tunnel return
            return Some(Stmt::Return(Return {
                ptr: None,
                value: None,
                onwards_args: Vec::new(),
            }));
        }

        if let Some(simple) = node.simple_divert() {
            let targets: Vec<DivertTarget> = simple
                .targets()
                .filter_map(|t| self.lower_divert_target_with_args(&t))
                .collect();
            return match targets.len() {
                1 => Some(Stmt::Divert(Divert {
                    ptr: Some(SyntaxNodePtr::from_node(node.syntax())),
                    target: targets.into_iter().next()?,
                })),
                n if n > 1 => Some(Stmt::TunnelCall(TunnelCall {
                    ptr: AstPtr::new(node),
                    targets,
                })),
                _ => {
                    self.emit(node.syntax().text_range(), DiagnosticCode::E012);
                    None
                }
            };
        }

        self.emit(node.syntax().text_range(), DiagnosticCode::E012);
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
            ptr: AstPtr::new(thread),
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
                ptr: Some(AstPtr::new(&ret)),
                value: ret.value().and_then(|e| self.lower_expr(&e)),
                onwards_args: Vec::new(),
            }));
        }

        if let Some(temp) = line.temp_decl() {
            let name = name_from_ident(&temp.identifier()?)?;
            let value = temp.value().and_then(|e| self.lower_expr(&e));
            // Emit the local *after* lowering the initializer so
            // `~ temp x = x` doesn't accidentally self-reference.
            self.manifest.locals.push(LocalSymbol {
                name: name.text.clone(),
                range: name.range,
                scope: self.current_scope(),
                kind: crate::SymbolKind::Temp,
                param_detail: None,
            });
            return Some(Stmt::TempDecl(TempDecl {
                ptr: AstPtr::new(&temp),
                name,
                value,
            }));
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
            return Some(Stmt::Assignment(Assignment {
                ptr: AstPtr::new(&assign),
                target,
                op,
                value,
            }));
        }

        // Bare expression statement: ~ expr
        for child in line.syntax().children() {
            if let Some(expr) = ast::Expr::cast(child)
                && let Some(e) = self.lower_expr(&expr)
            {
                return Some(Stmt::ExprStmt(e));
            }
        }

        self.emit(line.syntax().text_range(), DiagnosticCode::E014);
        None
    }
}

// ─── Phase 6: Choice and gather lowering ────────────────────────────

impl LowerCtx {
    /// Trim trailing whitespace from the last `Text` part in a content part list.
    /// The parser captures whitespace before diverts (e.g. `choice -> DONE`
    /// yields `"choice "`); the C# ink runtime strips this, so we must too.
    fn trim_trailing_whitespace(parts: &mut Vec<ContentPart>) {
        if let Some(ContentPart::Text(t)) = parts.last_mut() {
            let trimmed = t.trim_end().to_string();
            if trimmed.is_empty() {
                parts.pop();
            } else {
                *t = trimmed;
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "choice lowering has many CST regions"
    )]
    fn lower_choice(&mut self, choice: &ast::Choice) -> Option<Choice> {
        let bullets = choice.bullets().or_else(|| {
            self.emit(choice.syntax().text_range(), DiagnosticCode::E019);
            None
        })?;
        let is_sticky = bullets.is_sticky();

        let label = choice
            .label()
            .and_then(|l| name_from_ident(&l.identifier()?));

        if let Some(ref label_name) = label {
            let qualified = self.qualify_label(&label_name.text);
            self.declare(SymbolKind::Label, &qualified, label_name.range);
        }

        let is_fallback = choice.start_content().is_none()
            && choice.bracket_content().is_none()
            && choice.inner_content().is_none();

        // Multiple choice conditions are ANDed together. If a condition's
        // expression fails to lower, `lower_expr` already emits a diagnostic
        // (E015/E016), so we skip it here rather than duplicating the error.
        let condition = choice
            .conditions()
            .filter_map(|c| c.expr().and_then(|e| self.lower_expr(&e)))
            .reduce(|a, b| Expr::Infix(Box::new(a), InfixOp::And, Box::new(b)));

        let mut start_content = choice.start_content().map(|sc| {
            let mut parts = self.lower_content_node_children(sc.syntax());
            Self::trim_trailing_whitespace(&mut parts);
            Content {
                ptr: None,
                parts,
                tags: Vec::new(),
            }
        });

        let bracket_content = choice.bracket_content().map(|bc| {
            // Collect tags inside bracket content.
            let bracket_tags: Vec<Tag> = bc
                .syntax()
                .children()
                .filter_map(ast::Tags::cast)
                .flat_map(|t| lower_tags(Some(t), self))
                .collect();
            Content {
                ptr: None,
                parts: self.lower_content_node_children(bc.syntax()),
                tags: bracket_tags,
            }
        });

        let mut inner_content = choice.inner_content().map(|ic| Content {
            ptr: None,
            parts: self.lower_content_node_children(ic.syntax()),
            tags: Vec::new(),
        });

        // Assign TAGS at the CHOICE level to the preceding content region.
        // Walk CST children in order: TAGS after CHOICE_START_CONTENT → start,
        // TAGS after CHOICE_INNER_CONTENT → inner, trailing TAGS → inner.
        {
            use brink_syntax::SyntaxKind;
            let mut last_region = "start"; // default if no content regions yet
            for child in choice.syntax().children() {
                match child.kind() {
                    SyntaxKind::CHOICE_START_CONTENT => last_region = "start",
                    SyntaxKind::CHOICE_BRACKET_CONTENT => last_region = "bracket",
                    SyntaxKind::CHOICE_INNER_CONTENT => last_region = "inner",
                    SyntaxKind::TAGS => {
                        let tags_node = ast::Tags::cast(child);
                        let lowered = lower_tags(tags_node, self);
                        match last_region {
                            "start" => {
                                if let Some(ref mut sc) = start_content {
                                    sc.tags.extend(lowered);
                                }
                            }
                            _ => {
                                if let Some(ref mut ic) = inner_content {
                                    ic.tags.extend(lowered);
                                } else if let Some(ref mut sc) = start_content {
                                    sc.tags.extend(lowered);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let inline_divert = choice.divert().and_then(|d| {
            let target = d
                .simple_divert()?
                .targets()
                .next()
                .and_then(|t| self.lower_divert_target_with_args(&t))?;
            Some(Divert {
                ptr: Some(SyntaxNodePtr::from_node(d.syntax())),
                target,
            })
        });

        // Choice-level tags are now distributed to content regions above.
        let tags = Vec::new();
        // Skip the choice-level divert in the body when it was captured as an
        // inline divert OR when it's an empty simple divert (bare `->` on a
        // choice, meaning "fall through to gather"). Non-simple diverts
        // (tunnel onwards, tunnel calls) must flow through lower_body_child
        // so they get proper HIR representation.
        let has_empty_simple_divert = choice.divert().is_some_and(|d| {
            d.simple_divert()
                .is_some_and(|sd| sd.targets().next().is_none())
        });
        let skip_divert = inline_divert.is_some() || has_empty_simple_divert;
        let mut body = self.lower_choice_body(choice, skip_divert);

        // Prepend inline divert + EndOfLine to the body.
        let mut preamble = Vec::new();
        if let Some(d) = inline_divert {
            preamble.push(Stmt::Divert(d));
        }
        preamble.push(Stmt::EndOfLine);
        preamble.append(&mut body.stmts);
        body.stmts = preamble;

        Some(Choice {
            ptr: AstPtr::new(choice),
            is_sticky,
            is_fallback,
            label,
            condition,
            start_content,
            bracket_content,
            inner_content,
            tags,
            body,
        })
    }

    fn lower_choice_body(&mut self, choice: &ast::Choice, skip_divert: bool) -> Block {
        // The choice-level divert (e.g. `* choice -> DONE`) is skipped here
        // when it was captured as an inline simple divert by the caller
        // (lower_choice). Non-simple diverts (tunnel onwards, etc.) are NOT
        // skipped — they flow through lower_body_child for proper lowering.
        let choice_divert_range = if skip_divert {
            choice.divert().map(|d| d.syntax().text_range())
        } else {
            None
        };

        let mut stmts = Vec::new();
        for child in choice.syntax().children() {
            if choice_divert_range.is_some_and(|r| r == child.text_range()) {
                continue;
            }
            self.lower_body_child(child, &mut stmts);
        }
        Block { label: None, stmts }
    }

    /// Lower a content line, emitting Content + optional Divert + optional `EndOfLine`.
    ///
    /// If the content line wraps a multiline block-level construct (conditional or
    /// sequence), promotes it and returns `true`. Otherwise returns `false`.
    fn emit_content_line_stmts(
        &mut self,
        cl: &ast::ContentLine,
        push: &mut impl FnMut(Stmt),
    ) -> bool {
        // Check if this content line is just a wrapper around a multiline
        // block-level construct (conditional or sequence with multiline
        // branches). The reference ink parser doesn't distinguish these at the
        // brace level — they're all InlineLogic, with the multiline-vs-inline
        // decision made inside. So we promote here.
        if let Some(mc) = cl.mixed_content()
            && let Some(il) = mc.inline_logics().next()
            && let Some(stmt) = self.lower_multiline_block_from_inline(&il)
        {
            push(stmt);

            // Collect trailing content parts (glue, text, inline logic) that
            // appear after the promoted InlineLogic in the MixedContent node.
            let il_syntax = il.syntax().clone();
            let mut past_promoted = false;
            let mut trailing_parts = Vec::new();
            for child in mc.syntax().children_with_tokens() {
                if let rowan::NodeOrToken::Node(ref child_node) = child
                    && *child_node == il_syntax
                {
                    past_promoted = true;
                    continue;
                }
                if !past_promoted {
                    continue;
                }
                if let rowan::NodeOrToken::Node(child_node) = child {
                    match child_node.kind() {
                        SyntaxKind::TEXT => {
                            let text = child_node.text().to_string();
                            if !text.is_empty() {
                                trailing_parts.push(ContentPart::Text(text));
                            }
                        }
                        SyntaxKind::GLUE_NODE => trailing_parts.push(ContentPart::Glue),
                        SyntaxKind::ESCAPE => {
                            let text = child_node.text().to_string();
                            if text.len() > 1 {
                                trailing_parts.push(ContentPart::Text(text[1..].to_string()));
                            }
                        }
                        SyntaxKind::INLINE_LOGIC => {
                            if let Some(inline) = ast::InlineLogic::cast(child_node) {
                                // A trailing inline logic might itself be a multiline
                                // block that needs promotion.
                                if let Some(promoted) =
                                    self.lower_multiline_block_from_inline(&inline)
                                {
                                    // Flush any accumulated trailing parts first.
                                    if !trailing_parts.is_empty() {
                                        push(Stmt::Content(Content {
                                            ptr: None,
                                            parts: std::mem::take(&mut trailing_parts),
                                            tags: vec![],
                                        }));
                                    }
                                    push(promoted);
                                } else {
                                    self.lower_inline_logic(&inline, &mut trailing_parts);
                                }
                            }
                        }
                        // DIVERT_NODE, TAGS, and other node types are
                        // handled by the caller or are irrelevant here.
                        _ => {}
                    }
                }
            }

            if !trailing_parts.is_empty() {
                let ends_glue = content_ends_with_glue(&trailing_parts);
                push(Stmt::Content(Content {
                    ptr: None,
                    parts: trailing_parts,
                    tags: vec![],
                }));
                if !ends_glue {
                    if let Some(dn) = cl.divert()
                        && let Some(s) = self.lower_divert_node(&dn)
                    {
                        push(s);
                    } else if cl.divert().is_none() {
                        push(Stmt::EndOfLine);
                    }
                }
            } else if let Some(dn) = cl.divert()
                && let Some(s) = self.lower_divert_node(&dn)
            {
                push(s);
            }

            return true;
        }

        let stmt = self.lower_content_line(cl);
        let was_content = matches!(&stmt, Some(Stmt::Content(_)));
        let ends_glue = matches!(
            &stmt,
            Some(Stmt::Content(c)) if content_ends_with_glue(&c.parts)
        );
        if let Some(s) = stmt {
            push(s);
        }
        let has_divert = cl.divert().is_some();
        if was_content
            && let Some(dn) = cl.divert()
            && let Some(s) = self.lower_divert_node(&dn)
        {
            push(s);
        }
        if was_content && !has_divert && !ends_glue {
            push(Stmt::EndOfLine);
        }
        false
    }

    fn lower_body_child(&mut self, child: brink_syntax::SyntaxNode, out: &mut Vec<Stmt>) {
        match child.kind() {
            SyntaxKind::CONTENT_LINE => {
                if let Some(cl) = ast::ContentLine::cast(child) {
                    self.emit_content_line_stmts(&cl, &mut |s| out.push(s));
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
            // Structural parts of the Choice node handled by the caller.
            SyntaxKind::CHOICE_BULLETS
            | SyntaxKind::LABEL
            | SyntaxKind::CHOICE_CONDITION
            | SyntaxKind::CHOICE_START_CONTENT
            | SyntaxKind::CHOICE_BRACKET_CONTENT
            | SyntaxKind::CHOICE_INNER_CONTENT
            | SyntaxKind::TAGS
            | SyntaxKind::MIXED_CONTENT
            | SyntaxKind::GATHER_DASHES
            | SyntaxKind::MULTILINE_BLOCK
            | SyntaxKind::GATHER => {}
            other => {
                debug_assert!(
                    other.is_trivia(),
                    "unexpected SyntaxKind in lower_body_child: {other:?}"
                );
            }
        }
    }

    /// Lower an AST gather into a continuation `Block`.
    ///
    /// The gather's label becomes the block's label. Its content, divert, and
    /// tags become statements in the block.
    fn lower_gather_to_block(&mut self, gather: &ast::Gather) -> Block {
        let label = gather
            .label()
            .and_then(|l| name_from_ident(&l.identifier()?));

        if let Some(ref label_name) = label {
            let qualified = self.qualify_label(&label_name.text);
            self.declare(SymbolKind::Label, &qualified, label_name.range);
        }

        let content = gather.mixed_content().map(|mc| Content {
            ptr: None,
            parts: self.lower_mixed_content(&mc),
            tags: Vec::new(),
        });

        let divert_stmt = gather.divert().and_then(|d| self.lower_divert_node(&d));

        let tags = lower_tags(gather.tags(), self);

        let mut stmts = Vec::new();
        let has_content = content
            .as_ref()
            .is_some_and(|c| !c.parts.is_empty() || !tags.is_empty());
        let ends_glue = content
            .as_ref()
            .is_some_and(|c| content_ends_with_glue(&c.parts));
        if let Some(c) = content
            && has_content
        {
            stmts.push(Stmt::Content(Content {
                ptr: None,
                parts: c.parts,
                tags,
            }));
        }
        if let Some(d) = divert_stmt {
            stmts.push(d);
        } else if has_content && !ends_glue {
            // Gather line with content but no divert needs an EndOfLine,
            // just like a regular content line. Glue suppresses the newline.
            stmts.push(Stmt::EndOfLine);
        }

        Block { label, stmts }
    }
}

// ─── Phase 7: Body assembly and weave folding ───────────────────────

pub enum WeaveItem {
    Choice { choice: Box<Choice>, depth: usize },
    Continuation { block: Block, depth: usize },
    Stmt(Stmt),
}

impl LowerCtx {
    #[expect(clippy::too_many_lines, reason = "match arms are individually simple")]
    fn lower_body_children(&mut self, parent: &brink_syntax::SyntaxNode) -> Block {
        let mut items = Vec::new();

        for child in parent.children() {
            match child.kind() {
                SyntaxKind::CONTENT_LINE => {
                    if let Some(cl) = ast::ContentLine::cast(child.clone()) {
                        self.emit_content_line_stmts(&cl, &mut |s| {
                            items.push(WeaveItem::Stmt(s));
                        });
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
                        let tags = lower_tags(tl.tags(), self);
                        if !tags.is_empty() {
                            items.push(WeaveItem::Stmt(Stmt::Content(Content {
                                ptr: None,
                                parts: Vec::new(),
                                tags,
                            })));
                            items.push(WeaveItem::Stmt(Stmt::EndOfLine));
                        }
                    }
                }
                SyntaxKind::CHOICE => {
                    if let Some(c) = ast::Choice::cast(child) {
                        let depth = c.bullets().map_or(1, |b| b.depth());
                        if let Some(choice) = self.lower_choice(&c) {
                            items.push(WeaveItem::Choice {
                                choice: Box::new(choice),
                                depth,
                            });
                        }
                    }
                }
                SyntaxKind::GATHER => {
                    if let Some(g) = ast::Gather::cast(child) {
                        let depth = g.dashes().map_or(1, |d| d.depth());
                        items.push(WeaveItem::Continuation {
                            block: self.lower_gather_to_block(&g),
                            depth,
                        });
                        // Gather-choice same line: `- * hello` embeds a choice
                        // inside the gather node. Emit it as a separate weave item.
                        if let Some(c) = g.choice() {
                            let choice_depth = c.bullets().map_or(1, |b| b.depth());
                            if let Some(choice) = self.lower_choice(&c) {
                                items.push(WeaveItem::Choice {
                                    choice: Box::new(choice),
                                    depth: choice_depth,
                                });
                            }
                        }
                    }
                }
                SyntaxKind::INLINE_LOGIC => {
                    if let Some(il) = ast::InlineLogic::cast(child)
                        && let Some(stmt) = self.lower_multiline_block_from_inline(&il)
                    {
                        items.push(WeaveItem::Stmt(stmt));
                    }
                }
                SyntaxKind::MULTILINE_BLOCK => {
                    if let Some(mb) = ast::MultilineBlock::cast(child)
                        && let Some(stmt) = self.lower_multiline_block(&mb)
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
                // Structural parts of the parent node handled by the
                // caller (knot/stitch defs, declarations, headers).
                // These appear when the parent is SOURCE_FILE or KNOT_BODY.
                SyntaxKind::KNOT_DEF
                | SyntaxKind::KNOT_HEADER
                | SyntaxKind::STITCH_DEF
                | SyntaxKind::STITCH_HEADER
                | SyntaxKind::VAR_DECL
                | SyntaxKind::CONST_DECL
                | SyntaxKind::LIST_DECL
                | SyntaxKind::EXTERNAL_DECL
                | SyntaxKind::INCLUDE_STMT
                | SyntaxKind::EMPTY_LINE
                | SyntaxKind::STRAY_CLOSING_BRACE
                | SyntaxKind::AUTHOR_WARNING => {}
                other => {
                    debug_assert!(
                        other.is_trivia(),
                        "unexpected SyntaxKind in lower_body_children: {other:?}"
                    );
                }
            }
        }

        fold_weave(items)
    }

    fn lower_multiline_block(&mut self, mb: &ast::MultilineBlock) -> Option<Stmt> {
        let ptr = SyntaxNodePtr::from_node(mb.syntax());

        if let Some(cond) = mb.conditional() {
            if let Some(condition) = cond.condition().and_then(|e| self.lower_expr(&e)) {
                return Some(Stmt::Conditional(
                    self.lower_conditional_with_expr(&cond, &condition, ptr),
                ));
            }
            return None;
        }

        if let Some(seq) = mb.sequence()
            && seq.multiline_branches().is_some()
        {
            return Some(Stmt::Sequence(self.lower_block_sequence(&seq)));
        }

        if let Some(branches) = mb.branches_cond() {
            let cond = self.lower_multiline_conditional_from_branches(&branches, ptr);
            return Some(Stmt::Conditional(cond));
        }

        None
    }

    fn lower_multiline_block_from_inline(&mut self, inline: &ast::InlineLogic) -> Option<Stmt> {
        let ptr = SyntaxNodePtr::from_node(inline.syntax());

        if let Some(ml_cond) = inline.multiline_conditional() {
            return Some(Stmt::Conditional(
                self.lower_multiline_conditional(&ml_cond, ptr),
            ));
        }

        if let Some(cond) = inline.conditional() {
            if (cond.multiline_branches().is_some() || cond.branchless_body().is_some())
                && let Some(condition) = cond.condition().and_then(|e| self.lower_expr(&e))
            {
                return Some(Stmt::Conditional(
                    self.lower_conditional_with_expr(&cond, &condition, ptr),
                ));
            }
            return None;
        }

        if let Some(seq) = inline.sequence()
            && seq.multiline_branches().is_some()
        {
            return Some(Stmt::Sequence(self.lower_block_sequence(&seq)));
        }

        None
    }

    fn lower_multiline_conditional(
        &mut self,
        mc: &ast::MultilineConditional,
        ptr: SyntaxNodePtr,
    ) -> Conditional {
        self.lower_cond_branches(mc.branches(), ptr)
    }

    fn lower_multiline_conditional_from_branches(
        &mut self,
        mb: &ast::MultilineBranchesCond,
        ptr: SyntaxNodePtr,
    ) -> Conditional {
        self.lower_cond_branches(mb.branches(), ptr)
    }

    fn lower_cond_branches(
        &mut self,
        branches: impl Iterator<Item = ast::MultilineBranchCond>,
        ptr: SyntaxNodePtr,
    ) -> Conditional {
        let branches = branches
            .map(|b| {
                let condition = if b.is_else() {
                    None
                } else {
                    b.condition().and_then(|e| self.lower_expr(&e))
                };
                let body = b
                    .body()
                    .map_or_else(Block::default, |body| self.lower_branch_body(body.syntax()));
                CondBranch { condition, body }
            })
            .collect();
        Conditional {
            ptr,
            kind: CondKind::IfElse,
            branches,
        }
    }

    fn lower_block_sequence(&mut self, seq: &ast::SequenceWithAnnotation) -> Sequence {
        let kind = lower_sequence_type(seq);
        let branches = seq.multiline_branches().map_or_else(Vec::new, |ml| {
            ml.branches()
                .map(|b| {
                    let mut block = b
                        .body()
                        .map_or_else(Block::default, |body| self.lower_branch_body(body.syntax()));
                    // Block-level sequence branches start on a new line relative
                    // to any preceding content. Inklecate places a "\n" at the
                    // start of each branch's content stream.
                    block.stmts.insert(0, Stmt::EndOfLine);
                    block
                })
                .collect()
        });
        Sequence {
            ptr: SyntaxNodePtr::from_node(seq.syntax()),
            kind,
            branches,
        }
    }
}

// ─── Weave folding ──────────────────────────────────────────────────

/// Fold a flat stream of `WeaveItem`s into a recursively nested `Block`.
///
/// Matches the reference ink compiler's `ConstructWeaveHierarchyFromIndentation`:
/// items at deeper depths are recursively folded and inserted into the preceding
/// weave point's body.
pub fn fold_weave(items: Vec<WeaveItem>) -> Block {
    let base_depth = determine_base_depth(&items);
    fold_weave_at_depth(items, base_depth)
}

/// Determine the base depth from the first choice or gather in the list.
fn determine_base_depth(items: &[WeaveItem]) -> usize {
    for item in items {
        match item {
            WeaveItem::Choice { depth, .. } | WeaveItem::Continuation { depth, .. } => {
                return *depth;
            }
            WeaveItem::Stmt(_) => {}
        }
    }
    1
}

/// Fold items at a given base depth. Items at deeper depths are collected
/// and recursively folded into the preceding weave point's body.
fn fold_weave_at_depth(items: Vec<WeaveItem>, base_depth: usize) -> Block {
    // Phase 1: Group nested items into sub-weaves (matching ConstructWeaveHierarchyFromIndentation)
    let items = nest_deeper_items(items, base_depth);

    // Phase 2: Build choice sets from the now-single-depth stream.
    //
    // Key invariant: everything after a gather nests *inside* the gather's
    // continuation block. When we encounter a Continuation after accumulated
    // choices, we recursively fold all remaining items into the continuation
    // and stop — producing a nested tree, not flat siblings.
    let mut stmts = Vec::new();
    let mut choice_acc: Vec<Choice> = Vec::new();
    let mut last_standalone_label: Option<Name> = None;
    // Tracks where in `stmts` a standalone labeled gather's content begins,
    // so we can retroactively wrap it in a LabeledBlock if no choices follow.
    let mut gather_stmts_start: Option<usize> = None;

    let mut iter = items.into_iter();
    while let Some(item) = iter.next() {
        match item {
            WeaveItem::Stmt(stmt) => {
                if choice_acc.is_empty() {
                    stmts.push(stmt);
                } else {
                    // Content between choices belongs to the previous choice's body
                    // (matches reference ink's addContentToPreviousWeavePoint)
                    if let Some(c) = choice_acc.last_mut() {
                        c.body.stmts.push(stmt);
                    }
                }
            }
            WeaveItem::Choice { choice, .. } => {
                choice_acc.push(*choice);
            }
            WeaveItem::Continuation { block, depth } => {
                if choice_acc.is_empty() {
                    // When a new labeled gather arrives while a previous
                    // labeled gather is pending, nest the new gather (and
                    // everything after it) inside the previous one.  This
                    // mirrors inklecate's tail-nesting: `-> opts` loops
                    // back to opts, and because test is nested inside opts,
                    // test is naturally re-entered.
                    if let Some(start) = gather_stmts_start.take()
                        && let Some(prev_label) = last_standalone_label.take()
                        && block.label.is_some()
                    {
                        let mut gather_stmts = stmts.split_off(start);
                        // Recurse: fold the new gather + remaining items.
                        let mut remaining = vec![WeaveItem::Continuation { block, depth }];
                        remaining.extend(iter);
                        let nested = fold_weave_at_depth(remaining, base_depth);
                        gather_stmts.extend(nested.stmts);

                        stmts.push(Stmt::LabeledBlock(Box::new(Block {
                            label: Some(prev_label),
                            stmts: gather_stmts,
                        })));
                        return Block { label: None, stmts };
                    }
                    // Standalone gather — emit content as stmts, save label
                    gather_stmts_start = block.label.as_ref().map(|_| stmts.len());
                    emit_standalone_gather(&mut stmts, &block);
                    last_standalone_label = block.label;
                } else {
                    // Gather after choices — label was consumed as opening label.
                    // Collect remaining items, fold them recursively, and nest
                    // everything into the continuation.
                    let mut continuation = block;
                    let remaining: Vec<WeaveItem> = iter.collect();
                    if !remaining.is_empty() {
                        let nested = fold_weave_at_depth(remaining, base_depth);
                        continuation.stmts.extend(nested.stmts);
                    }
                    flush_choices(
                        &mut stmts,
                        &mut choice_acc,
                        continuation,
                        last_standalone_label.take(),
                        gather_stmts_start.take(),
                        base_depth,
                    );
                    // All remaining items consumed — we're done
                    return Block { label: None, stmts };
                }
            }
        }
    }

    // If a standalone labeled gather was never consumed by a choice set,
    // retroactively wrap its content in a LabeledBlock so the planning phase
    // allocates a container for it (making it a valid divert target).
    if choice_acc.is_empty()
        && let Some(start) = gather_stmts_start
        && let Some(label) = last_standalone_label.take()
    {
        let gather_stmts = stmts.split_off(start);
        stmts.push(Stmt::LabeledBlock(Box::new(Block {
            label: Some(label),
            stmts: gather_stmts,
        })));
    }

    flush_choices(
        &mut stmts,
        &mut choice_acc,
        Block::default(),
        last_standalone_label.take(),
        gather_stmts_start,
        base_depth,
    );
    Block { label: None, stmts }
}

/// Extract runs of deeper-depth items and recursively fold them into nested blocks,
/// inserting the result into the preceding weave point's body.
fn nest_deeper_items(items: Vec<WeaveItem>, base_depth: usize) -> Vec<WeaveItem> {
    let mut result = Vec::new();
    let mut iter = items.into_iter().peekable();

    while let Some(item) = iter.next() {
        let depth = item_depth(&item);

        if let Some(d) = depth
            && d > base_depth
        {
            // Collect all consecutive items at this deeper depth or beyond
            let inner_depth = d;
            let mut nested_items = vec![item];
            loop {
                let Some(peeked) = iter.peek() else {
                    break;
                };
                if let Some(d) = item_depth(peeked)
                    && d <= base_depth
                {
                    break;
                }
                // Safe: we just peeked successfully
                if let Some(next) = iter.next() {
                    nested_items.push(next);
                }
            }
            let nested_block = fold_weave_at_depth(nested_items, inner_depth);

            // Attach the nested block to the previous weave point's body
            if let Some(WeaveItem::Choice { choice, .. }) = result.last_mut() {
                choice.body.stmts.extend(nested_block.stmts);
            } else {
                // No preceding choice — emit as standalone stmts
                for stmt in nested_block.stmts {
                    result.push(WeaveItem::Stmt(stmt));
                }
            }
        } else {
            result.push(item);
        }
    }

    result
}

fn item_depth(item: &WeaveItem) -> Option<usize> {
    match item {
        WeaveItem::Choice { depth, .. } | WeaveItem::Continuation { depth, .. } => Some(*depth),
        WeaveItem::Stmt(_) => None,
    }
}

#[expect(clippy::cast_possible_truncation)]
fn flush_choices(
    stmts: &mut Vec<Stmt>,
    choice_acc: &mut Vec<Choice>,
    continuation: Block,
    opening_label: Option<Name>,
    gather_stmts_start: Option<usize>,
    base_depth: usize,
) {
    if choice_acc.is_empty() {
        return;
    }
    let choices = std::mem::take(choice_acc);
    let cs = Stmt::ChoiceSet(Box::new(ChoiceSet {
        choices,
        continuation,
        context: ChoiceSetContext::Weave,
        depth: base_depth as u32,
    }));
    if let Some(label) = opening_label {
        // Move statements emitted after the standalone gather into the
        // labeled block so they live inside the gather container.  This
        // ensures thread calls and other code between the gather label
        // and the first choice are re-executed when looping back.
        let mut labeled_stmts = gather_stmts_start
            .map(|start| stmts.split_off(start))
            .unwrap_or_default();
        labeled_stmts.push(cs);
        stmts.push(Stmt::LabeledBlock(Box::new(Block {
            label: Some(label),
            stmts: labeled_stmts,
        })));
    } else {
        stmts.push(cs);
    }
}

/// Emit a standalone gather's content as statements.
///
/// The label is preserved by the caller for potential use as an opening label
/// on a subsequent choice set.
fn emit_standalone_gather(stmts: &mut Vec<Stmt>, block: &Block) {
    for stmt in &block.stmts {
        stmts.push(stmt.clone());
    }
}
