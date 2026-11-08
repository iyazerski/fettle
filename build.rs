//! Compile the Swift bridge to Apple's on-device model (`swift/on_device.swift`)
//! into a static library, and link it with the Swift runtime macOS ships.

use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCE: &str = "swift/on_device.swift";
/// Fettle's oldest macOS, as `LSMinimumSystemVersion` in Info.plist says.
const TARGET: &str = "arm64-apple-macos13.0";

fn main() {
    println!("cargo:rerun-if-changed={SOURCE}");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let lib = out.join("libfettle_on_device.a");
    let mut swiftc = Command::new("xcrun");
    swiftc
        .args(["swiftc", "-parse-as-library", "-emit-library", "-static"])
        .args(["-module-name", "FettleOnDevice", "-target", TARGET])
        .args(["-swift-version", "5", "-O", SOURCE, "-o"])
        .arg(&lib);
    run(&mut swiftc);

    let sdk = run(Command::new("xcrun").arg("--show-sdk-path"));
    let swiftc = PathBuf::from(run(Command::new("xcrun").args(["--find", "swiftc"])));
    // The toolchain's static compatibility libraries, which Swift code built
    // for older macOS links against.
    let toolchain = swiftc
        .parent()
        .and_then(Path::parent)
        .expect("swiftc lives in <toolchain>/usr/bin")
        .join("lib/swift/macosx");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=fettle_on_device");
    println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
    println!("cargo:rustc-link-search=native={}", toolchain.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    // Before macOS 26 the framework doesn't exist; a weak link lets Fettle start anyway.
    println!("cargo:rustc-link-arg=-Wl,-weak_framework,FoundationModels");
}

/// Run `command`, failing the build if it fails, and return its trimmed output.
fn run(command: &mut Command) -> String {
    let output = command
        .output()
        .unwrap_or_else(|e| panic!("running {command:?}: {e}"));
    assert!(
        output.status.success(),
        "{command:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
