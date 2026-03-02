#![no_main]

use libfuzzer_sys::fuzz_target;

// Parse an index, then attempt to read each section individually.
// None of these should panic regardless of input.
fuzz_target!(|data: &[u8]| {
    let Ok(index) = brink_format::read_inkb_index(data) else {
        return;
    };

    let _ = brink_format::read_section_name_table(data, &index);
    let _ = brink_format::read_section_variables(data, &index);
    let _ = brink_format::read_section_list_defs(data, &index);
    let _ = brink_format::read_section_list_items(data, &index);
    let _ = brink_format::read_section_externals(data, &index);
    let _ = brink_format::read_section_containers(data, &index);
});
