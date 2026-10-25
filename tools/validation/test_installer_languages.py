"""Keep installer languages and safety prompts complete without installing software."""
from collections import Counter
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
LANGUAGES = {
    "english": ("en-US", "Default.isl"),
    "chinesesimplified": ("zh-CN", "Languages\\ChineseSimplified.isl"),
    "chinesetraditional": ("zh-TW", "Languages\\ChineseTraditional.isl"),
    "japanese": ("ja-JP", "Languages\\Japanese.isl"),
    "korean": ("ko-KR", "Languages\\Korean.isl"),
    "german": ("de-DE", "Languages\\German.isl"),
    "russian": ("ru-RU", "Languages\\Russian.isl"),
}

def validate(script):
    section = script.split("[Languages]", 1)[1].split("[Tasks]", 1)[0]
    languages = re.findall(r'Name: "([^"]+)"; MessagesFile: "compiler:([^"]+)"', section)
    expected = [(name, resource) for name, (_, resource) in LANGUAGES.items()]
    if languages != expected:
        raise ValueError("Installer must declare all seven languages with English fallback")
    section = script.split("[CustomMessages]", 1)[1].split("[Code]", 1)[0]
    messages = {}
    for line in section.splitlines():
        if not line.strip() or line.startswith(';'):
            continue
        match = re.fullmatch(r"([a-z]+)\.([A-Za-z]+)=(.+)", line)
        if not match:
            raise ValueError(f"Invalid custom message: {line}")
        lang, key, value = match.groups()
        if lang not in LANGUAGES or (lang, key) in messages or not value.strip():
            raise ValueError(f"Unknown language, empty or duplicate message: {line}")
        messages[lang, key] = value
    keys = {key for lang, key in messages if lang == "english"}
    if not keys:
        raise ValueError("English custom messages are missing")
    for lang in LANGUAGES:
        if {key for language, key in messages if language == lang} != keys:
            raise ValueError(f"Incomplete custom messages: {lang}")
        for key in keys:
            placeholders = lambda value: Counter(re.findall(r"%[1-9nt%]|\{[^}]+\}", value))
            if placeholders(messages[lang, key]) != placeholders(messages["english", key]):
                raise ValueError(f"Mismatched placeholders: {lang}.{key}")
    code = script.split("[Code]", 1)[1]
    used = set(re.findall(r"CustomMessage\('([^']+)'\)", code))
    for key in re.findall(r"OperationMessage\('([^']+)'", code):
        used.update((key, key + "Uninstall"))
    if not used <= keys:
        raise ValueError(f"Untranslated code messages: {used - keys}")


class InstallerLanguageTests(unittest.TestCase):
    def setUp(self):
        self.script = (ROOT / "installer/LucidDesk.iss").read_text(encoding="utf-8-sig")

    def test_languages_match_app_and_all_custom_prompts_are_translated(self):
        validate(self.script)
        self.assertEqual({locale for locale, _ in LANGUAGES.values()},
                         {path.stem for path in (ROOT / "app/locales").glob("*.ftl")})
        for directive in ["LanguageDetectionMethod=uilanguage", "ShowLanguageDialog=yes", "UsePreviousLanguage=yes"]:
            self.assertIn(directive, self.script)

    def test_rejects_missing_language_or_prompt_and_duplicate_prompt(self):
        line = next(line for line in self.script.splitlines() if line.startswith("japanese.CloseTimeout="))
        for broken in [self.script.replace('Name: "korean"', 'Name: "unknown"'),
                       self.script.replace(line, ''), self.script.replace(line, line + "\n" + line)]:
            with self.subTest(broken=broken[-100:]), self.assertRaises(ValueError):
                validate(broken)

    def test_rejects_missing_placeholders_and_untranslated_new_prompts(self):
        for broken in [self.script.replace("english.CloseApp=", "english.CloseApp=%1 "),
                       self.script + "\nCustomMessage('NewSafetyPrompt');"]:
            with self.assertRaises(ValueError):
                validate(broken)


if __name__ == "__main__":
    unittest.main()
