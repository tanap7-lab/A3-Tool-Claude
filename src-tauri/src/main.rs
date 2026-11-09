#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The file reader runs as a child of this same exe (section 8). It exits here, before any window starts.
    let args: Vec<String> = std::env::args().collect();
    if a3_core::extract::run_child_mode(&args) {
        return;
    }
    a3_sheet_lib::run();
}
