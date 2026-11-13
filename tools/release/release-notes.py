"""Extract matching Chinese and English changelog sections for GitHub Releases (standard library only)."""
import argparse
import json
import re
from pathlib import Path
from urllib.parse import urljoin


def extract(text, tag, repository, link_tag=None):
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
    base = f"https://github.com/{repository}/blob/{link_tag or tag}/"
    body = re.sub(r"(\]\()([^\s)]+)(\))",
                  lambda m: m[1] + urljoin(base, m[2]) + m[3], body)
    return title + "\n\n" + body + "\n"


def bilingual(chinese, english, tag, repository, link_tag=None):
    zh = extract(chinese, tag, repository, link_tag)
    en = extract(english, tag, repository, link_tag)
    # Show the version once; preserve category headings without language banners.
    english_body = en.split("\n", 1)[1].lstrip()
    return zh.rstrip() + "\n\n---\n\n" + english_body


def version_key(tag):
    if not re.fullmatch(r"v?\d+\.\d+\.\d+", tag):
        raise ValueError(f"Invalid release tag: {tag}")
    return tuple(map(int, tag.removeprefix("v").split(".")))


def release_range(chinese, english, tag, repository, releases):
    current = version_key(tag)
    # Ignore drafts, prereleases, the current release on retries, and newer releases.
    candidates = []
    for release in releases:
        name = release["tag_name"]
        if release["draft"] or release["prerelease"] or not release.get("published_at"):
            continue
        if re.fullmatch(r"v?\d+\.\d+\.\d+", name) and version_key(name) < current:
            candidates.append(name)
    previous = max(candidates, key=version_key) if candidates else None
    lower = version_key(previous) if previous else (-1, -1, -1)
    def versions(text):
        return [v for v in re.findall(r"^##[ \t]+(\d+\.\d+\.\d+)\b", text, re.M)
                if lower < version_key(v) <= current]
    selected = versions(chinese)
    if selected != versions(english) or tag.removeprefix("v") not in selected:
        raise ValueError("Release range must contain matching bilingual versions including the current release")
    if previous:
        # Refuse a missing baseline rather than silently publishing a truncated history.
        extract(chinese, previous, repository)
        extract(english, previous, repository)
    sections = [bilingual(chinese, english, version, repository, tag)
                for version in sorted(selected, key=version_key, reverse=True)]
    header = (f"更新范围 / Changes: {previous} → {tag}\n\n"
              f"[完整差异 / Full diff](https://github.com/{repository}/compare/{previous}...{tag})\n\n"
              if previous else "")
    return header + "\n".join(sections)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    parser.add_argument("--changelog-en", type=Path, default=Path("CHANGELOG.en.md"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--releases-json", type=Path, help="GitHub releases from gh api --paginate --slurp")
    args = parser.parse_args()
    inputs = (args.changelog.read_text(encoding="utf-8-sig"),
              args.changelog_en.read_text(encoding="utf-8-sig"), args.tag, args.repository)
    if args.releases_json:
        pages = json.loads(args.releases_json.read_text(encoding="utf-8-sig"))
        releases = [release for page in pages for release in page]
        notes = release_range(*inputs, releases)
    else:
        notes = bilingual(*inputs)
    args.output.write_text(notes, encoding="utf-8")
