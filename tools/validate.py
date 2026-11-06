#!/usr/bin/env python3
"""Repeat offline checks and retain bounded local evidence. Never captures media.

Use --session to include the Unix listener test in an unrestricted desktop terminal.
Real recording/quality/audio acceptance remains in docs/release-checklist.md.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

REPO = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report-directory", type=Path)
    parser.add_argument("--session", action="store_true")
    args = parser.parse_args()
    if args.report_directory:
        root = args.report_directory.absolute()
        root.mkdir(mode=0o700)  # Existing destinations are never overwritten.
    else:
        root = Path(tempfile.mkdtemp(prefix="hyprrecorder-validation-"))
    commands = [
        ["cargo", "fmt", "--check"],
        ["cargo", "clippy", "--offline", "--locked", "--all-targets", "--", "-D", "warnings"],
        ["cargo", "clippy", "--offline", "--locked", "--all-features", "--all-targets", "--", "-D", "warnings"],
        ["cargo", "test", "--offline", "--locked"],
        ["cargo", "test", "--offline", "--locked", "--all-features"],
        ["python3", "-m", "unittest", "discover", "-s", "tests", "-p", "test_*.py", "-v"],
        ["cargo", "build", "--offline", "--locked", "--release", "--features", "desktop"],
    ]
    if args.session:
        commands.append(["cargo", "test", "--offline", "--locked", "--all-features", "--", "--ignored"])
    os.environ["PYTHONDONTWRITEBYTECODE"] = "1"
    results = []
    for index, command in enumerate(commands):
        print("Running:", " ".join(command), flush=True)
        started = time.monotonic()
        with (root / f"{index:02d}.log").open("wb") as log:
            try:
                process = subprocess.run(command, cwd=REPO, stdout=log, stderr=subprocess.STDOUT, timeout=600)
                code = process.returncode
            except (OSError, subprocess.TimeoutExpired) as error:
                log.write(str(error).encode())
                code = -1
        results.append({"command": command, "returncode": code,
                        "seconds": round(time.monotonic() - started, 3), "log": f"{index:02d}.log"})
        (root / "checks.json").write_text(json.dumps(results, indent=2))
        if code:
            print(f"Failed; inspect {root / f'{index:02d}.log'}")
            return 1
    metadata = subprocess.run(["cargo", "metadata", "--offline", "--locked", "--all-features", "--format-version", "1"],
                              cwd=REPO, capture_output=True, check=True, timeout=60)
    packages = json.loads(metadata.stdout)["packages"]
    (root / "rust-licenses.json").write_text(json.dumps(
        [{"name": p["name"], "version": p["version"], "license": p["license"]} for p in packages], indent=2))
    native = {}
    for name in ["Qt6Widgets", "Qt6Network", "libavcodec", "libavformat", "libavutil", "libavfilter", "gbm", "libdrm", "libpulse"]:
        p = subprocess.run(["pkg-config", "--modversion", name], capture_output=True, text=True, timeout=5)
        native[name] = p.stdout.strip() if p.returncode == 0 else "unavailable"
    (root / "native-versions.json").write_text(json.dumps(native, indent=2))
    print(f"Offline checks passed. Evidence: {root}")
    print("Real GPU capture, audio sync, window invisibility and distro acceptance remain separate checks.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
