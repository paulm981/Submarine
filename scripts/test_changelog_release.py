"""Tests for changelog_release.py: python3 -m unittest scripts/test_changelog_release.py"""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))
from changelog_release import file_release  # noqa: E402

HEAD = "# Changelog\n\nIntro.\n\n"
OLD = "## v0.4.0-fork.32\n\n- Old thing. ([#1](x))\n"


class FileRelease(unittest.TestCase):
    def test_the_released_lines_go_under_the_tag(self):
        text = HEAD + "## Next release\n\n- **A.** Detail. ([#2](x))\n  - a sub-point\n\n" + OLD
        got = file_release("v0.4.0-fork.33", text, text)
        self.assertEqual(
            got,
            HEAD + "## Next release\n\n## v0.4.0-fork.33\n\n- **A.** Detail. ([#2](x))\n  - a sub-point\n\n" + OLD,
        )

    def test_a_line_merged_after_the_release_stays_for_the_next(self):
        released = HEAD + "## Next release\n\n- A. ([#2](x))\n\n" + OLD
        now = HEAD + "## Next release\n\n- B. ([#3](x))\n- A. ([#2](x))\n\n" + OLD
        got = file_release("v0.4.0-fork.33", released, now)
        self.assertEqual(got, HEAD + "## Next release\n\n- B. ([#3](x))\n\n## v0.4.0-fork.33\n\n- A. ([#2](x))\n\n" + OLD)

    def test_nothing_to_file_leaves_it(self):
        text = HEAD + "## Next release\n\n" + OLD
        self.assertEqual(file_release("v0.4.0-fork.33", text, text), text)

    def test_run_twice_files_once(self):
        text = HEAD + "## Next release\n\n- A. ([#2](x))\n\n" + OLD
        once = file_release("v0.4.0-fork.33", text, text)
        self.assertEqual(file_release("v0.4.0-fork.33", text, once), once)

    def test_the_real_changelog_files_cleanly(self):
        path = os.path.join(os.path.dirname(__file__), "..", "CHANGELOG.md")
        with open(path, encoding="utf-8") as f:
            text = f.read()
        # Whatever waits under Next release now, plus one line of our own, so
        # there's always something to file.
        text = text.replace("## Next release\n\n", "## Next release\n\n- **Test.** A line. ([#0](x))\n", 1)
        got = file_release("v9.9.9-test", text, text)
        self.assertIn("## Next release\n\n## v9.9.9-test\n\n- **Test.** A line.", got)
        self.assertEqual(got.count("## v9.9.9-test"), 1)
        # Nothing lost or added but the heading (blank lines aside: the
        # filed section gets one before the next heading).
        lines = lambda t: sorted(l for l in t.split("\n") if l.strip())
        self.assertEqual(lines(got.replace("## v9.9.9-test\n", "")), lines(text))


if __name__ == "__main__":
    unittest.main()
