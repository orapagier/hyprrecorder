#!/usr/bin/env python3
"""Temporary Phase 2 external baseline, never the product recording backend."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import threading
import time


def command(args, filename):
    # Force the hardware encoder; the baseline must not silently use software.
    result = ["wl-screenrec", "--output", args.monitor, "--filename", str(filename),
              "--max-fps", str(args.fps), "--ffmpeg-encoder", "h264_vaapi",
              "--ffmpeg-muxer", "matroska"]
    if args.no_cursor:
        result.append("--no-cursor")
    return result


def preflight():
    problems = []
    for program in ("wl-screenrec", "ffprobe"):
        if not shutil.which(program):
            problems.append(f"{program} is not installed/on PATH")
    if not list(Path("/dev/dri").glob("renderD*")):
        problems.append("No render device is visible; run in a GPU-accessible session")
    if not os.environ.get("WAYLAND_DISPLAY"):
        problems.append("WAYLAND_DISPLAY is unset; run from the graphical session")
    return problems


def sample(pid):
    """Approximate recorder-only CPU seconds and RSS, excluding helpers/GPU."""
    try:
        stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return {"cpu_seconds": (int(stat[11]) + int(stat[12])) / os.sysconf("SC_CLK_TCK"),
                "rss_bytes": int(stat[21]) * os.sysconf("SC_PAGE_SIZE")}
    except (OSError, ValueError, IndexError):
        return None


def bounded_log(pipe, retained):
    with pipe:
        while chunk := pipe.read(8192):
            retained.extend(chunk[:max(0, 256 * 1024 - len(retained))])


def stop(child):
    forced = False
    if child.poll() is None:
        child.send_signal(signal.SIGINT)
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            forced = True
            child.kill()
            child.wait()
    return forced


def terminate(_signum, _frame):
    raise KeyboardInterrupt


def capture(argv, seconds, directory):
    retained = bytearray()
    samples = []
    started = time.monotonic()
    child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT)
    reader = threading.Thread(target=bounded_log, args=(child.stdout, retained), daemon=True)
    reader.start()
    previous_term = signal.signal(signal.SIGTERM, terminate)
    interrupted = False
    forced = False
    try:
        while child.poll() is None and time.monotonic() - started < seconds:
            if measurement := sample(child.pid):
                samples.append(measurement)
            time.sleep(0.1)
    except KeyboardInterrupt:
        interrupted = True
    finally:
        reached_duration = time.monotonic() - started >= seconds
        forced = stop(child)
        signal.signal(signal.SIGTERM, previous_term)
        reader.join(timeout=2)
        (directory / "recorder.log").write_bytes(retained)
    return {"command": argv, "requested_seconds": seconds,
            "elapsed_including_finalize_seconds": time.monotonic() - started,
            "interrupted": interrupted, "forced_kill": forced,
            "reached_duration": reached_duration, "returncode": child.returncode,
            "sampled_cpu_seconds": max((s["cpu_seconds"] for s in samples), default=None),
            "sampled_peak_rss_bytes": max((s["rss_bytes"] for s in samples), default=None),
            "sample_count": len(samples), "audio": False,
            "helpers_gpu_power_drops_quality": "unmeasured"}


def inspect(filename):
    result = subprocess.run(
        ["ffprobe", "-v", "error", "-count_frames", "-show_streams", "-show_format",
         "-of", "json", str(filename)], capture_output=True, timeout=30, check=False)
    if result.returncode:
        raise ValueError("ffprobe could not inspect the recording")
    info = json.loads(result.stdout)
    streams = info.get("streams", [])
    video = [s for s in streams if s.get("codec_type") == "video"]
    if len(video) != 1 or video[0].get("codec_name") != "h264":
        raise ValueError("Expected exactly one H.264 video stream")
    if int(video[0].get("nb_read_frames", "0")) <= 0:
        raise ValueError("No decodable video frames counted")
    if any(s.get("codec_type") == "audio" for s in streams):
        raise ValueError("Unexpected audio in video-only baseline")
    return info


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--monitor", required=True, help="explicit compositor output name")
    parser.add_argument("--fps", type=int, choices=(30, 60), default=30)
    parser.add_argument("--seconds", type=int, choices=range(1, 301), metavar="1..300", default=60)
    parser.add_argument("--no-cursor", action="store_true")
    parser.add_argument("--directory", required=True, type=Path, help="new private sample directory")
    parser.add_argument("--run", action="store_true", help="actually capture the selected monitor")
    args = parser.parse_args(argv)
    if not args.monitor.strip() or args.monitor.startswith("-"):
        parser.error("monitor must be a nonempty output name, not an option")
    directory = args.directory.absolute()
    filename = directory / "capture.mkv"
    argv = command(args, filename)
    if not args.run:
        print(json.dumps({"dry_run": True, "command": argv, "preflight": preflight()}, indent=2))
        return 0
    if problems := preflight():
        raise ValueError("; ".join(problems))
    # Refuse any existing directory, even an empty one. Never reuse recordings.
    directory.mkdir(mode=0o700)
    report = capture(argv, args.seconds, directory)
    report["verified_capture"] = False
    try:
        if (report["returncode"] != 0 or report["forced_kill"]
                or report["interrupted"] or not report["reached_duration"]):
            raise ValueError("Capture did not complete cleanly; partial file retained")
        report["media"] = inspect(filename)
        report["inspection"] = "H.264 frames decoded; visual/hardware acceptance still required"
    except (ValueError, OSError, subprocess.TimeoutExpired) as error:
        report["error"] = str(error)
    (directory / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Evidence: {directory / 'report.json'}")
    return 2 if "error" in report else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        print(f"capture-spike: {error}", file=sys.stderr)
        sys.exit(2)
