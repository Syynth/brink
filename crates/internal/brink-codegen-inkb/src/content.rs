//! Content emission: LIR `Content` → opcodes + line table entries.

use brink_format::Opcode;
use brink_ir::lir;

use crate::ContainerEmitter;

impl ContainerEmitter<'_> {
    pub(super) fn emit_recognized_line(&mut self, emission: &lir::ContentEmission) {
        let slot_info = emission.metadata.slot_info.clone();
        let source_location = emission.metadata.source_location.clone();
        match &emission.line {
            lir::RecognizedLine::Plain(text) => {
                let idx = self.add_line_with_hash(
                    text,
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                self.emit(Opcode::EmitLine(idx, 0));
            }
            lir::RecognizedLine::Template {
                parts: template_parts,
                slot_exprs,
            } => {
                // Evaluate slot expressions — each pushes one value onto the stack.
                // Function calls need composition: side-effect output + return value
                // are composed into a single FragmentRef so the line table entry
                // stays clean while the output order matches C#.
                for expr in slot_exprs {
                    self.emit_slot_expr(expr);
                }
                let idx = self.add_template_line(
                    template_parts.clone(),
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                #[expect(clippy::cast_possible_truncation)]
                self.emit(Opcode::EmitLine(idx, slot_exprs.len() as u8));
            }
        }

        for tag in &emission.tags {
            self.emit(Opcode::BeginTag);
            // No per-tag location — see `emit_content`'s tag loop doc.
            self.emit_content_parts(tag, None);
            self.emit(Opcode::EndTag);
        }
    }

    /// Emit a recognized line wrapped in `BeginFragment`/`EndFragment` with
    /// tags **inside** the fragment. Tags captured inside the fragment are
    /// stored on the `Fragment` struct so the runtime can route them to the
    /// consumer (e.g. `BeginChoice` pulls them onto the choice).
    pub(super) fn emit_fragment_recognized_line_with_tags(
        &mut self,
        emission: &lir::ContentEmission,
        extra_tags: &[Vec<lir::ContentPart>],
    ) {
        let slot_info = emission.metadata.slot_info.clone();
        let source_location = emission.metadata.source_location.clone();
        match &emission.line {
            lir::RecognizedLine::Plain(text) => {
                let idx = self.add_line_with_hash(
                    text,
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                self.emit(Opcode::BeginFragment);
                self.emit(Opcode::EmitLine(idx, 0));
                self.emit_tags(&emission.tags);
                self.emit_tags(extra_tags);
                self.emit(Opcode::EndFragment);
            }
            lir::RecognizedLine::Template {
                parts: template_parts,
                slot_exprs,
            } => {
                for expr in slot_exprs {
                    self.emit_slot_expr(expr);
                }
                let idx = self.add_template_line(
                    template_parts.clone(),
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                self.emit(Opcode::BeginFragment);
                #[expect(clippy::cast_possible_truncation)]
                self.emit(Opcode::EmitLine(idx, slot_exprs.len() as u8));
                self.emit_tags(&emission.tags);
                self.emit_tags(extra_tags);
                self.emit(Opcode::EndFragment);
            }
        }
    }

    /// Emit `BeginTag`/content/`EndTag` for each tag.
    pub(super) fn emit_tags(&mut self, tags: &[Vec<lir::ContentPart>]) {
        for tag in tags {
            self.emit(Opcode::BeginTag);
            // No per-tag location — see `emit_content`'s tag loop doc.
            self.emit_content_parts(tag, None);
            self.emit(Opcode::EndTag);
        }
    }

    /// Emit a recognized line as an `EvalLine` opcode (pushes result onto value stack).
    /// Used for choice display text promoted to a line table entry.
    pub(super) fn emit_eval_line(&mut self, emission: &lir::ContentEmission) {
        let slot_info = emission.metadata.slot_info.clone();
        let source_location = emission.metadata.source_location.clone();
        match &emission.line {
            lir::RecognizedLine::Plain(text) => {
                let idx = self.add_line_with_hash(
                    text,
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                self.emit(Opcode::EvalLine(idx, 0));
            }
            lir::RecognizedLine::Template {
                parts: template_parts,
                slot_exprs,
            } => {
                for expr in slot_exprs {
                    self.emit_slot_expr(expr);
                }
                let idx = self.add_template_line(
                    template_parts.clone(),
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                #[expect(clippy::cast_possible_truncation)]
                self.emit(Opcode::EvalLine(idx, slot_exprs.len() as u8));
            }
        }
        // No tags for EvalLine — choice tags are emitted separately after EndChoice.
    }

    pub(super) fn emit_content(&mut self, content: &lir::Content) {
        self.emit_content_parts(&content.parts, content.source_location.as_ref());

        for tag in &content.tags {
            self.emit(Opcode::BeginTag);
            // Tags carry no location of their own here (issue #3181):
            // `lir::Content::tags` is `Vec<Vec<ContentPart>>` — the
            // `hir::Tag::ptr` each one had is discarded flattening it in
            // `lower_content`, and the enclosing content's range would
            // over-claim precision for a tag's own byte span (a tag can sit
            // on the same source line as content it isn't co-extensive
            // with). Reusing it would be exactly the "confidently wrong"
            // location the issue warns against — `None` stays honest.
            self.emit_content_parts(tag, None);
            self.emit(Opcode::EndTag);
        }
    }

    /// Emit content parts for choice display text (no trailing newline).
    pub(super) fn emit_choice_content(&mut self, content: &lir::Content) {
        self.emit_content_parts(&content.parts, content.source_location.as_ref());
    }

    /// Emit content parts — text, glue, interpolations, inline conditionals/sequences.
    ///
    /// `source_location` covers the whole enclosing `Content` line (issue
    /// #3181) — the same one-location-per-line granularity the recognized
    /// path uses (`LineMetadata::source_location`), not a separate range
    /// per flattened `Text` fragment; every non-empty `Text` part reaching
    /// [`Self::add_line`] here gets a clone of it.
    pub(super) fn emit_content_parts(
        &mut self,
        parts: &[lir::ContentPart],
        source_location: Option<&brink_format::SourceLocation>,
    ) {
        for part in parts {
            match part {
                lir::ContentPart::Text(s) => {
                    // Strip boundary whitespace, emit Springs for word breaks.
                    let has_leading_ws = s.starts_with(char::is_whitespace);
                    let has_trailing_ws = s.ends_with(char::is_whitespace);
                    let trimmed = s.trim();

                    if has_leading_ws {
                        self.emit(Opcode::Spring);
                    }
                    if !trimmed.is_empty() {
                        let idx = self.add_line(trimmed, source_location.cloned());
                        self.emit(Opcode::EmitLine(idx, 0));
                    }
                    if has_trailing_ws && !trimmed.is_empty() {
                        self.emit(Opcode::Spring);
                    }
                    // If the string was entirely whitespace (trimmed is empty),
                    // the leading Spring covers it — no trailing Spring needed.
                }
                lir::ContentPart::Glue => {
                    self.emit(Opcode::Glue);
                }
                lir::ContentPart::Spring => {
                    self.emit(Opcode::Spring);
                }
                lir::ContentPart::Interpolation(expr) => {
                    self.emit_expr(expr, true);
                    self.emit(Opcode::EmitValue);
                }
                lir::ContentPart::InlineConditional(cond) => {
                    self.emit_conditional(cond);
                }
                lir::ContentPart::InlineSequence(seq) => {
                    self.emit_sequence(seq);
                }
                lir::ContentPart::EnterSequence(id) => {
                    self.emit(Opcode::EnterContainer(*id));
                }
            }
        }
    }

    /// Emit a slot expression for a template line.
    ///
    /// For function calls, uses the composition pattern: captures side-effect
    /// output in a fragment, then composes it with the return value into a
    /// single `FragmentRef`.  This ensures the line table entry stays clean
    /// (one slot) while side-effect text appears in the correct position
    /// within the resolved line.
    ///
    /// For non-call expressions, evaluates directly — the result goes on the
    /// value stack with no fragment overhead.
    pub(super) fn emit_slot_expr(&mut self, expr: &lir::Expr) {
        // A call anywhere in the slot — `{f()}` or `{f() == "x"}` alike
        // (issue #3525) — composes its printed output into the slot, where
        // ink evaluates it; a bare evaluation would push that text ahead
        // of the line's earlier content.
        if expr.contains_function_call() {
            // Composition pattern:
            //   BeginFragment (compose)
            //     BeginFragment (side effects)
            //       Call func → side effects captured, return value on stack
            //     EndFragment  → store side effects → FragmentRef on stack
            //                    stack: [return_value, FragmentRef(side_effects)]
            //     EmitValue    → pop FragmentRef → emit side effects into compose
            //     EmitValue    → pop return_value → emit into compose
            //   EndFragment    → store composed → FragmentRef on stack
            self.emit(Opcode::BeginFragment);
            self.emit(Opcode::BeginFragment);
            self.emit_expr(expr, false);
            self.emit(Opcode::EndFragment);
            self.emit(Opcode::EmitValue);
            self.emit(Opcode::EmitValue);
            self.emit(Opcode::EndFragment);
        } else {
            self.emit_expr(expr, false);
        }
    }
}

impl ContainerEmitter<'_> {
    /// #3273 (stage 1): emit an enumerated variant-group line.
    ///
    /// Three phases, matching `StmtKind::EmitLineVariants`'s contract:
    ///
    /// 1. **Advance + index** per alternative: `TouchVisit` records the
    ///    view on the SHARED container and hands back the pre-increment
    ///    count; the kind arithmetic is byte-for-byte `emit_sequence`'s
    ///    (`cycle` = modulo, `stopping` = min N-1, `once` = min N — where
    ///    index N selects the exhausted empty variant the enumeration laid
    ///    out at dim position N), except shuffles, which route through
    ///    `ShuffleIndexOf` so the seed is the alternative's own
    ///    `path_hash`, not the line's container.
    /// 2. **Fold** the indices row-major (first alternative slowest):
    ///    `acc = ((i0 * d1) + i1) * d2 + i2 …`.
    /// 3. **Switch** on the combo: `Duplicate / PushInt(c) / Equal /
    ///    JumpIfFalse(next)` per variant, each leaf a `Pop` (discard the
    ///    combo), the variant's slot expressions, and a STATIC
    ///    `EmitLine` — the line table stays whole-line-per-variant, which
    ///    is what keeps every variant a translation unit and a VO slot.
    ///
    /// Registers the `LineVariantGroup` record for the run of entries it
    /// appends, keyed by this emitter's scope.
    pub(super) fn emit_line_variants(&mut self, v: &lir::VariantLineEmission) {
        debug_assert_eq!(
            v.variants.len(),
            v.dims.iter().map(|&d| usize::from(d)).product::<usize>(),
            "variants must fill the dims product (enumeration contract)"
        );
        debug_assert_eq!(v.alts.len(), v.dims.len());
        if v.variants.is_empty() || v.alts.is_empty() {
            return;
        }

        // Phase 1 + 2: per-alt index with running row-major fold.
        for (pos, alt) in v.alts.iter().enumerate() {
            if pos > 0 {
                // acc *= dims[pos] BEFORE this alt's index lands.
                self.emit(Opcode::PushInt(i32::from(v.dims[pos])));
                self.emit(Opcode::Multiply);
            }
            let n = i32::from(alt.branch_count);
            self.emit(Opcode::PushDivertTarget(alt.container_id));
            self.emit(Opcode::TouchVisit);
            if alt.kind == brink_ir::SequenceType::SHUFFLE {
                // seq_count is on the stack; ShuffleIndexOf pops target,
                // then num_elements, then seq_count.
                self.emit(Opcode::PushInt(n));
                self.emit(Opcode::PushDivertTarget(alt.container_id));
                self.emit(Opcode::ShuffleIndexOf);
            } else if alt.kind == brink_ir::SequenceType::CYCLE {
                self.emit(Opcode::PushInt(n));
                self.emit(Opcode::Modulo);
            } else if alt.kind == brink_ir::SequenceType::ONCE {
                // min(count, N): index N IS meaningful — the exhausted
                // variant at dim position N.
                self.emit(Opcode::PushInt(n));
                self.emit(Opcode::Min);
            } else {
                // stopping (the admission default): min(count, N-1).
                self.emit(Opcode::PushInt(n - 1));
                self.emit(Opcode::Min);
            }
            if pos > 0 {
                self.emit(Opcode::Add);
            }
        }

        // Register the line-table run + its group record.
        #[expect(clippy::cast_possible_truncation)]
        let base = self.scope_line_table.len() as u16;
        self.line_variant_groups
            .push(brink_format::LineVariantGroup {
                scope_id: self.scope_id,
                base: u32::from(base),
                dims: v.dims.clone(),
            });

        // Phase 3: the switch. Leaves emit in variant order; every leaf
        // but the last tests the combo, the last is unconditional (the
        // fold can only produce in-range values, so falling through to it
        // is correct, not lenient).
        let mut end_jumps = Vec::with_capacity(v.variants.len().saturating_sub(1));
        for (combo, emission) in v.variants.iter().enumerate() {
            let is_last = combo + 1 == v.variants.len();
            let next_jump = if is_last {
                None
            } else {
                self.emit(Opcode::Duplicate);
                #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                self.emit(Opcode::PushInt(combo as i32));
                self.emit(Opcode::Equal);
                Some(self.emit_jump_placeholder(Opcode::JumpIfFalse(0)))
            };

            // The combo value is spent.
            self.emit(Opcode::Pop);
            self.emit_variant_leaf(emission);

            if let Some(site) = next_jump {
                end_jumps.push(self.emit_jump_placeholder(Opcode::Jump(0)));
                self.patch_jump(site);
            }
        }
        for site in end_jumps {
            self.patch_jump(site);
        }

        // Tags once, from the first variant — one authored line, one tag
        // set (`VariantLineEmission::variants`' doc).
        if let Some(first) = v.variants.first() {
            for tag in &first.tags {
                self.emit(Opcode::BeginTag);
                self.emit_content_parts(tag, None);
                self.emit(Opcode::EndTag);
            }
        }
    }

    /// One switch leaf: the variant's slot expressions, then its static
    /// `EmitLine` — `emit_recognized_line` minus the tag loop (tags are
    /// the GROUP's, emitted once after the switch).
    fn emit_variant_leaf(&mut self, emission: &lir::ContentEmission) {
        let slot_info = emission.metadata.slot_info.clone();
        let source_location = emission.metadata.source_location.clone();
        match &emission.line {
            lir::RecognizedLine::Plain(text) => {
                let idx = self.add_line_with_hash(
                    text,
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                self.emit(Opcode::EmitLine(idx, 0));
            }
            lir::RecognizedLine::Template {
                parts: template_parts,
                slot_exprs,
            } => {
                for expr in slot_exprs {
                    self.emit_slot_expr(expr);
                }
                let idx = self.add_template_line(
                    template_parts.clone(),
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                #[expect(clippy::cast_possible_truncation)]
                self.emit(Opcode::EmitLine(idx, slot_exprs.len() as u8));
            }
        }
    }
}
