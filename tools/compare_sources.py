"""Run matched SHM source probes; no encoder, media file, or backend ranking."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import signal
import subprocess
import sys


BACKENDS = ("wlr-shm", "ext-shm", "portal-shm")


def commands(binary, directory, monitor, seconds, fps):
    common = ["--seconds", str(seconds), "--fps", str(fps), "--no-cursor"]
    result = []
    for backend in BACKENDS:
        if backend == "portal-shm":
            args = [str(binary), "probe-portal-source", "--json", *common]
        else:
            args = [str(binary), "probe-source", "--capture-backend",
                    backend.split("-")[0], "--monitor", monitor,
                    "--directory", str(directory / backend), *common]
        result.append((backend, args))
    return result


def normalize(backend, report, returncode, timed_out):
    portal = backend == "portal-shm"
    frames = report.get("valid_frames" if portal else "frames", 0)
    clean = (returncode == 0 and not timed_out and not report.get("error")
             and not report.get("interrupted", False) and frames > 0)
    if portal:
        clean = (clean and report.get("cleanup_ok") is True
                 and report.get("session_started") is True
                 and report.get("remote_opened") is True
                 and report.get("rejected_buffers") == 0
                 and report.get("nonincreasing_pts") == 0)
    else:
        clean = clean and report.get("completed") is True
    return {
        "backend": backend, "clean_source_run": bool(clean),
        "returncode": returncode, "timed_out": timed_out,
        "frames": frames,
        "width": report.get("width" if portal else "source_width"),
        "height": report.get("height" if portal else "source_height"),
        "first_to_last_arrival_fps": report.get(
            "delivery_span_fps" if portal else "actual_acquisition_fps"),
        "error": report.get("error"),
    }


def run_one(backend, args, directory, timeout):
    stdout_path = directory / (backend + ".stdout.log")
    stderr_path = directory / (backend + ".stderr.log")
    timed_out = False
    with stdout_path.open("x") as out, stderr_path.open("x") as err:
        child = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=out, stderr=err)
        try:
            child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            child.send_signal(signal.SIGINT)
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        except BaseException:
            child.send_signal(signal.SIGINT)
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
            raise
    report_path = (stdout_path if backend == "portal-shm"
                   else directory / backend / "report.json")
    try:
        report = json.loads(report_path.read_text())
        if not isinstance(report, dict):
            raise ValueError("expected a JSON object")
    except (OSError, ValueError) as error:
        report = {"error": f"No usable source report: {error}"}
    return normalize(backend, report, child.returncode, timed_out)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path(__file__).resolve().parents[1]
                        / "target/debug/hyprrecorder")
    parser.add_argument("--directory", required=True, type=Path)
    parser.add_argument("--monitor", required=True)
    parser.add_argument("--seconds", type=int, choices=range(1, 61), default=10,
                        metavar="1..60")
    parser.add_argument("--fps", type=int, choices=(30, 60), default=60)
    parser.add_argument("--workload-note", default="",
                        help="Browser/workload/driver conditions; visibility is confirmed after runs")
    parser.add_argument("--run", action="store_true", help="Otherwise only print commands")
    options = parser.parse_args(argv)
    if not options.directory.is_absolute():
        parser.error("--directory must be absolute and new")
    binary = options.binary.resolve()
    plan = commands(binary, options.directory, options.monitor, options.seconds, options.fps)
    if not options.run:
        print(json.dumps(dict(plan), indent=2))
        return 0
    if not options.workload_note.strip():
        parser.error("--run requires --workload-note describing matching conditions")
    if not binary.is_file():
        parser.error("Build with --features capture-spike,portal-spike first")
    options.directory.mkdir(mode=0o700)  # Refuse existing evidence directories.
    summary = {
        "schema_version": 1, "started_utc": datetime.now(timezone.utc).isoformat(),
        "requested_monitor": options.monitor, "requested_seconds": options.seconds,
        "requested_fps": options.fps, "cursor_requested": False,
        "workload_note": options.workload_note,
        "commands": dict(plan), "runs": [],
        "visibility_acceptance": "pending user confirmation for every sample",
        "portal_monitor_acceptance": "pending user confirmation of picker choice",
        "detail": "SHM source-only sequential runs; no encoding, audio or image inspection. "
                  "Direct requests are paced; portal negotiates an upper rate. "
                  "A clean run is not a matched-workload or performance verdict.",
    }
    summary_path = options.directory / "comparison.json"
    with summary_path.open("x") as output:
        json.dump(summary, output, indent=2)
    for backend, args in plan:
        print(f"Starting {backend}; keep the same motion visible. "
              "For the portal picker choose the requested monitor.", flush=True)
        run = run_one(backend, args, options.directory, options.seconds + 90)
        summary["runs"].append(run)
        summary_path.write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps(run), flush=True)
        if not run["clean_source_run"]:
            print(f"Stopped after failed probe; evidence: {summary_path}", file=sys.stderr)
            return 2
    print(f"Evidence: {summary_path}. Confirm fixture visibility and picker monitor separately.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, KeyboardInterrupt) as error:
        print(f"Comparison stopped: {error}", file=sys.stderr)
        sys.exit(2)
