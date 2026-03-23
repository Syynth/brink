//! Expression compilation: LIR `Expr` → opcodes.

use brink_format::{ListValue, Opcode};
use brink_ir::lir;

use crate::ContainerEmitter;

impl ContainerEmitter<'_> {
    /// Emit an expression. When `display` is true, function calls are
    /// wrapped in `BeginFragment`/`EndFragment` so their output is captured
    /// structurally for locale re-rendering.
    pub(super) fn emit_expr(&mut self, expr: &lir::Expr, display: bool) {
        match expr {
            lir::Expr::Int(n) => self.emit(Opcode::PushInt(*n)),
            lir::Expr::Float(f) => self.emit(Opcode::PushFloat(*f)),
            lir::Expr::Bool(b) => self.emit(Opcode::PushBool(*b)),
            lir::Expr::Null => self.emit(Opcode::PushNull),

            lir::Expr::String(s) => self.emit_string_expr(s),

            lir::Expr::GetGlobal(id) => self.emit(Opcode::GetGlobal(*id)),
            lir::Expr::GetTemp(slot, _) => self.emit(Opcode::GetTemp(*slot)),

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
                self.emit_fragment_wrapped(display, Opcode::CallVariable);
            }

            lir::Expr::CallVariableTemp { slot, args, .. } => {
                for arg in args {
                    self.emit_call_arg(arg);
                }
                self.emit(Opcode::GetTemp(*slot));
                self.emit_fragment_wrapped(display, Opcode::CallVariable);
            }

            lir::Expr::CallBuiltin { builtin, args } => {
                self.emit_builtin(*builtin, args);
            }
        }
    }

    /// Emit a call opcode, wrapping in BeginFragment/EndFragment when in
    /// display context so function output is captured structurally.
    fn emit_fragment_wrapped(&mut self, display: bool, op: Opcode) {
        if display {
            self.emit(Opcode::BeginFragment);
        }
        self.emit(op);
        if display {
            self.emit(Opcode::EndFragment);
        }
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
