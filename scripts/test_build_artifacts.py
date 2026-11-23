"""Client Web artifact checks without Cargo or network."""
import importlib.util
import hashlib
import json
from pathlib import Path
import tarfile
import tomllib
import zipfile
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


class SourceLicenseTests(unittest.TestCase):
    def test_project_package_metadata_uses_apache_2(self):
        root = Path(__file__).resolve().parent.parent
        workspace = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))
        self.assertEqual(workspace['workspace']['package']['license'], 'Apache-2.0')
        desktop = tomllib.loads((root / 'desktop/src-tauri/Cargo.toml').read_text(encoding='utf-8'))
        self.assertTrue(desktop['package']['license']['workspace'])
        for name in ['web', 'desktop']:
            manifest = json.loads((root / name / 'package.json').read_text(encoding='utf-8'))
            self.assertEqual(manifest['license'], 'Apache-2.0')

    def test_desktop_native_clarifications_keep_dpi_both_licenses(self):
        root = Path(__file__).resolve().parent.parent
        config = tomllib.loads((root / 'about.toml').read_text(encoding='utf-8'))
        self.assertEqual(config['dpi']['clarify']['license'], 'Apache-2.0 AND MIT')
        self.assertEqual({x['path'] for x in config['dpi']['clarify']['files']}, {'LICENSE', 'LICENSE-LIBM-MIT'})
        self.assertEqual(config['brotli-decompressor']['clarify']['license'], 'BSD-3-Clause')
        for name in ['alloc-stdlib', 'webview2-com', 'webview2-com-sys', 'webview2-com-macros', 'dlopen2', 'dlopen2_derive']:
            clarification = config[name]['clarify']
            self.assertNotIn('override-git-commit', clarification)
            self.assertEqual(clarification['git'][0]['path'], 'LICENSE')
            self.assertRegex(clarification['git'][0]['checksum'], r'^[a-f0-9]{64}$')

    def test_android_license_assets_track_project_notices_and_preserve_wrapper_license(self):
        root = Path(__file__).resolve().parent.parent
        build = (root / 'android/app/build.gradle.kts').read_text(encoding='utf-8')
        crypto = build.split('val cryptoNotices =', 1)[1].split('tasks.named("preBuild")', 1)[0]
        for name in ['LICENSE', 'NOTICE']:
            self.assertIn(f'rootProject.projectDir.resolve("../{name}")', crypto)
            self.assertIn(f'source{name.title()}.set(rootProject.layout.projectDirectory.file("../{name}"))', build)
        self.assertIn('supplementalNotices.set(rootProject.layout.projectDirectory.dir("gradle/licenses"))', build)
        stdlib = (root / 'android/gradle/licenses/kotlin-stdlib-2.2.10.txt').read_text(encoding='utf-8')
        self.assertIn('Boost Software License', stdlib)
        self.assertIn('Stephen Colebourne', stdlib)
        self.assertIn('Neither the name', stdlib)
        coroutines = (root / 'android/gradle/licenses/kotlinx-coroutines-1.6.4-NOTICE.txt').read_text(encoding='utf-8')
        self.assertIn('Copyright 2016-2021 JetBrains', coroutines)
        for name in ['kotlin-stdlib-2.2.10.txt', 'kotlinx-coroutines-1.6.4-NOTICE.txt']:
            self.assertIn('"' + name + '"', build)
        with zipfile.ZipFile(root / 'android/gradle/wrapper/gradle-wrapper.jar') as wrapper:
            license_text = wrapper.read('META-INF/LICENSE').decode('utf-8')
            self.assertIn('Apache License', license_text)
            self.assertIn('END OF TERMS AND CONDITIONS', license_text)
        for name in ['gradlew', 'gradlew.bat']:
            self.assertIn('SPDX-License-Identifier: Apache-2.0', (root / 'android' / name).read_text(encoding='utf-8'))

if __name__ == '__main__':
    unittest.main()
