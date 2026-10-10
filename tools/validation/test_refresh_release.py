"""Check accepted release assets without calling GitHub."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('refresh_release', Path(__file__).resolve().parents[1] / 'release/refresh-release.py')
refresh = importlib.util.module_from_spec(spec)
spec.loader.exec_module(refresh)


class ReleaseAssetsTests(unittest.TestCase):
    def fixture(self, root, msi):
        sha = 'a' * 40
        for portable in [False, True]:
            suffix = 'portable' if portable else '20261004-120000'
            prefix = 'LucidDesk-0.20.0-' + ('' if portable else sha[:7] + '-') + 'windows-x64-'
            with zipfile.ZipFile(root / (prefix + suffix + '.zip'), 'w') as archive:
                archive.writestr('app/build.json', json.dumps(dict(version='0.20.0', revision=sha[:7],
                    uncommittedChanges=False, portable=portable, files=[])))
        (root / 'LucidDesk-0.20.0-windows-x64-setup.exe').write_bytes(b'exe fixture')
        if msi:
            (root / 'LucidDesk-0.20.0-windows-x64.msi').write_bytes(b'msi fixture')
        for file in list(root.iterdir()):
            file.with_name(file.name + '.sha256').write_text(hashlib.sha256(file.read_bytes()).hexdigest() + '  ' + file.name)
        return sha

    def test_old_and_new_builds_and_corrupt_msi(self):
        for msi in [False, True]:
            with self.subTest(msi=msi), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                sha = self.fixture(root, msi)
                self.assertEqual(len(refresh.verify_packages(root, 'v0.20.0', sha)), 8 if msi else 6)
                if msi:
                    (root / 'LucidDesk-0.20.0-windows-x64.msi').write_bytes(b'corrupted')
                    with self.assertRaises(AssertionError):
                        refresh.verify_packages(root, 'v0.20.0', sha)


if __name__ == '__main__':
    unittest.main()
