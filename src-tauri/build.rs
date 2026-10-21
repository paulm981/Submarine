/// Set to something: a workflow sets the one it doesn't use to "".
fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

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
        .filter(|_| !env_set("SUBMARINE_RELEASE"))
        .unwrap_or_default();
    println!("cargo:rustc-env=SUBMARINE_GIT_SHA={}", commit);
    // What kind of build, for the update check: "release" (every merge to
    // main is one, v0.4.0-fork.N, built by the workflow) or "source" (built
    // from a clone, updated by pull and rebuild).
    let kind = if env_set("SUBMARINE_RELEASE") { "release" } else { "source" };
    println!("cargo:rustc-env=SUBMARINE_BUILD_KIND={}", kind);
    // The version to show and compare (about.rs): the workflow's whole
    // release version, such as "0.4.0-fork.1". The Windows installer is
    // stamped "0.4.0", since its version can't carry "-fork.1", so Cargo's
    // version alone would make that release look older than itself. Built
    // from source, it's the newest fork release in the clone's history, and
    // otherwise Cargo's.
    println!("cargo:rerun-if-env-changed=SUBMARINE_VERSION");
    let shape = |v: &String| {
        v.starts_with(|c: char| c.is_ascii_digit()) && v.chars().all(|c| c.is_ascii_alphanumeric() || ".+-".contains(c))
    };
    let version = std::env::var("SUBMARINE_VERSION")
        .ok()
        .filter(shape)
        .or_else(|| {
            git(&["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*-fork.[0-9]*"])
                .map(|t| t.trim_start_matches('v').to_string())
                .filter(shape)
        })
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env=SUBMARINE_VERSION={}", version);
    // Rebuild when HEAD moves, or a release is tagged. Asked of git rather than assumed to be under
    // ../.git, which is a file in a worktree or a submodule. Only paths that
    // exist: cargo reruns a build script on every build when one it watches
    // is missing.
    for name in ["HEAD", "refs/heads", "refs/tags", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", name]) {
            if std::path::Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={}", path);
            }
        }
    }
    tauri_build::build()
}
