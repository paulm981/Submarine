fn main() {
    // The commit this build is made from, for the update check
    // (src/about.rs). Empty outside a git checkout.
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    // A release build (SUBMARINE_RELEASE set by the workflow that publishes
    // it) leaves it empty: its users get new versions as releases, not by
    // pulling commits and rebuilding.
    println!("cargo:rerun-if-env-changed=SUBMARINE_RELEASE");
    let commit = git(&["rev-parse", "HEAD"])
        .filter(|c| c.len() == 40 && c.chars().all(|ch| ch.is_ascii_hexdigit()))
        .filter(|_| std::env::var_os("SUBMARINE_RELEASE").is_none())
        .unwrap_or_default();
    println!("cargo:rustc-env=SUBMARINE_GIT_SHA={}", commit);
    // Rebuild when HEAD moves. Asked of git rather than assumed to be under
    // ../.git, which is a file in a worktree or a submodule. Only paths that
    // exist: cargo reruns a build script on every build when one it watches
    // is missing.
    for name in ["HEAD", "refs/heads", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name]) {
            if std::path::Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={}", path);
            }
        }
    }
    tauri_build::build()
}
