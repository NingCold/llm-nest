import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

from secure_backup import digest, node_lock, prepare, verify


class BackupTests(unittest.TestCase):
    def test_verified_copy_and_modified_bytes(self):
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root)
            data = directory / 'database.dump'
            data.write_bytes(b'private backup fixture')
            manifest = {'remote': {'restoreVerified': True}, 'files': [{'name': data.name, 'bytes': data.stat().st_size, 'sha256': digest(data)}]}
            (directory / 'manifest.json').write_text(json.dumps(manifest), encoding='utf-8')
            with contextlib.redirect_stdout(io.StringIO()):
                verify(directory)
            data.write_bytes(b'modified backup bytes!')
            with self.assertRaisesRegex(ValueError, 'checksum differs'):
                verify(directory)

    def test_running_node_lock_and_recursive_destination_are_rejected(self):
        with tempfile.TemporaryDirectory() as root:
            node = Path(root) / 'node'
            node.mkdir(); (node / 'node.lock').write_bytes(b'0')
            with node_lock(node):
                with self.assertRaises(OSError):
                    with node_lock(node):
                        self.fail('Two backup writers acquired the same node')
            with self.assertRaisesRegex(ValueError, 'outside'):
                prepare('unused', '/unused', node / 'backup', [node])
            self.assertFalse((node / 'backup').exists())

    def test_manifest_cannot_read_an_unrelated_file(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            (root / 'private').write_bytes(b'unrelated')
            directory = root / 'backup'; directory.mkdir()
            manifest = {'remote': {'restoreVerified': True}, 'files': [{'name': '../private', 'bytes': 9, 'sha256': digest(root / 'private')}]}
            (directory / 'manifest.json').write_text(json.dumps(manifest), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'outside'):
                verify(directory)


if __name__ == '__main__':
    unittest.main()
