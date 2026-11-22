"""Behaviour tests for design/gen-swift-theme.py: they run the generator as CI does, on the real tokens and on
edited copies, and check what it produces and when it refuses. Run: python3 -I -m unittest discover -s design
"""
import pathlib
import re
import subprocess
import sys
import tempfile
import unittest

DESIGN = pathlib.Path(__file__).resolve().parent
SCRIPT = DESIGN / "gen-swift-theme.py"
TOKENS = DESIGN / "tokens.css"
COMMITTED = DESIGN.parent / "apps/ios/Shared/Sources/Shared/Theme/Tokens.generated.swift"


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, "-I", str(SCRIPT), *args], capture_output=True, text=True)


class Generator(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def generate(self, css: str) -> subprocess.CompletedProcess:
        tokens = self.dir / "tokens.css"
        tokens.write_text(css)
        return run("--tokens", str(tokens), "--out", str(self.dir / "out.swift"))

    def swift(self) -> str:
        return (self.dir / "out.swift").read_text()

    def test_the_committed_theme_is_what_the_tokens_produce(self):
        done = run("--check")
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertTrue(COMMITTED.exists())

    def test_it_emits_every_light_and_dark_color_pair(self):
        css = TOKENS.read_text()
        self.assertEqual(self.generate(css).returncode, 0)
        out = self.swift()
        self.assertIn("light: FLRGBA(red: 59, green: 90, blue: 254, alpha: 1)", out)  # --color-primary
        self.assertIn("dark: FLRGBA(red: 159, green: 176, blue: 255, alpha: 1)", out)  # dark --color-primary
        self.assertEqual(len(re.findall(r"= flDynamicColor\(", out)), 24)

    def test_it_converts_units_and_keeps_the_ios_touch_target(self):
        self.assertEqual(self.generate(TOKENS.read_text()).returncode, 0)
        out = self.swift()
        self.assertIn("static let s4: CGFloat = 16\n", out)  # 1 rem
        self.assertIn("static let controlHeightLg: CGFloat = 44\n", out)
        self.assertIn("static let base: TimeInterval = 0.2\n", out)  # 200 ms in seconds
        self.assertIn("static let standard = FLCubicBezier(0.2, 0, 0, 1)\n", out)
        self.assertIn("static let wide: CGFloat = 0.04\n", out)

    def test_a_changed_token_changes_the_output_and_fails_the_check(self):
        css = TOKENS.read_text().replace("--color-primary: #3b5afe;", "--color-primary: #112233;")
        tokens, out = self.dir / "tokens.css", self.dir / "out.swift"
        tokens.write_text(TOKENS.read_text())
        self.assertEqual(run("--tokens", str(tokens), "--out", str(out)).returncode, 0)
        self.assertEqual(run("--check", "--tokens", str(tokens), "--out", str(out)).returncode, 0)
        tokens.write_text(css)
        drift = run("--check", "--tokens", str(tokens), "--out", str(out))
        self.assertEqual(drift.returncode, 1)
        self.assertIn("out of date", drift.stderr)
        self.assertEqual(run("--tokens", str(tokens), "--out", str(out)).returncode, 0)
        self.assertIn("red: 17, green: 34, blue: 51", out.read_text())
        self.assertEqual(run("--check", "--tokens", str(tokens), "--out", str(out)).returncode, 0)

    def test_a_missing_generated_file_fails_the_check(self):
        done = run("--check", "--out", str(self.dir / "missing.swift"))
        self.assertEqual(done.returncode, 1)

    def test_a_missing_token_is_an_error_not_a_gap(self):
        done = self.generate(TOKENS.read_text().replace("--color-agent:", "--color-renamed-agent:", 1))
        self.assertEqual(done.returncode, 2)
        self.assertIn("--color-agent", done.stderr)

    def test_dark_palettes_that_disagree_are_refused(self):
        css = TOKENS.read_text().replace("--color-bg: #121114;", "--color-bg: #000000;", 1)
        done = self.generate(css)
        self.assertEqual(done.returncode, 2)
        self.assertIn("dark", done.stderr)

    def test_reduced_motion_must_zero_every_duration(self):
        css = re.sub(r"(prefers-reduced-motion: reduce\).*?--duration-base:) 0ms", r"\1 90ms", TOKENS.read_text(), flags=re.S)
        done = self.generate(css)
        self.assertEqual(done.returncode, 2)
        self.assertIn("duration-base", done.stderr)

    def test_an_unreadable_value_names_the_token(self):
        done = self.generate(TOKENS.read_text().replace("--space-4: 1rem;", "--space-4: wide;"))
        self.assertEqual(done.returncode, 2)
        self.assertIn("4", done.stderr)


if __name__ == "__main__":
    unittest.main()
