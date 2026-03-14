#![allow(clippy::expect_used, clippy::print_stderr)]

//! Generate an XLIFF file from a `.inkb` binary.
//!
//! Usage: `cargo run -p brink-intl --example generate_xliff -- <input.inkb> <output.xlf>`

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <input.inkb> <output.xlf>", args[0]);
        std::process::exit(1);
    }

    let inkb_bytes = std::fs::read(&args[1]).expect("failed to read .inkb");
    let index = brink_format::read_inkb_index(&inkb_bytes).expect("failed to read inkb index");
    let data = brink_format::read_inkb(&inkb_bytes).expect("failed to decode inkb");

    let doc = brink_intl::generate_locale(&data, index.checksum, "en");
    let xml = xliff2::write::to_string(&doc).expect("failed to serialize XLIFF");

    std::fs::write(&args[2], xml).expect("failed to write output");
    eprintln!("Wrote {}", args[2]);
}
