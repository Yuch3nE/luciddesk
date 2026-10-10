"""Guard the EXE installer's minimal payload without installing on the CI host."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
# Runtime, Agent support, license and build provenance only.
PAYLOAD = {
    "luciddesk.exe", "luciddesk_explorer.dll", "luciddesk-cli.exe",
    "skills/luciddesk-control/SKILL.md", "cli.md", "protocol.schema.json",
    "LICENSE", "build.json", "installed",
}
PAYLOAD.update(path.relative_to(ROOT).as_posix()
               for path in (ROOT / 'skills/luciddesk-control').rglob('*') if path.is_file())


def validate(script):
    sections = re.findall(r"(?ms)^\[Files\]\s*\n(.*?)(?=^\[|\Z)", script)
    if len(sections) != 1:
        raise ValueError("Expected exactly one [Files] section")
    installed = set()
    for line in sections[0].splitlines():
        line = line.strip()
        if not line or line.startswith(";"):
            continue
        match = re.fullmatch(r'Source: "([^"]+)"; DestDir: "([^"]+)"; Flags: ignoreversion', line)
        if not match:
            raise ValueError(f"Unexpected payload directive: {line}")
        source, destination = (part.replace('\\', '/') for part in match.groups())
        name = source.removeprefix('{#SourcePath}/')
        if name not in PAYLOAD or name in installed:
            raise ValueError(f"Unapproved or duplicate payload: {source}")
        expected_source = 'installed' if name == 'installed' else '{#SourcePath}/' + name
        parent = name.rpartition('/')[0]
        expected_destination = '{app}' + ('/' + parent if parent else '')
        if source != expected_source or destination != expected_destination:
            raise ValueError(f"Unexpected source or destination: {line}")
        installed.add(name)
    if installed != PAYLOAD:
        raise ValueError(f"Missing payload: {sorted(PAYLOAD - installed)}")


class InstallerPayloadTests(unittest.TestCase):
    def setUp(self):
        self.script = (ROOT / 'installer/LucidDesk.iss').read_text(encoding='utf-8-sig')

    def test_production_payload(self):
        validate(self.script)
        self.assertTrue((ROOT / 'installer/installed').is_file())

    def test_rejects_broad_copy_and_extra_documentation(self):
        for source in ['*', 'CHANGELOG.md', 'Refresh-App-Icon.ps1']:
            extra = f'Source: "{{#SourcePath}}\\{source}"; DestDir: "{{app}}"; Flags: ignoreversion\n'
            with self.subTest(source=source), self.assertRaises(ValueError):
                validate(self.script.replace('[Files]\n', '[Files]\n' + extra))

    def test_rejects_missing_cli_or_wrong_skill_destination(self):
        with self.assertRaises(ValueError):
            validate('\n'.join(line for line in self.script.splitlines()
                               if not line.startswith('Source: "{#SourcePath}\\luciddesk-cli.exe"')))
        with self.assertRaises(ValueError):
            validate(self.script.replace('DestDir: "{app}\\skills\\luciddesk-control"', 'DestDir: "{app}"'))

    def test_rejects_each_missing_skill_reference(self):
        for path in sorted(PAYLOAD):
            if '/references/' not in path:
                continue
            with self.subTest(path=path), self.assertRaises(ValueError):
                validate('\n'.join(line for line in self.script.splitlines()
                                   if path.replace('/', '\\') not in line))


if __name__ == '__main__':
    unittest.main()
