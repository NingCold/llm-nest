"""Client Web artifact checks without Cargo or network."""
import importlib.util
import hashlib
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
spec = importlib.util.spec_from_file_location('package_web', Path(__file__).with_name('package-web.py'))
web = importlib.util.module_from_spec(spec)
spec.loader.exec_module(web)
class ArtifactTests(unittest.TestCase):
    def test_web_package_contains_versioned_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dist = root / 'dist'
            dist.mkdir()
            (dist / 'index.html').write_text('fixture')
            with self.assertRaisesRegex(ValueError, 'THIRD_PARTY_NOTICES'):
                web.package(dist, root / 'missing', '1.2.3')
            (dist / 'THIRD_PARTY_NOTICES.txt').write_text('upstream notices')
            (dist / 'LICENSE.txt').write_text('client license')
            (dist / 'NOTICE.txt').write_text('client attribution')
            artifact = web.package(dist, root / 'out', '1.2.3')
            manifest = json.loads((artifact.parent / 'web-manifest.json').read_text())
            self.assertEqual(manifest['webVersion'], '1.2.3')
            self.assertEqual(manifest['sha256'], hashlib.sha256(artifact.read_bytes()).hexdigest())
            with tarfile.open(artifact) as archive:
                self.assertEqual(archive.extractfile('index.html').read(), b'fixture')
                self.assertEqual(archive.extractfile('THIRD_PARTY_NOTICES.txt').read(), b'upstream notices')
                self.assertEqual(archive.extractfile('LICENSE.txt').read(), b'client license')
                self.assertEqual(archive.extractfile('NOTICE.txt').read(), b'client attribution')
if __name__ == '__main__':
    unittest.main()
