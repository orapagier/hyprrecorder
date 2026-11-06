import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("installer", Path(__file__).resolve().parents[1] / "tools/install.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class Installation(unittest.TestCase):
    def test_preview_install_modified_file_protection_and_uninstall(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            binary = root / "fake binary"
            binary.write_text("#!/bin/sh\nif [ \"$1\" = --build-info ]; then printf '%s\\n' '{\"desktop\":true,\"recorder\":true}'; else printf 'hyprrecorder test\\n'; fi\n")
            binary.chmod(0o755)
            prefix = root / "prefix with spaces"
            installer.execute(prefix, binary)
            self.assertFalse(prefix.exists())
            result = installer.execute(prefix, binary, apply=True)
            self.assertTrue(Path(result["backup"]).is_dir())
            desktop = prefix / installer.MANAGED[1]
            self.assertIn(f'Exec="{prefix}/bin/hyprrecorder"', desktop.read_text())
            original = desktop.read_bytes()
            desktop.write_bytes(b"user customization")
            with self.assertRaises(ValueError):
                installer.execute(prefix, binary, uninstall=True, apply=True)
            self.assertTrue((prefix / installer.MANAGED[0]).exists())
            desktop.write_bytes(original)
            installer.execute(prefix, binary, apply=True)  # managed update
            unrelated = prefix / "keep.txt"
            unrelated.write_text("preserved")
            installer.execute(prefix, binary, uninstall=True, apply=True)
            self.assertTrue(unrelated.exists())
            self.assertTrue((prefix / "share/hyprrecorder/backups").exists())
            for relative in installer.MANAGED:
                self.assertFalse((prefix / relative).exists())

    def test_unowned_file_or_symlink_is_never_replaced(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            prefix = root / "prefix"
            (prefix / "bin").mkdir(parents=True)
            (prefix / "bin/hyprrecorder").write_text("existing")
            with self.assertRaises(ValueError):
                installer.execute(prefix, root / "not-needed", apply=True)
            (prefix / "bin/hyprrecorder").unlink()
            target = root / "target"
            target.write_text("preserve")
            (prefix / "bin/hyprrecorder").symlink_to(target)
            with self.assertRaises(ValueError):
                installer.execute(prefix, root / "not-needed", apply=True)
            self.assertEqual(target.read_text(), "preserve")


if __name__ == "__main__":
    unittest.main()
