//! Expression compilation: LIR `Expr` → opcodes.

use brink_format::{ListValue, Opcode};
use brink_ir::lir;

use crate::ContainerEmitter;

impl ContainerEmitter<'_> {
    /// Emit an expression. When `display` is true, function calls are
    /// wrapped in `BeginFragment`/`EndFragment` so their output is captured
    /// structurally for locale re-rendering.
    #[expect(
        clippy::too_many_lines,
        reason = "one match arm per LIR Expr variant; splitting would obscure the dispatch"
    )]
    pub(super) fn emit_expr(&mut self, expr: &lir::Expr, display: bool) {
        match expr {
            lir::Expr::Int(n) => self.emit(Opcode::PushInt(*n)),
            lir::Expr::Float(f) => self.emit(Opcode::PushFloat(*f)),
            lir::Expr::Bool(b) => self.emit(Opcode::PushBool(*b)),
            lir::Expr::Null => self.emit(Opcode::PushNull),

            lir::Expr::String(s) => self.emit_string_expr(s),

            lir::Expr::GetGlobal(id) => self.emit(Opcode::GetGlobal(*id)),
            lir::Expr::GetTemp(slot, _) => self.emit(Opcode::GetTemp(*slot)),
            lir::Expr::TakeGlobal(id) => self.emit(Opcode::TakeGlobal(*id)),
            lir::Expr::TakeTemp(slot, _) => self.emit(Opcode::TakeTemp(*slot)),

            lir::Expr::VisitCount(id) => {
                self.emit(Opcode::PushDivertTarget(*id));
                self.emit(Opcode::VisitCount);
            }

            lir::Expr::DivertTarget(id) => self.emit(Opcode::PushDivertTarget(*id)),

            lir::Expr::ListLiteral { items, origins } => {
                let lv = ListValue {
                    items: items.clone(),
                    origins: origins.clone(),
                };
                let idx = self.list_literals.len();
                self.list_literals.push(lv);
                #[expect(clippy::cast_possible_truncation)]
                self.emit(Opcode::PushList(idx as u16));
            }

            lir::Expr::Prefix(op, inner) => {
                self.emit_expr(inner, false);
                match op {
                    brink_ir::PrefixOp::Negate => self.emit(Opcode::Negate),
                    brink_ir::PrefixOp::Not => self.emit(Opcode::Not),
                }
            }

            lir::Expr::Infix(lhs, op, rhs) => {
                self.emit_expr(lhs, false);
                self.emit_expr(rhs, false);
                self.emit(infix_op_to_opcode(*op));
            }

            lir::Expr::Postfix(inner, op) => {
                self.emit_expr(inner, false);
                match op {
                    brink_ir::PostfixOp::Increment => {
                        self.emit(Opcode::PushInt(1));
                        self.emit(Opcode::Add);
                    }
                    brink_ir::PostfixOp::Decrement => {
                        self.emit(Opcode::PushInt(1));
                        self.emit(Opcode::Subtract);
                    }
                }
            }

            lir::Expr::Call { target, args } => {
                for arg in args {
                    self.emit_call_arg(arg);
                }
                self.emit_fragment_wrapped(display, Opcode::Call(*target));
            }

            lir::Expr::CallExternal {
                target,
                args,
                arg_count,
            } => {
                for arg in args {
                    self.emit_call_arg(arg);
                }
                self.emit_fragment_wrapped(display, Opcode::CallExternal(*target, *arg_count));
            }

            lir::Expr::CallVariable { target, args } => {
                for arg in args {
                    self.emit_call_arg(arg);
                }
                self.emit(Opcode::GetGlobal(*target));
                self.emit_fragment_wrapped(
                    display,
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "a call supplies <=255 args"
                    )]
                    Opcode::CallVariable(args.len() as u8),
                );
            }

            lir::Expr::CallVariableTemp { slot, args, .. } => {
                for arg in args {
                    self.emit_call_arg(arg);
                }
                self.emit(Opcode::GetTemp(*slot));
                self.emit_fragment_wrapped(
                    display,
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "a call supplies <=255 args"
                    )]
                    Opcode::CallVariable(args.len() as u8),
                );
            }

            lir::Expr::CallBuiltin { builtin, args } => {
                self.emit_builtin(*builtin, args);
            }

            // ── Function values (T1c, #700) ──────────────────────────
            lir::Expr::MakeFnValue { target, bound } => {
                for arg in bound {
                    self.emit_call_arg(arg);
                }
                if bound.is_empty() {
                    self.emit(Opcode::PushFnRef(*target));
                } else {
                    self.emit(Opcode::MakeClosure {
                        target: *target,
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "a #fn binds <=255 args (E081 caps at the declared row)"
                        )]
                        bound_count: bound.len() as u8,
                    });
                }
            }

            lir::Expr::CallValue { callee, args } => {
                for arg in args {
                    self.emit_expr(arg, false);
                }
                self.emit_expr(callee, false);
                self.emit_fragment_wrapped(
                    display,
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "a call supplies <=255 args"
                    )]
                    Opcode::CallValue(args.len() as u8),
                );
            }

            lir::Expr::BindValue { callee, args } => {
                // Same stack shape as `CallValue`: push the supplied args
                // (bottom), then the callee (top). `BindValue` returns a new
                // function value rather than entering the target, so it never
                // produces localized output — no fragment wrapping needed.
                for arg in args {
                    self.emit_expr(arg, false);
                }
                self.emit_expr(callee, false);
                self.emit(
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "a bind supplies <=255 args"
                    )]
                    Opcode::BindValue(args.len() as u8),
                );
            }

            // ── Collections (T1b) ────────────────────────────────────
            lir::Expr::ConstLiteral(v) => self.emit_literal_pool_push(v),

            #[expect(
                clippy::cast_possible_truncation,
                reason = "collection literals stay well under u32::MAX elements"
            )]
            lir::Expr::ArrayNew(elements) => {
                for e in elements {
                    self.emit_expr(e, false);
                }
                self.emit(Opcode::ArrayNew(elements.len() as u32));
            }

            #[expect(
                clippy::cast_possible_truncation,
                reason = "collection literals stay well under u32::MAX entries"
            )]
            lir::Expr::MapNew(entries) => {
                for (k, v) in entries {
                    self.emit_expr(k, false);
                    self.emit_expr(v, false);
                }
                self.emit(Opcode::MapNew(entries.len() as u32));
            }

            lir::Expr::Index { base, index } => {
                self.emit_expr(base, false);
                self.emit_expr(index, false);
                self.emit(Opcode::IndexGet);
            }

            lir::Expr::IndexSet { base, index, value } => {
                self.emit_expr(base, false);
                self.emit_expr(index, false);
                self.emit_expr(value, false);
                self.emit(Opcode::IndexSet);
            }

            lir::Expr::CollectionLen(inner) => {
                self.emit_expr(inner, false);
                self.emit(Opcode::CollectionLen);
            }

            lir::Expr::CollectionKeys(inner) => {
                self.emit_expr(inner, false);
                self.emit(Opcode::CollectionKeys);
            }

            lir::Expr::CollectionValues(inner) => {
                self.emit_expr(inner, false);
                self.emit(Opcode::CollectionValues);
            }

            lir::Expr::CollectionContains { container, needle } => {
                self.emit_expr(container, false);
                self.emit_expr(needle, false);
                self.emit(Opcode::MapContains);
            }

            lir::Expr::CollectionInsert { base, key, value } => {
                self.emit_expr(base, false);
                self.emit_expr(key, false);
                self.emit_expr(value, false);
                self.emit(Opcode::MapInsert);
            }

            lir::Expr::CollectionRemove { base, key } => {
                self.emit_expr(base, false);
                self.emit_expr(key, false);
                self.emit(Opcode::MapRemove);
            }

            // ── Records (TM-4c) ──────────────────────────────────────
            lir::Expr::RecordNew { shape_id, fields } => {
                for f in fields {
                    self.emit_expr(f, false);
                }
                self.emit(Opcode::RecordNew(*shape_id));
            }

            lir::Expr::RecordGet {
                base,
                field,
                static_offset,
            } => {
                self.emit_expr(base, false);
                if let Some(offset) = static_offset {
                    self.emit(Opcode::RecordGet(*offset));
                } else {
                    self.emit(Opcode::RecordGetDyn(field.0));
                }
            }

            lir::Expr::RecordSet {
                base,
                field,
                static_offset,
                value,
            } => {
                self.emit_expr(base, false);
                self.emit_expr(value, false);
                if let Some(offset) = static_offset {
                    self.emit(Opcode::RecordSet(*offset));
                } else {
                    self.emit(Opcode::RecordSetDyn(field.0));
                }
            }

            // ── Conversion intrinsics (TM-3 completion, #659) ─────────
            lir::Expr::ConvertInt(inner) => {
                self.emit_expr(inner, false);
                self.emit(Opcode::ConvertInt);
            }

            lir::Expr::ConvertFloat(inner) => {
                self.emit_expr(inner, false);
                self.emit(Opcode::ConvertFloat);
            }

            lir::Expr::ConvertString(inner) => {
                self.emit_expr(inner, false);
                self.emit(Opcode::ConvertString);
            }
        }
    }

    /// Push a T1b constant collection literal via the literal pool
    /// (`PushLiteral(idx)`), deduplicating by structural equality against
    /// entries already in the pool (`docs/format-v4-rfc.md` §2).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "literal pools stay well under u32::MAX entries"
    )]
    fn emit_literal_pool_push(&mut self, v: &lir::ConstValue) {
        let value = crate::const_to_value(v, self.state_name_table, self.state_name_index);
        let idx = self
            .literal_pool
            .iter()
            .position(|existing| *existing == value)
            .unwrap_or_else(|| {
                self.literal_pool.push(value);
                self.literal_pool.len() - 1
            });
        self.emit(Opcode::PushLiteral(idx as u32));
    }

    /// Emit a call opcode. The runtime no longer implicitly captures
    /// function output — the compiler emits explicit `BeginFragment`/
    /// `EndFragment` around calls when capture is needed (e.g. template
    /// slot composition in `emit_recognized_line`).
    fn emit_fragment_wrapped(&mut self, _display: bool, op: Opcode) {
        self.emit(op);
    }

    pub(super) fn emit_call_arg(&mut self, arg: &lir::CallArg) {
        match arg {
            lir::CallArg::Value(expr) => self.emit_expr(expr, false),
            lir::CallArg::RefGlobal(id) => self.emit(Opcode::PushVarPointer(*id)),
            lir::CallArg::RefTemp(slot, _) => self.emit(Opcode::PushTempPointer(*slot)),
        }
    }

    fn emit_string_expr(&mut self, s: &lir::StringExpr) {
        // Single literal → intern as PushString
        if s.parts.len() == 1
            && let lir::StringPart::Literal(text) = &s.parts[0]
        {
            let name_id = self.intern_string(text);
            self.emit(Opcode::PushString(name_id.0));
            return;
        }

        // Mixed parts → BeginStringEval + parts + EndStringEval
        self.emit(Opcode::BeginStringEval);
        for part in &s.parts {
            match part {
                lir::StringPart::Literal(text) => {
                    let idx = self.add_line(text);
                    self.emit(Opcode::EmitLine(idx, 0));
                }
                lir::StringPart::Interpolation(expr) => {
                    self.emit_expr(expr, false);
                    self.emit(Opcode::EmitValue);
                }
            }
        }
        self.emit(Opcode::EndStringEval);
    }

    fn emit_builtin(&mut self, builtin: lir::BuiltinFn, args: &[lir::Expr]) {
        match builtin {
            lir::BuiltinFn::ChoiceCount => self.emit(Opcode::ChoiceCount),
            lir::BuiltinFn::Turns => self.emit(Opcode::TurnIndex),
            lir::BuiltinFn::TurnsSince => {
                for arg in args {
                    self.emit_expr(arg, false);
                }
                self.emit(Opcode::TurnsSince);
            }
            lir::BuiltinFn::ReadCount => {
                for arg in args {
                    self.emit_expr(arg, false);
                }
                self.emit(Opcode::VisitCount);
            }
            _ => {
                for arg in args {
                    self.emit_expr(arg, false);
                }
                self.emit(builtin_to_opcode(builtin));
            }
        }
    }
}

