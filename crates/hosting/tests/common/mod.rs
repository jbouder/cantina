//! Shared by the hosting crate's integration tests: where the CLAP
//! fixture is, and how to dress it up as a `.clap` bundle.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The fixture dylib, next to the test binary's profile directory. Always
/// built, which is a no-op when it is current, so a `-p cantina-hosting`
/// run never tests a stale fixture.
pub fn fixture_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile_dir = exe
        .ancestors()
        .find(|p| p.join("deps").is_dir() && p.join(".fingerprint").is_dir())
        .expect("test binary lives under target/<profile>/")
        .to_path_buf();
    let path = profile_dir.join("libcantina_clap_gain.dylib");
    static BUILT: std::sync::Once = std::sync::Once::new();
    BUILT.call_once(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut command = Command::new(cargo);
        command.args(["build", "-p", "cantina-clap-gain"]);
        if profile_dir.ends_with("release") {
            command.arg("--release");
        }
        let status = command.status().expect("cargo runs");
        assert!(status.success(), "building the CLAP fixture failed");
    });
    assert!(path.exists(), "{} is missing", path.display());
    path
}

/// Copies the fixture into `dir` as `Cantina Test.clap`, the bundle
/// layout a real installation has, and returns the bundle's path.
pub fn fixture_bundle(dir: &Path) -> PathBuf {
    let bundle = dir.join("Cantina Test.clap");
    let macos = bundle.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    std::fs::copy(fixture_path(), macos.join("Cantina Test")).unwrap();
    std::fs::write(
        bundle.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>Cantina Test</string>
  <key>CFBundleIdentifier</key>
  <string>com.cantina.test-gain</string>
  <key>CFBundlePackageType</key>
  <string>BNDL</string>
</dict>
</plist>
"#,
    )
    .unwrap();
    bundle
}

/// A fresh temporary directory for one test.
pub fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cantina-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
