"""Run with HYPRRECORDER_TEST_BINARY pointing to a desktop build; no live capture."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


BINARY = os.environ.get("HYPRRECORDER_TEST_BINARY")


@unittest.skipUnless(BINARY, "set HYPRRECORDER_TEST_BINARY to a desktop build")
class RegionWindowTests(unittest.TestCase):
    def run_case(self, case, script=None):
        with tempfile.TemporaryDirectory(prefix="hyprrecorder-region-") as directory:
            root = Path(directory)
            if script is not None:
                selector = root / "slurp"
                selector.write_text(f"#!{sys.executable}\n" + script)
                selector.chmod(0o700)
            env = dict(os.environ, PATH=str(root), XDG_RUNTIME_DIR=str(root),
                       XDG_CONFIG_HOME=str(root / "config"), QT_QPA_PLATFORM="offscreen",
                       QT_QPA_PLATFORMTHEME="", QT_FORCE_STDERR_LOGGING="1",
                       QT_LOGGING_TO_CONSOLE="1", HYPRRECORDER_SMOKE_REGION_CASE=case,
                       WAYLAND_DISPLAY="/nonexistent/hyprrecorder-region-test")
            env.pop("WAYLAND_SOCKET", None)
            env.pop("HYPRRECORDER_SMOKE_SCREENSHOTS", None)
            result = subprocess.run([BINARY, "ui", "--smoke"], env=env,
                                    capture_output=True, text=True, timeout=8)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn(f"Region smoke passed: {case}\n", result.stdout,
                          "binary did not run the region regression scenario")
            self.assertFalse((root / "config").exists(), "selection must not save preferences")

    def test_selector_receives_eof_and_returns_selected_region(self):
        self.run_case("success", "import sys\nsys.stdin.read()\nprint('10 20 300 200')\n")

    def test_escape_restores_window_and_keeps_previous_region(self):
        self.run_case("cancel", "import sys\nsys.stdin.read()\nsys.exit(1)\n")

    def test_missing_selector_restores_window(self):
        self.run_case("missing")

    def test_invalid_region_restores_window_and_keeps_previous_region(self):
        for output in ["not coordinates", "10 20 0 200", "10 20 300 -1"]:
            with self.subTest(output=output):
                self.run_case("invalid", f"print({output!r})\n")

    def test_crashed_selector_restores_window(self):
        self.run_case("crash", "import os, signal\nos.kill(os.getpid(), signal.SIGKILL)\n")

    def test_selector_error_is_shown(self):
        self.run_case("failure", "import sys\nprint('test display failure', file=sys.stderr)\nsys.exit(1)\n")

    def test_reopen_cancels_pending_start(self):
        self.run_case("pending", "print('10 20 300 200')\n")

    def test_reopen_cancels_running_selection(self):
        self.run_case("reopen", "import time\nprint('ready', flush=True)\ntime.sleep(10)\n")

    def test_reopen_recovers_even_if_selector_ignores_termination(self):
        self.run_case("stubborn", "import signal, time\nsignal.signal(signal.SIGTERM, signal.SIG_IGN)\nprint('ready', flush=True)\ntime.sleep(10)\n")


if __name__ == "__main__":
    unittest.main()
