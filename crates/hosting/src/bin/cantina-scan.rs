//! The plugin scanner's child process, standalone. The shell runs the
//! same code by re-invoking its own binary with `--scan-plugins`; this
//! binary exists so the hosting crate's tests can spawn a child too.

fn main() {
    std::process::exit(cantina_hosting::scan::child_main())
}
