"""Verify MSI Skill layout without installing anything on the test host."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == 'nt', 'MSI generation requires PowerShell 7 on Windows')
class MsiPayloadTests(unittest.TestCase):
    def test_skill_files_keep_their_directory_and_content(self):
        with tempfile.TemporaryDirectory(prefix='luciddesk-msi-payload-') as temporary:
            root = Path(temporary)
            source = root / 'package with spaces'
            subprocess.run(['pwsh', '-NoProfile', '-File', str(ROOT / 'tools/packaging/stage-agent-payload.ps1'),
                            '-SourceRoot', str(ROOT), '-Destination', str(source)],
                           check=True, capture_output=True)
            output = root / 'payload.wxs'
            subprocess.run(['pwsh', '-NoProfile', '-File', str(ROOT / 'tools/packaging/msi-payload.ps1'),
                            '-SourcePath', str(source), '-RepoRoot', str(ROOT), '-OutputFile', str(output)],
                           check=True, capture_output=True)
            ns = {'w': 'http://wixtoolset.org/schemas/v4/wxs'}
            tree = ET.parse(output)
            parents = {}
            for ref in tree.findall('.//w:DirectoryRef', ns):
                for directory in ref:
                    parents[directory.attrib['Id']] = (ref.attrib['Id'], directory.attrib['Name'])

            def path_for(directory):
                if directory == 'INSTALLFOLDER':
                    return Path()
                parent, name = parents[directory]
                return path_for(parent) / name

            installed = {}
            for component in tree.findall('.//w:Component', ns):
                for file in component.findall('w:File', ns):
                    origin = Path(file.attrib['Source'])
                    destination = path_for(component.attrib.get('Directory', 'INSTALLFOLDER')) / file.attrib.get('Name', origin.name)
                    self.assertNotIn(destination, installed)
                    installed[destination] = origin
            for file in (ROOT / 'skills/luciddesk-control').rglob('*'):
                if file.is_file():
                    relative = file.relative_to(ROOT)
                    self.assertEqual(installed[relative].read_bytes(), file.read_bytes())


if __name__ == '__main__':
    unittest.main()
