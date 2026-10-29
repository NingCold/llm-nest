import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "clean_build_artifacts", Path(__file__).with_name("clean-build-artifacts.py")
)
cleanup_module = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = cleanup_module
SPEC.loader.exec_module(cleanup_module)


class CleanupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = Path(tempfile.mkdtemp(prefix="llmn-cleanup-test-")).resolve()
        self.addCleanup(self.remove_temporary)
        self.repository = self.temporary / "repo"
        self.repository.mkdir()
        self.idle = patch.object(cleanup_module, "active_build_processes", return_value=[])
        self.idle.start()
        self.addCleanup(self.idle.stop)

    def remove_temporary(self):
        # Only this test's owned temporary tree; no recursive deletion API.
        files_to_remove = []
        directories_to_remove = [self.temporary]
        for root, dirs, files in os.walk(self.temporary, followlinks=False):
            for name in dirs:
                path = Path(root) / name
                cleanup_module.reject_link(path)
                directories_to_remove.append(path)
            for name in files:
                path = Path(root) / name
                cleanup_module.reject_link(path)
                files_to_remove.append(path)
        for path in files_to_remove:
            path.unlink()
        for path in sorted(directories_to_remove, key=lambda entry: -len(entry.parts)):
            self.assertFalse(any(path.iterdir()), f"Directory is not empty: {path}")
            path.rmdir()

    def file(self, relative):
        path = self.repository / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"fixture")
        return path

    def test_preview_and_apply_preserve_other_artifacts(self):
        caches = [
            self.file("target/debug/incremental/session/cache"),
            self.file("target/release/incremental/session/cache"),
            self.file("target/x86_64-pc-windows-msvc/debug/incremental/session/cache"),
        ]
        retained = [self.file(path) for path in [
            "target/debug/deps/dependency", "target/debug/build/generated",
            "target/debug/.fingerprint/fingerprint", "target/release/LLM-Nest.exe",
            "target/release/bundle/msi/package.msi", "target/.tauri/tool",
            "target/testing-work/debug/incremental/cache",
            "target/x86_64-pc-windows-msvc-work/debug/incremental/cache",
            "target/personal/debug/incremental/cache",
            "target/personal-test-data/debug/incremental/cache", "backup/original.sqlite",
        ]]
        empty = self.repository / "target/debug/incremental/empty/deep"
        empty.mkdir(parents=True)
        planned = cleanup_module.cleanup(self.repository)
        self.assertEqual(len(planned), 3)
        self.assertTrue(all(path.is_absolute() for candidate in planned
                            for path in candidate.files + candidate.directories))
        self.assertIn(empty, planned[0].directories)
        self.assertTrue(all(path.exists() for path in caches + retained))
        with patch("shutil.rmtree", side_effect=AssertionError("recursive deletion forbidden")):
            cleanup_module.cleanup(self.repository, apply=True)
        self.assertTrue(all(not path.parent.parent.exists() for path in caches))
        self.assertTrue(all(path.exists() for path in retained))

    def test_same_size_inventory_change_stops_before_removal(self):
        cache = self.file("target/debug/incremental/cache")
        original_plan = cleanup_module.plan_cleanup
        calls = 0

        def changing_plan(repository):
            nonlocal calls
            calls += 1
            if calls == 2:
                cache.rename(cache.with_name("replacement"))
            return original_plan(repository)

        with patch.object(cleanup_module, "plan_cleanup", side_effect=changing_plan):
            with self.assertRaises(cleanup_module.CleanupError):
                cleanup_module.cleanup(self.repository, apply=True)
        self.assertEqual(cache.with_name("replacement").read_bytes(), b"fixture")

    def test_external_target_rejected(self):
        external = self.repository.parent / "external"
        external.mkdir()
        with self.assertRaises(cleanup_module.CleanupError):
            cleanup_module.plan_cleanup(self.repository, external)

    def test_active_build_leaves_cache(self):
        cache = self.file("target/debug/incremental/cache")
        with patch.object(cleanup_module, "active_build_processes", return_value=["rustc"]):
            with self.assertRaises(cleanup_module.CleanupError):
                cleanup_module.cleanup(self.repository, apply=True)
        self.assertTrue(cache.exists())

    def test_build_starting_after_preview_leaves_cache(self):
        cache = self.file("target/debug/incremental/cache")
        with patch.object(cleanup_module, "active_build_processes", side_effect=[[], ["cargo"]]):
            with self.assertRaises(cleanup_module.CleanupError):
                cleanup_module.cleanup(self.repository, apply=True)
        self.assertTrue(cache.exists())

    def test_link_is_rejected_without_touching_destination(self):
        original = self.file("personal/cache")
        link = self.repository / "target/debug/incremental"
        link.parent.mkdir(parents=True)
        try:
            link.symlink_to(original.parent, target_is_directory=True)
        except OSError:
            if sys.platform != "win32":
                raise
            # Directory junction creation does not require Windows Developer Mode.
            subprocess.run(
                ["cmd", "/c", "mklink", "/J", str(link), str(original.parent)],
                check=True, capture_output=True,
            )
        self.addCleanup(link.unlink if link.is_symlink() else link.rmdir)
        with self.assertRaises(cleanup_module.CleanupError):
            cleanup_module.cleanup(self.repository, apply=True)
        self.assertEqual(original.read_bytes(), b"fixture")

    def test_nested_link_rejects_all_candidates_before_removal(self):
        cache = self.file("target/debug/incremental/cache")
        linked = self.repository / "target/release/incremental/session"
        linked.parent.mkdir(parents=True)
        destination = self.repository / "personal"
        destination.mkdir()
        try:
            linked.symlink_to(destination, target_is_directory=True)
        except OSError:
            if sys.platform != "win32":
                raise
            subprocess.run(
                ["cmd", "/c", "mklink", "/J", str(linked), str(destination)],
                check=True, capture_output=True,
            )
        self.addCleanup(linked.unlink if linked.is_symlink() else linked.rmdir)
        with self.assertRaises(cleanup_module.CleanupError):
            cleanup_module.cleanup(self.repository, apply=True)
        self.assertTrue(cache.exists())


if __name__ == "__main__":
    unittest.main()
