// Prevents an extra console window on Windows in release; harmless elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// A builder failure means the shell cannot start; propagating it out of
// `main` exits non-zero and prints the error, instead of the `.expect()`
// abort this used to reach through `run` (#2415).
fn main() -> tauri::Result<()> {
    brink_desktop_lib::run()
}
