"""The release workflow keeps the fork's Mac steps (#228).

A sync with the original project once dropped them while merging
.github/workflows/build.yml, and the Mac build on request then made the app
and uploaded nothing, which only showed after ten minutes on a Mac runner.
This fails in the ordinary checks instead.
"""

import pathlib
import re
import unittest

BUILD = pathlib.Path(__file__).resolve().parent.parent / ".github" / "workflows" / "build.yml"


def steps(text: str) -> dict:
    """Each step's name, with the text up to the next step."""
    found = {}
    parts = re.split(r"^      - name: ", text, flags=re.M)
    for part in parts[1:]:
        name, _, body = part.partition("\n")
        found[name.strip()] = body
    return found


class MacSteps(unittest.TestCase):
    def setUp(self):
        self.text = BUILD.read_text(encoding="utf-8")
        self.steps = steps(self.text)

    def test_the_app_is_zipped(self):
        body = self.steps.get("Zip macOS .app")
        self.assertIsNotNone(body, "build.yml lost the 'Zip macOS .app' step")
        self.assertIn("startsWith(matrix.os_name, 'macos')", body)
        self.assertIn("ditto -c -k", body)
        self.assertIn("MAC_ZIP=", body)

    def test_the_mac_files_reach_the_upload(self):
        body = self.steps.get("Stage artifacts (macOS)")
        self.assertIsNotNone(body, "build.yml lost the 'Stage artifacts (macOS)' step")
        self.assertIn("startsWith(matrix.os_name, 'macos')", body)
        self.assertIn('cp "$MAC_ZIP" artifacts/', body)
        self.assertIn("*.dmg", body)

    def test_staging_comes_before_the_upload(self):
        names = list(self.steps)
        for step in ("Zip macOS .app", "Stage artifacts (macOS)"):
            self.assertLess(names.index(step), names.index("Upload artifacts"), step)
        self.assertLess(names.index("Zip macOS .app"), names.index("Stage artifacts (macOS)"))

    def test_the_mac_release_job_still_wants_these_files(self):
        self.assertIn("*.app.zip", self.text)
        self.assertIn("*.dmg", self.text)


if __name__ == "__main__":
    unittest.main()