fn infix_op_to_opcode(op: brink_ir::InfixOp) -> Opcode {
    match op {
        brink_ir::InfixOp::Add => Opcode::Add,
        brink_ir::InfixOp::Sub => Opcode::Subtract,
        brink_ir::InfixOp::Mul => Opcode::Multiply,
        brink_ir::InfixOp::Div => Opcode::Divide,
        brink_ir::InfixOp::Mod => Opcode::Modulo,
        brink_ir::InfixOp::Intersect => Opcode::ListIntersect,
        brink_ir::InfixOp::Eq => Opcode::Equal,
        brink_ir::InfixOp::NotEq => Opcode::NotEqual,
        brink_ir::InfixOp::Lt => Opcode::Less,
        brink_ir::InfixOp::Gt => Opcode::Greater,
        brink_ir::InfixOp::LtEq => Opcode::LessOrEqual,
        brink_ir::InfixOp::GtEq => Opcode::GreaterOrEqual,
        brink_ir::InfixOp::And => Opcode::And,
        brink_ir::InfixOp::Or => Opcode::Or,
        brink_ir::InfixOp::Has => Opcode::ListContains,
        brink_ir::InfixOp::HasNot => Opcode::ListNotContains,
    }
}

fn builtin_to_opcode(b: lir::BuiltinFn) -> Opcode {
    match b {
        lir::BuiltinFn::TurnsSince => Opcode::TurnsSince,
        lir::BuiltinFn::ReadCount => Opcode::VisitCount,
        lir::BuiltinFn::ChoiceCount => Opcode::ChoiceCount,
        lir::BuiltinFn::Turns => Opcode::TurnIndex,
        lir::BuiltinFn::Random => Opcode::Random,
        lir::BuiltinFn::SeedRandom => Opcode::SeedRandom,
        lir::BuiltinFn::CastToInt => Opcode::CastToInt,
        lir::BuiltinFn::CastToFloat => Opcode::CastToFloat,
        lir::BuiltinFn::Floor => Opcode::Floor,
        lir::BuiltinFn::Ceiling => Opcode::Ceiling,
        lir::BuiltinFn::Pow => Opcode::Pow,
        lir::BuiltinFn::Min => Opcode::Min,
        lir::BuiltinFn::Max => Opcode::Max,
        lir::BuiltinFn::ListCount => Opcode::ListCount,
        lir::BuiltinFn::ListMin => Opcode::ListMin,
        lir::BuiltinFn::ListMax => Opcode::ListMax,
        lir::BuiltinFn::ListAll => Opcode::ListAll,
        lir::BuiltinFn::ListInvert => Opcode::ListInvert,
        lir::BuiltinFn::ListRange => Opcode::ListRange,
        lir::BuiltinFn::ListRandom => Opcode::ListRandom,
        lir::BuiltinFn::ListValue => Opcode::ListValue,
        lir::BuiltinFn::ListFromInt => Opcode::ListFromInt,
    }
}
