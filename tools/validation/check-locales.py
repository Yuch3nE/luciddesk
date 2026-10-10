"""Check embedded Fluent catalogs and UI resource references without third-party packages."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
LANGUAGES = ("zh-CN", "zh-TW", "en-US", "ja-JP", "ko-KR", "de-DE", "ru-RU")

def catalog(language):
    result = {}
    for line in (ROOT / "app/locales" / f"{language}.ftl").read_text(encoding="utf-8-sig").splitlines():
        if not line.strip() or line.startswith("#") or line[0].isspace():
            continue
        match = re.fullmatch(r"([a-z][a-z0-9-]*) = (.+)", line)
        if not match:
            raise ValueError(f"{language}: invalid message: {line}")
        key, value = match.groups()
        if key in result:
            raise ValueError(f"{language}: duplicate message {key}")
        result[key] = set(re.findall(r"\$([a-zA-Z][a-zA-Z0-9_-]*)", value))
    return result


def check():
    baseline = catalog("en-US")
    for language in LANGUAGES:
        other = catalog(language)
        if other != baseline:
            raise ValueError(f"{language}: message IDs or variables differ from en-US")
    for path in (ROOT / "app/src").rglob("*.rs"):
        for key in re.findall(r'i18n::(?:text|format|wide)\("([^"]+)"', path.read_text(encoding="utf-8-sig")):
            if key not in baseline:
                raise ValueError(f"{path}: missing message {key}")
        source = path.read_text(encoding="utf-8-sig")
        for call in re.finditer(r'i18n::format\("([^"]+)", &\[(.*?)\]\)', source, re.DOTALL):
            key, arguments = call.groups()
            names = set(re.findall(r'\("([a-zA-Z][a-zA-Z0-9_-]*)",', arguments))
            if names != baseline[key]:
                raise ValueError(f"{path}: incorrect arguments for {key}: {names}")
    print(f"Validated {len(baseline)} messages in {len(LANGUAGES)} languages.")


if __name__ == "__main__":
    check()
