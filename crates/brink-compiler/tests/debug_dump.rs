#![allow(clippy::unwrap_used, clippy::panic, clippy::print_stderr)]

use serde_json::Value;

fn dump_json(name: &str, source: &str) {
    match brink_compiler::compile_string_to_json(source) {
        Ok(j) => {
            let v: Value = serde_json::to_value(&j).unwrap();
            eprintln!("=== {name} JSON ===");
            eprintln!("{}", serde_json::to_string_pretty(&v).unwrap());
        }
        Err(e) => {
            eprintln!("=== {name}: COMPILE ERROR: {e}");
        }
    }
}

#[test]
fn dump_cases() {
    dump_json("I082: choice -> DONE", "* choice -> DONE\n");
    eprintln!();
    dump_json("I084: sticky", "+ sticky\n");
    eprintln!();
    dump_json("I086: gather", "* hello\n- gather\n");
}
