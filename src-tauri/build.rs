fn main() {
    // The commit this build is made from, for the update check
    // (src/about.rs). Empty outside a git checkout.
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|c| c.len() == 40 && c.chars().all(|ch| ch.is_ascii_hexdigit()))
        .unwrap_or_default();
    println!("cargo:rustc-env=SUBMARINE_GIT_SHA={}", commit);
    // Only paths that exist: cargo reruns a build script on every build when
    // one it watches is missing (a source tarball has no .git).
    for watched in ["../.git/HEAD", "../.git/refs/heads", "../.git/packed-refs"] {
        if std::path::Path::new(watched).exists() {
            println!("cargo:rerun-if-changed={}", watched);
        }
    }
    tauri_build::build()
}
