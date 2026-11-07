#!/usr/bin/env python3
"""Files a release's changelog lines under its own heading (#163).

A pull request adds its line as a file of its own, `changelog.d/<issue>.md`,
so two pull requests never change the same lines of CHANGELOG.md, and the
release's own commit never conflicts with one that's open. When a merge is
released as TAG, the release job runs this on main as it is by then:

    changelog_release.py TAG RELEASED_CHANGELOG CURRENT_CHANGELOG [FRAGMENT...]

Each FRAGMENT is a file from `changelog.d/` as it was in the released commit:
its lines go under `## TAG`, in the order given, and the job then deletes the
fragment files. Lines still written straight under `## Next release` (the way
before fragments) are filed too:

RELEASED_CHANGELOG is CHANGELOG.md as it was in the commit that was released.
The entries under its `## Next release` move from CURRENT_CHANGELOG's
`## Next release` to a new `## TAG` heading just below it. Entries added after
that commit (another pull request merged meanwhile) stay where they are, for
the next release. Nothing to move, or a TAG already there: the file is left
as it is. CURRENT_CHANGELOG is rewritten in place.
"""
import sys

NEXT = "## Next release"


def entries(section_lines):
    """The entries of a section: each top-level `- ` line with the indented
    lines under it, as blocks of text."""
    blocks, cur = [], None
    for line in section_lines:
        if line.startswith("- "):
            if cur is not None:
                blocks.append(cur)
            cur = [line]
        elif cur is not None and (line.startswith(" ") or line.startswith("\t")) and line.strip():
            cur.append(line)
        elif cur is not None and not line.strip():
            continue
        elif cur is not None:
            blocks.append(cur)
            cur = None
    if cur is not None:
        blocks.append(cur)
    return ["\n".join(b) for b in blocks]


def split(text):
    """(before, the Next release section's lines, after), or None."""
    lines = text.split("\n")
    try:
        start = lines.index(NEXT)
    except ValueError:
        return None
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")), len(lines))
    return lines[:start], lines[start + 1:end], lines[end:]


def fragment_entries(fragments):
    """The entries in fragment files' texts, in the order given."""
    out = []
    for text in fragments:
        out += entries(text.replace("\r\n", "\n").split("\n"))
    return out


def file_release(tag, released, current, fragments=()):
    """`current` with `released`'s Next release entries, and the entries of
    `fragments` (the texts of the released commit's changelog.d files), under
    `## tag`."""
    if f"## {tag}" in current.split("\n"):
        return current
    old = split(released)
    now = split(current)
    if old is None or now is None:
        return current
    shipped = entries(old[1])
    before, section, after = now
    waiting = [e for e in entries(section) if e not in shipped]
    moved = [e for e in entries(section) if e in shipped] + fragment_entries(fragments)
    if not moved:
        return current
    out = before + [NEXT, ""]
    for e in waiting:
        out += [e]
    if waiting:
        out.append("")
    out += [f"## {tag}", ""]
    for e in moved:
        out += [e]
    out.append("")
    return "\n".join(out + after)


def main():
    tag, released_path, current_path = sys.argv[1:4]
    with open(released_path, encoding="utf-8") as f:
        released = f.read()
    with open(current_path, encoding="utf-8") as f:
        current = f.read()
    fragments = []
    for path in sys.argv[4:]:
        with open(path, encoding="utf-8") as f:
            fragments.append(f.read())
    result = file_release(tag, released, current, fragments)
    if result != current:
        with open(current_path, "w", encoding="utf-8") as f:
            f.write(result)
        print(f"Filed under {tag}.")
    else:
        print("Nothing to file.")


if __name__ == "__main__":
    main()
