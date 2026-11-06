import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "tools/capture_spike.py"
spec = importlib.util.spec_from_file_location("spike", SCRIPT)
spike = importlib.util.module_from_spec(spec)
spec.loader.exec_module(spike)


class CaptureSpikeTests(unittest.TestCase):
    def test_dry_run_never_creates_destination(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "new"
            result = subprocess.run([sys.executable, str(SCRIPT), "--monitor", "DP-1",
                                     "--directory", str(path)], capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(path.exists())
            self.assertIn(b"h264_vaapi", result.stdout)

    def test_existing_destination_preserved(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(spike, "preflight", return_value=[]):
            path = Path(tmp) / "capture.mkv"
            path.write_bytes(b"existing")
            with self.assertRaises(FileExistsError):
                spike.main(["--monitor", "DP-1", "--directory", tmp, "--run"])
            self.assertEqual(path.read_bytes(), b"existing")

    def test_prerequisite_failure_creates_nothing(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(spike, "preflight", return_value=["no GPU"]):
            path = Path(tmp) / "new"
            with self.assertRaisesRegex(ValueError, "no GPU"):
                spike.main(["--monitor", "DP-1", "--directory", str(path), "--run"])
            self.assertFalse(path.exists())

    def test_startup_failure_is_not_a_completed_sample(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = spike.capture(["false"], 1, Path(tmp))
            self.assertNotEqual(report["returncode"], 0)
            self.assertFalse(report["reached_duration"])

    def test_stuck_process_is_killed_and_reaped(self):
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        real_signal = child.send_signal
        real_wait = child.wait
        waits = 0

        def wait(timeout=None):
            nonlocal waits
            waits += 1
            if waits == 1:
                raise subprocess.TimeoutExpired("test", 10)
            return real_wait(timeout=2)

        try:
            with patch.object(child, "send_signal", side_effect=lambda sig:
                              None if sig == spike.signal.SIGINT else real_signal(sig)), \
                    patch.object(child, "wait", side_effect=wait):
                self.assertTrue(spike.stop(child))
            self.assertIsNotNone(child.returncode)
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=2)


if __name__ == "__main__":
    unittest.main()
