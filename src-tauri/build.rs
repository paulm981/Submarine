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
    // What kind of build, for the update check: "release" (a numbered
    // release), "main" (the workflow's build of each commit on main, which
    // its users replace by downloading the next one from the "Latest build"
    // release) or "source" (built from a clone, updated by pull and rebuild).
    println!("cargo:rerun-if-env-changed=SUBMARINE_BUILD");
    let kind = if env_set("SUBMARINE_RELEASE") {
        "release"
    } else if std::env::var("SUBMARINE_BUILD").as_deref() == Ok("main") {
        "main"
    } else {
        "source"
    };
    println!("cargo:rustc-env=SUBMARINE_BUILD_KIND={}", kind);
    // The fork's build number: how many commits main's own line holds
    // (`--first-parent`, so a merged pull request counts once). It only ever
    // grows, so a higher number is a newer build, and the workflow names the
    // Latest build release with the same count (about.rs compares the two).
    // Empty when it can't be counted: outside a git checkout, or in a
    // shallow clone, whose count would be far too low.
    println!("cargo:rerun-if-env-changed=SUBMARINE_BUILD_NUMBER");
    let digits = |n: &String| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit());
    let number = std::env::var("SUBMARINE_BUILD_NUMBER")
        .ok()
        .filter(digits)
        .or_else(|| {
            if git(&["rev-parse", "--is-shallow-repository"]).as_deref() != Some("false") {
                return None;
            }
            git(&["rev-list", "--count", "--first-parent", "HEAD"]).filter(digits)
        })
        .unwrap_or_default();
    println!("cargo:rustc-env=SUBMARINE_BUILD_NUMBER={}", number);
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
