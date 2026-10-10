"""Extract matching Chinese and English changelog sections for GitHub Releases (standard library only)."""
import argparse
import re
from pathlib import Path
from urllib.parse import urljoin


def extract(text, tag, repository):
    if not re.fullmatch(r"v?[0-9]+\.[0-9]+\.[0-9]+", tag):
        raise ValueError("Expected a release tag such as 0.14.0 or v0.14.0")
    release_version = tag.removeprefix("v")
    headings = list(re.finditer(r"^##[ \t]+(.+)$", text, re.MULTILINE))
    matches = []
    for index, heading in enumerate(headings):
        version = heading.group(1).strip().split()[0]
        if version == release_version:
            end = headings[index + 1].start() if index + 1 < len(headings) else len(text)
            body = text[heading.end():end].strip()
            matches.append((heading.group(0).strip(), body))
    if len(matches) != 1 or not matches[0][1]:
        raise ValueError(f"Expected exactly one nonempty CHANGELOG section for {tag}")
    title, body = matches[0]
    # Release pages need repository-relative links resolved against the tagged source.
    base = f"https://github.com/{repository}/blob/{tag}/"
    body = re.sub(r"(\]\()([^\s)]+)(\))",
                  lambda m: m[1] + urljoin(base, m[2]) + m[3], body)
    return title + "\n\n" + body + "\n"


def bilingual(chinese, english, tag, repository):
    zh = extract(chinese, tag, repository)
    en = extract(english, tag, repository)
    # Show the version once; preserve category headings without language banners.
    english_body = en.split("\n", 1)[1].lstrip()
    return zh.rstrip() + "\n\n---\n\n" + english_body


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    parser.add_argument("--changelog-en", type=Path, default=Path("CHANGELOG.en.md"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    notes = bilingual(
        args.changelog.read_text(encoding="utf-8-sig"),
        args.changelog_en.read_text(encoding="utf-8-sig"),
        args.tag, args.repository,
    )
    args.output.write_text(notes, encoding="utf-8")
