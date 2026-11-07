# Working on this repository

## Every change goes issue → pull request → release

1. **An issue first.** Every change has an issue: an existing one, or a new one opened before the work starts. The issue says what and why; a design that grows goes in the issue, so it's kept.
2. **A pull request that closes it.** The pull request's body starts with `Closes #N` (one line per issue it finishes; `Part of #N` when it does only some of one). GitHub links them and closes the issue on merge. One pull request may close several small issues that don't touch the same code.
3. **A line in CHANGELOG.md** for what someone using the app will notice, ending with the issue's link, e.g. `([#45](https://github.com/paulm981/Submarine/issues/45))`. It goes under `## Next release` at the top. Never guess a version number: every merge to main is a release and takes the next number, so two pull requests open at once can't both know theirs.
4. **The release** is made by the merge itself (`.github/workflows/build.yml`): it's numbered with the original project's version and the fork's count, such as `v0.4.0-fork.N`, and its page lists the merged pull requests, each linking its issues. So a pull request's title is written for the person using the app, not for the code.

Changes to `.md` files only (docs, this file, the changelog) don't make a release.

When the `## Next release` heading has entries from a release that has already been made, the next pull request that adds a changelog line renames that heading to the release's number (the title bar and the Releases page show it) and starts a new `## Next release` above it.

## Fork features that don't cost sync time

Every line the fork changes inside one of the original project's files can conflict at the next sync; files only the fork has cost nothing. So ([#163](https://github.com/paulm981/Submarine/issues/163)):

- **A sync comes first.** When the original project has changes the fork doesn't, bring them in before any other work (FORK.md, "Keeping up with the original project").
- **New features go in files of the fork's own.** The original project's files get one-line hooks only, each checked by a test (`fork_hooks.rs`), so a sync that drops one fails instead of quietly losing the feature.
- **Where the original project has a feature, the fork uses theirs.** Replacing one of theirs needs the owner's OK in its issue first: the fork's Walls, in place of their Broadcast, were thousands of lines in their busiest files.
- **Every pull request says what it does to the footprint:** `scripts/upstream-footprint.sh` before and after (the lines the fork changes in the original project's files). It should only go down, or stay as it is.
- **One pull request at a time,** opened after the last one's release is out, so its changelog heading is right in its first push and the checks run once.

## Before pushing

- Rust, in `src-tauri`: `cargo test --locked` and `cargo clippy --locked --lib` with no more warnings than `.github/clippy-warnings` allows. The tests need a stand-in page: `mkdir -p dist && [ -f dist/index.html ] || echo '<!doctype html><title>Submarine</title>' > dist/index.html`.
- Frontend: `npm run typecheck` and `npm run build`.
- `src-tauri/src/lib.rs` keeps CRLF line endings on every line, as the original project has it: `grep -c $'\r$'` must equal `wc -l`. An editor or tool that writes LF there makes every line conflict at the next merge.
- Syncing with the original project: FORK.md's "Keeping up with the original project".
- The pull request template's checklist, including saying plainly when something wasn't tried in the app.

## Writing

The changelog, issues, pull requests and the app's own words are plain English for the people who use the app: short sentences, what they'll notice first, the detail after.
