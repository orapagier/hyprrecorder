import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "tools/compare_sources.py"
spec = importlib.util.spec_from_file_location("compare", SCRIPT)
compare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare)


class CompareSourcesTests(unittest.TestCase):
    def test_successful_sequence_preserves_raw_reports_and_pending_acceptance(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / "fake-recorder"
            binary.write_text("#!" + sys.executable + "\n" + '''
import json, pathlib, sys
args = sys.argv[1:]
if args[0] == 'probe-portal-source':
    print(json.dumps(dict(valid_frames=300, cleanup_ok=True, session_started=True,
                         remote_opened=True, rejected_buffers=0, nonincreasing_pts=0,
                         delivery_span_fps=29.9, width=1920, height=1080)))
else:
    directory = pathlib.Path(args[args.index('--directory') + 1])
    directory.mkdir()
    (directory / 'report.json').write_text(json.dumps(dict(frames=600, completed=True,
        actual_acquisition_fps=59.9, source_width=1920, source_height=1080)))
''')
            binary.chmod(0o700)
            destination = Path(tmp) / "new"
            code = compare.main(["--monitor", "DP-1", "--directory", str(destination),
                                 "--binary", str(binary), "--workload-note", "synthetic test", "--run"])
            self.assertEqual(code, 0)
            report = json.loads((destination / "comparison.json").read_text())
            self.assertEqual(len(report["runs"]), 3)
            self.assertTrue(all(run["clean_source_run"] for run in report["runs"]))
            self.assertIn("pending", report["visibility_acceptance"])
            self.assertTrue((destination / "portal-shm.stdout.log").is_file())
            self.assertTrue((destination / "ext-shm/report.json").is_file())

    def test_dry_run_creates_nothing_and_matches_settings(self):
        with tempfile.TemporaryDirectory() as tmp:
            destination = Path(tmp) / "new"
            result = subprocess.run([sys.executable, str(SCRIPT), "--monitor", "DP-1",
                                     "--directory", str(destination)], capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            plan = json.loads(result.stdout)
            self.assertEqual(tuple(plan), compare.BACKENDS)
            for args in plan.values():
                self.assertIn("--no-cursor", args)
                self.assertEqual(args[args.index("--seconds") + 1], "10")
                self.assertEqual(args[args.index("--fps") + 1], "60")
            self.assertFalse(destination.exists())

    def test_failed_probe_keeps_evidence_and_stops_sequence(self):
        with tempfile.TemporaryDirectory() as tmp:
            destination = Path(tmp) / "new"
            with patch.object(compare, "run_one", return_value={"clean_source_run": False}) as run:
                code = compare.main(["--monitor", "DP-1", "--directory", str(destination),
                                     "--binary", sys.executable, "--workload-note", "fixture", "--run"])
            self.assertEqual(code, 2)
            self.assertEqual(run.call_count, 1)
            self.assertEqual(len(json.loads((destination / "comparison.json").read_text())["runs"]), 1)
            with self.assertRaises(FileExistsError):
                compare.main(["--monitor", "DP-1", "--directory", str(destination),
                              "--binary", sys.executable, "--workload-note", "fixture", "--run"])

    def test_timeout_reaps_process_and_rejects_sample(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = compare.run_one("portal-shm", [sys.executable, "-c", "import time; time.sleep(60)"],
                                     Path(tmp), 0.1)
            self.assertTrue(result["timed_out"])
            self.assertFalse(result["clean_source_run"])
            self.assertIsNotNone(result["returncode"])

    def test_report_failure_and_missing_report_are_not_success(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = compare.run_one("wlr-shm", [sys.executable, "-c", "pass"], Path(tmp), 2)
            self.assertFalse(result["clean_source_run"])
            self.assertIn("No usable source report", result["error"])
        report = {"valid_frames": 230, "cleanup_ok": True, "session_started": True,
                  "remote_opened": True, "rejected_buffers": 0, "nonincreasing_pts": 0,
                  "delivery_span_fps": 22.8, "width": 1920, "height": 1080}
        self.assertTrue(compare.normalize("portal-shm", report, 0, False)["clean_source_run"])
        for field, value in [("error", "disconnected"), ("cleanup_ok", False),
                             ("interrupted", True), ("rejected_buffers", 1)]:
            self.assertFalse(compare.normalize("portal-shm", {**report, field: value},
                                               0, False)["clean_source_run"])


if __name__ == "__main__":
    unittest.main()
