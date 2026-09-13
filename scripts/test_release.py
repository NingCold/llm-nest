"""Exercise release failure gates without network, installers or secrets."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_versions_and_tags_must_agree(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "Cargo.toml").write_text('[workspace.package]\nversion="0.1.0"\n')
            names = ("frontends/tauri/src-tauri/tauri.conf.json",
                     "frontends/tauri/package.json", "frontends/web/package.json")
            for name in names:
                p = root / name
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text(json.dumps({"version": "0.1.0"}))
            self.assertEqual(release.validate(root, "v0.1.0"), "0.1.0")
            for tag in ("v0.2.0", "v0.1.0-extra", "main", "v0.1.0;echo bad"):
                with self.assertRaises(ValueError):
                    release.validate(root, tag)
            (root / names[0]).write_text('{"version":"0.2.0"}')
            with self.assertRaises(ValueError):
                release.validate(root)

    def test_missing_duplicate_and_stale_artifacts_are_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            bundle = root / "bundle"
            for folder, name in (("nsis", "LLM-Nest_0.1.0_x64-setup.exe"),
                                 ("msi", "LLM-Nest_0.1.0_x64_en-US.msi")):
                (bundle / folder).mkdir(parents=True)
                (bundle / folder / name).write_bytes(b"test artifact")
            output = root / "out"
            release.collect(bundle, output, "windows-x86_64", "0.1.0")
            self.assertEqual(len(list(output.iterdir())), 3)
            self.assertIn("LLM-Nest_0.1.0_x64-setup.exe", (output / "SHA256SUMS-windows-x86_64.txt").read_text())
            with self.assertRaises(ValueError):
                release.collect(bundle, output, "windows-x86_64", "0.1.0")
            with self.assertRaises(ValueError):
                release.collect(bundle, root / "missing", "windows-x86_64", "0.2.0")
            (bundle / "nsis" / "duplicate_0.1.0_x64.exe").write_bytes(b"other")
            with self.assertRaises(ValueError):
                release.collect(bundle, root / "duplicate", "windows-x86_64", "0.1.0")

    def test_linux_release_requires_both_requested_formats(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            bundle = root / "bundle"
            (bundle / "deb").mkdir(parents=True)
            (bundle / "deb" / "LLM-Nest_0.1.0_amd64.deb").write_bytes(b"deb")
            output = root / "out"
            with self.assertRaises(ValueError):
                release.collect(bundle, output, "linux-x86_64", "0.1.0", appimage=True)
            self.assertFalse(output.exists())
            (bundle / "appimage").mkdir()
            (bundle / "appimage" / "LLM-Nest_0.1.0_amd64.AppImage").write_bytes(b"appimage")
            release.collect(bundle, output, "linux-x86_64", "0.1.0", appimage=True)
            manifest = (output / "SHA256SUMS-linux-x86_64.txt").read_text()
            self.assertIn(".deb", manifest)
            self.assertIn(".AppImage", manifest)


if __name__ == "__main__":
    unittest.main()
