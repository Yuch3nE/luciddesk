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
    def test_release_range_uses_previous_stable_release_and_current_tag_links(self):
        text = "\n\n".join(f"## {v}\n\n- {v} [Guide](docs/usage.md)" for v in
                             ["1.11.0", "1.10.0", "1.9.1", "1.9.0", "1.8.0"])
        releases = [{"tag_name": tag, "draft": draft, "prerelease": pre, "published_at": "date"}
                    for tag, draft, pre in [("v1.8.0", False, False), ("1.9.0", False, False),
                                           ("v1.9.1", False, True), ("v1.10.0", False, False),
                                           ("v1.11.0", False, False), ("v1.9.2", True, False)]]
        result = notes.release_range(text, text, "v1.10.0", "owner/repo", releases)
        self.assertIn("1.9.0 → v1.10.0", result)
        self.assertIn("compare/1.9.0...v1.10.0", result)
        self.assertEqual(re.findall(r"^## (.+)$", result, re.M), ["1.10.0", "1.9.1"])
        self.assertIn("blob/v1.10.0/docs/usage.md", result)
        self.assertNotIn("blob/1.9.1/", result)
        for invalid in [text.replace("## 1.9.1", "## 1.9.2"), text.replace("## 1.10.0", "## 2.0.0")]:
            with self.assertRaises(ValueError):
                notes.release_range(text, invalid, "v1.10.0", "owner/repo", releases)
        with self.assertRaises(ValueError):
            notes.release_range(text.replace("## 1.9.0", "## Missing"), text,
                                "v1.10.0", "owner/repo", releases)
        first = notes.release_range(text, text, "v1.10.0", "owner/repo", [])
        self.assertEqual(re.findall(r"^## (.+)$", first, re.M), ["1.10.0", "1.9.1", "1.9.0", "1.8.0"])

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
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        self.assertEqual(versions(zh)[0], version)
        lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
        self.assertEqual(next(p["version"] for p in lock["package"] if p["name"] == "luciddesk"), version)
        self.assertEqual(next(p["version"] for p in lock["package"] if p["name"] == "luciddesk-cli"), version)
        for name in ["README.md", "README.en.md"]:
            badges = re.findall(r"badge/(?i:version)-([0-9]+\.[0-9]+\.[0-9]+)-", (ROOT / name).read_text(encoding="utf-8"))
            self.assertEqual(badges, [version])
        self.assertIn(f"**{version}**", zh.split("\n## ")[0])
        self.assertIn(f"**{version}**", en.split("\n## ")[0])

        for ver in versions(zh):
            for tag in (ver, "v" + ver):
                notes.bilingual(zh, en, tag, "Yuch3nE/luciddesk")

    def test_website_document_versions_match_release(self):
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        documents = {
            "manual-version": ["docs/usage.md", "docs/usage.en.md"],
            "release-doc-version": ["PRIVACY.md", "PRIVACY.en.md", "docs/cli.md",
                                    "docs/installer.md", "docs/portable.md", "docs/package.md"],
        }
        for marker, paths in documents.items():
            for path in paths:
                with self.subTest(path=path):
                    text = (ROOT / path).read_text(encoding="utf-8-sig")
                    self.assertEqual(re.findall(rf"^<!-- {marker}: (.*?) -->$", text, re.M), [version])
                    self.assertRegex(text, r"(?m)^# .+")
                    self.assertRegex(text, r"(?m)^## .+")

    def test_workspace_packages_inherit_version_and_plugins_remain_independent(self):
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]
        version = workspace["package"]["version"]
        self.assertRegex(version, r"^\d+\.\d+\.\d+$")
        lock = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
        for member in workspace["members"]:
            package = tomllib.loads((ROOT / member / "Cargo.toml").read_text())["package"]
            with self.subTest(member=member):
                if member.startswith("plugins/"):
                    self.assertIsInstance(package["version"], str)
                    expected = package["version"]
                else:
                    self.assertEqual(package["version"], {"workspace": True})
                    expected = version
                entries = [p for p in lock if p["name"] == package["name"] and "source" not in p]
                self.assertEqual(len(entries), 1)
                self.assertEqual(entries[0]["version"], expected)


if __name__ == "__main__":
    unittest.main()
