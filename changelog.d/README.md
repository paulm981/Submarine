# Changelog lines waiting for their release

Each pull request with something a person using the app will notice adds one file here, named after its issue: `126.md` for issue #126 (`126-2.md` for a second one). It holds the line, or lines, as they'll appear in CHANGELOG.md:

```markdown
- **What changes, in a few words.** What the person notices first, then the detail. ([#126](https://github.com/paulm981/Submarine/issues/126))
  - an optional sub-point
```

When the merge is released, the release job moves the lines of every file here into CHANGELOG.md, under that release's number, and deletes the files (`scripts/changelog_release.py`). A file merged after the release was built waits for the next one.

A file of its own per change means two pull requests never edit the same lines, so they never conflict over the changelog.
