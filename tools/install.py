#!/usr/bin/env python3
"""User-local install/uninstall, preview by default; preserve modified files.

No packages, services, preferences, recordings, or compositor bindings are changed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

REPO = Path(__file__).resolve().parents[1]
MANAGED = ("bin/hyprrecorder", "share/applications/io.github.hyprrecorder.desktop",
           "share/icons/hicolor/scalable/apps/io.github.hyprrecorder.svg")
RECEIPT = "share/hyprrecorder/install.json"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular(path):
    if path.is_symlink() or (path.exists() and not path.is_file()):
        raise ValueError(f"Refusing non-regular managed path: {path}")


def execute(prefix, binary, uninstall=False, apply=False):
    prefix = Path(prefix).absolute()
    # Prefix parent components may intentionally be links (e.g. a mounted home),
    # but owned leaf files may never be links.
    receipt = prefix / RECEIPT
    regular(receipt)
    previous = json.loads(receipt.read_text()) if receipt.exists() else None
    if previous and (previous.get("version") != 1 or set(previous.get("files", {})) != set(MANAGED)):
        raise ValueError("Invalid installation receipt; nothing changed")
    for relative in MANAGED:
        path = prefix / relative
        regular(path)
        if path.exists() and (not previous or digest(path.read_bytes()) != previous["files"][relative]):
            raise ValueError(f"Managed file is unowned or modified; preserve it before proceeding: {path}")
    if uninstall:
        if not previous:
            raise ValueError("No managed installation found")
        payload = {}
    else:
        binary = Path(binary).absolute()
        regular(binary)
        version = subprocess.run([str(binary), "--version"], capture_output=True, timeout=5, check=True)
        if not version.stdout.startswith(b"hyprrecorder "):
            raise ValueError("Binary does not identify itself as Hyprrecorder")
        build = subprocess.run([str(binary), "--build-info"], capture_output=True, timeout=5, check=True)
        info = json.loads(build.stdout)
        if info.get("desktop") is not True or info.get("recorder") is not True:
            raise ValueError("Install needs a desktop build: cargo build --release --features desktop")
        target = str(prefix / MANAGED[0])
        # Desktop Entry Exec quoting is not shell quoting.
        quoted = target.replace("\\", "\\\\").replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%')
        desktop = ("[Desktop Entry]\nType=Application\nName=Hyprrecorder\n"
                   "Comment=Record a screen or region\n"
                   f'Exec="{quoted}"\nIcon=io.github.hyprrecorder\n'
                   "Terminal=false\nCategories=AudioVideo;Recorder;\nStartupNotify=true\n"
                   "StartupWMClass=io.github.hyprrecorder\n")
        payload = {MANAGED[0]: binary.read_bytes(), MANAGED[1]: desktop.encode(),
                   MANAGED[2]: (REPO / "assets/hyprrecorder.svg").read_bytes()}
    plan = {"action": "uninstall" if uninstall else "install", "prefix": str(prefix),
            "files": list(MANAGED), "apply": apply}
    if not apply:
        return plan
    # Save exact pre-change bytes/modes outside ordinary cleanup and verify them.
    backup = prefix / "share/hyprrecorder/backups" / f"{time.time_ns()}"
    backup.mkdir(parents=True, mode=0o700)
    before = {}
    for relative in (*MANAGED, RECEIPT):
        path = prefix / relative
        if path.exists():
            data = path.read_bytes()
            saved = backup / relative
            saved.parent.mkdir(parents=True, exist_ok=True)
            saved.write_bytes(data)
            saved.chmod(path.stat().st_mode & 0o777)
            if digest(saved.read_bytes()) != digest(data):
                raise ValueError("Backup verification failed; installation not started")
            before[relative] = {"sha256": digest(data), "mode": path.stat().st_mode & 0o777}
        else:
            before[relative] = None
    (backup / "before.json").write_text(json.dumps(before, indent=2))
    staged = {}
    try:
        if not uninstall:
            for relative, data in payload.items():
                path = prefix / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                fd, temp = tempfile.mkstemp(prefix=".hyprrecorder-", dir=path.parent)
                staged[relative] = Path(temp)
                with os.fdopen(fd, "wb") as stream:
                    stream.write(data)
                    stream.flush()
                    os.fsync(stream.fileno())
                staged[relative].chmod(0o755 if relative.startswith("bin/") else 0o644)
            for relative, temp in staged.items():
                os.replace(temp, prefix / relative)
            receipt.parent.mkdir(parents=True, exist_ok=True)
            new_receipt = {"version": 1, "files": {p: digest(b) for p, b in payload.items()}}
            fd, temp = tempfile.mkstemp(prefix=".receipt-", dir=receipt.parent)
            staged[RECEIPT] = Path(temp)
            with os.fdopen(fd, "w") as stream:
                json.dump(new_receipt, stream, indent=2)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temp, receipt)
        else:
            for relative in MANAGED:
                (prefix / relative).unlink(missing_ok=True)
            receipt.unlink()
    except BaseException:
        # Only restore paths validated and exclusively managed by this operation.
        for relative, state in before.items():
            path = prefix / relative
            if state is None:
                path.unlink(missing_ok=True)
            else:
                shutil.copy2(backup / relative, path)
        raise
    finally:
        for temp in staged.values():
            temp.unlink(missing_ok=True)
    plan["backup"] = str(backup)
    return plan


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prefix", type=Path, default=Path.home() / ".local")
    parser.add_argument("--binary", type=Path, default=REPO / "target/release/hyprrecorder")
    parser.add_argument("--uninstall", action="store_true")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    try:
        print(json.dumps(execute(args.prefix, args.binary, args.uninstall, args.apply), indent=2))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(2, f"hyprrecorder install: {error}\n")


if __name__ == "__main__":
    main()
