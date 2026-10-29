import importlib.util
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
        self.temporary = tempfile.TemporaryDirectory(prefix="llmn-cleanup-test-")
        self.addCleanup(self.temporary.cleanup)
        self.repository = Path(self.temporary.name).resolve() / "repo"
        self.repository.mkdir()
        self.idle = patch.object(cleanup_module, "active_build_processes", return_value=[])
        self.idle.start()
        self.addCleanup(self.idle.stop)

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
        planned = cleanup_module.cleanup(self.repository)
        self.assertEqual(len(planned), 3)
        self.assertTrue(all(path.exists() for path in caches + retained))
        cleanup_module.cleanup(self.repository, apply=True)
        self.assertTrue(all(not path.parent.parent.exists() for path in caches))
        self.assertTrue(all(path.exists() for path in retained))

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
