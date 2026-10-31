// Prevents an extra console window on Windows; harmless on macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The plugin scanner is this binary again, so nothing extra has to be
    // bundled: see `cantina_hosting::scan`.
    if std::env::args().nth(1).as_deref() == Some(cantina_hosting::scan::CHILD_FLAG) {
        std::process::exit(cantina_hosting::scan::child_main());
    }
    cantina_lib::run()
}
