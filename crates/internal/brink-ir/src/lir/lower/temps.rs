use crate::hir;

use super::context::TempMap;

/// Allocate temp slots for a scope (knot/function).
///
/// Params occupy slots `0..n-1` (knot params first, then each stitch's
/// params in order), then temp declarations are assigned sequentially.
#[expect(
    clippy::cast_possible_truncation,
    reason = "ink functions won't have >u16::MAX params/temps"
)]
pub fn alloc_temps(
    params: &[hir::Param],
    stitches: &[hir::Stitch],
    blocks: &[&hir::Block],
) -> TempMap {
    let mut map = TempMap::new();

    // Knot params first
    for (i, param) in params.iter().enumerate() {
        map.insert(param.name.text.clone(), i as u16);
    }

    let mut next_slot = params.len() as u16;

    // Stitch params get sequential slots after knot params
    for stitch in stitches {
        for param in &stitch.params {
            if map.get(&param.name.text).is_none() {
                map.insert(param.name.text.clone(), next_slot);
                next_slot += 1;
            }
        }
    }

    // Walk all blocks in the scope to find TempDecl
    for block in blocks {
        collect_temps_from_block(block, &mut map, &mut next_slot);
    }

    map
}

fn collect_temps_from_block(block: &hir::Block, map: &mut TempMap, next_slot: &mut u16) {
    for stmt in &block.stmts {
        collect_temps_from_stmt(stmt, map, next_slot);
    }
}

fn collect_temps_from_stmt(stmt: &hir::Stmt, map: &mut TempMap, next_slot: &mut u16) {
    match stmt {
        hir::Stmt::TempDecl(decl) => {
            if map.get(&decl.name.text).is_none() {
                map.insert(decl.name.text.clone(), *next_slot);
                *next_slot += 1;
            }
        }
        hir::Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                collect_temps_from_block(&choice.body, map, next_slot);
            }
            collect_temps_from_block(&cs.continuation, map, next_slot);
        }
        hir::Stmt::Conditional(cond) => {
            for branch in &cond.branches {
                collect_temps_from_block(&branch.body, map, next_slot);
            }
        }
        hir::Stmt::Sequence(seq) => {
            for branch in &seq.branches {
                collect_temps_from_block(branch, map, next_slot);
            }
        }
        hir::Stmt::LabeledBlock(block) => {
            collect_temps_from_block(block, map, next_slot);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::*;
    use rowan::TextRange;

    fn dummy_name(s: &str) -> Name {
        Name {
            text: s.to_string(),
            range: TextRange::default(),
        }
    }

    #[test]
    fn params_occupy_first_slots() {
        let params = vec![
            Param {
                name: dummy_name("a"),
                is_ref: false,
                is_divert: false,
            },
            Param {
                name: dummy_name("b"),
                is_ref: false,
                is_divert: false,
            },
        ];
        let empty_block = Block::default();
        let map = alloc_temps(&params, &[], &[&empty_block]);
        assert_eq!(map.get("a"), Some(0));
        assert_eq!(map.get("b"), Some(1));
        assert_eq!(map.total_slots(), 2);
    }
}
