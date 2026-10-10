"""Regression checks for version-specific bilingual Release notes."""
import importlib.util
from pathlib import Path
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release_notes", ROOT / "tools/release/release-notes.py")
notes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notes)


class ReleaseNotesTests(unittest.TestCase):
    def test_both_languages_and_tagged_links(self):
        zh = "## 1.2.3 · 2026-09-30\n\n### feat · 新增功能\n\n- 中文 [指南](docs/usage.md)\n\n### fix · 问题修复\n\n- 中文修复\n\n## 1.2.2\n\nOLD"
        en = "## 1.2.3 · 2026-09-30\n\n### feat · Features\n\n- English [Guide](docs/usage.md) [Web](https://example.com)\n\n### fix · Fixes\n\n- English fix\n"
        result = notes.bilingual(zh, en, "v1.2.3", "owner/repo")
        self.assertLess(result.index("- 中文"), result.index("- English"))
        self.assertEqual(result.count("## 1.2.3 · 2026-09-30"), 1)
        self.assertNotIn("## 简体中文", result)
        self.assertNotIn("## English", result)
        for category in ["### feat · 新增功能", "### fix · 问题修复", "### feat · Features", "### fix · Fixes"]:
            self.assertIn(category, result)
        self.assertEqual(result.count("https://github.com/owner/repo/blob/v1.2.3/docs/usage.md"), 2)
        self.assertIn("https://example.com", result)
        self.assertNotIn("OLD", result)

    def test_either_language_must_have_one_nonempty_section(self):
        valid = "## 1.2.3\n\n- Change\n"
        for invalid in ["## 1.2.2\n\n- Older", "## 1.2.3\n", valid + valid]:
            for zh, en in [(valid, invalid), (invalid, valid)]:
                with self.subTest(zh=zh, en=en), self.assertRaises(ValueError):
                    notes.bilingual(zh, en, "v1.2.3", "owner/repo")

    def test_unprefixed_tag_uses_version_section_and_original_tag_in_links(self):
        zh = "## 1.2.3 · 2026-09-30\n\n- 中文 [指南](docs/usage.md)\n\n## 1.2.2\n\nOLD"
        en = "## 1.2.3 · 2026-09-30\n\n- English [Guide](docs/usage.md)\n"
        result = notes.bilingual(zh, en, "1.2.3", "owner/repo")
        self.assertEqual(result.count("## 1.2.3 · 2026-09-30"), 1)
        self.assertEqual(result.count("https://github.com/owner/repo/blob/1.2.3/docs/usage.md"), 2)
        self.assertNotIn("blob/v1.2.3/", result)
        self.assertNotIn("OLD", result)

    def test_invalid_tag(self):
        with self.assertRaises(ValueError):
            notes.extract("## 1.2.3\n\n- Change", "main", "owner/repo")

    def test_repository_versions_and_history_match(self):
        zh = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8-sig")
        en = (ROOT / "CHANGELOG.en.md").read_text(encoding="utf-8-sig")
        versions = lambda text: re.findall(r"^## (\d+\.\d+\.\d+)\b", text, re.M)
        self.assertEqual(versions(zh), versions(en))
        version = tomllib.loads((ROOT / "app/Cargo.toml").read_text())["package"]["version"]
        self.assertEqual(versions(zh)[0], version)
        lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
        self.assertEqual(next(p["version"] for p in lock["package"] if p["name"] == "luciddesk"), version)
        cli_version = tomllib.loads((ROOT / "cli/Cargo.toml").read_text())["package"]["version"]
        self.assertEqual(cli_version, version, "GUI and CLI versions must match")
        self.assertEqual(next(p["version"] for p in lock["package"] if p["name"] == "luciddesk-cli"), version)
        for name in ["README.md", "README.en.md"]:
            badges = re.findall(r"badge/version-([0-9]+\.[0-9]+\.[0-9]+)-", (ROOT / name).read_text(encoding="utf-8"))
            self.assertEqual(badges, [version])
        self.assertIn(f"**{version}**", zh.split("\n## ")[0])
        self.assertIn(f"**{version}**", en.split("\n## ")[0])

        for ver in versions(zh):
            for tag in (ver, "v" + ver):
                notes.bilingual(zh, en, tag, "Yuch3nE/luciddesk")


if __name__ == "__main__":
    unittest.main()
