"""Replace release packages with a verified, successful Build CI artifact."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile


def gh(*args):
    return subprocess.check_output(["gh", *args], text=True)


def verify_packages(root, tag, sha):
    version = tag.removeprefix("v")
    files = sorted(path for path in root.rglob("*") if path.is_file())
    archives = [path for path in files if path.name.endswith((".zip", "-setup.exe", ".msi"))]
    # CI contains two ZIPs and EXE; optionally accept builds that also include MSI.
    assert len(archives) in (3, 4) and len(files) == 2 * len(archives), "Expected packages and matching checksums"
    expected_names = [
        rf"LucidDesk-{re.escape(version)}-[0-9a-f]{{7,40}}-windows-x64-[0-9-]+\.zip",
        rf"LucidDesk-{re.escape(version)}-windows-x64-portable\.zip",
        rf"LucidDesk-{re.escape(version)}-windows-x64-setup\.exe",
    ]
    if len(archives) == 4:
        expected_names.append(rf"LucidDesk-{re.escape(version)}-windows-x64\.msi")
    for pattern in expected_names:
        assert sum(bool(re.fullmatch(pattern, path.name)) for path in archives) == 1
    for archive in archives:
        checksum = archive.with_name(archive.name + ".sha256")
        expected, filename = checksum.read_text(encoding="utf-8-sig").strip().split(maxsplit=1)
        assert filename.lstrip(" *") == archive.name, "Checksum filename mismatch"
        assert hashlib.sha256(archive.read_bytes()).hexdigest() == expected.lower()
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as zipped:
                manifests = [name for name in zipped.namelist() if name.endswith("/build.json")]
                assert len(manifests) == 1
                manifest = json.loads(zipped.read(manifests[0]).decode("utf-8-sig"))
                assert manifest["version"] == version
                assert re.fullmatch(r"[0-9a-f]{7,40}", manifest["revision"])
                assert sha.startswith(manifest["revision"])
                assert not manifest["uncommittedChanges"]
                assert manifest["portable"] == archive.name.endswith("-portable.zip")
                prefix = manifests[0].removesuffix("build.json")
                for entry in manifest["files"]:
                    data = zipped.read(prefix + entry["file"])
                    assert hashlib.sha256(data).hexdigest() == entry["sha256"]
    return files


def main():
    tag, run_id, sha = (os.environ[name] for name in ("RELEASE_TAG", "SOURCE_RUN_ID", "SOURCE_SHA"))
    assert re.fullmatch(r"v?[0-9]+\.[0-9]+\.[0-9]+", tag), "Invalid version tag"
    assert run_id.isdecimal() and re.fullmatch(r"[0-9a-f]{40}", sha), "Invalid source build"
    run = json.loads(gh("api", f"repos/{{owner}}/{{repo}}/actions/runs/{run_id}"))
    assert run["status"] == "completed" and run["conclusion"] == "success"
    assert run["head_sha"] == sha and run["path"] == ".github/workflows/build.yml"
    assert run["head_repository"]["full_name"] == os.environ["GH_REPO"]
    release = json.loads(gh("api", f"repos/{{owner}}/{{repo}}/releases/tags/{tag}"))
    assert not release["draft"], "Expected a published release"
    files = verify_packages(Path("artifacts"), tag, sha)
    # Replace matching names first; remove obsolete packages only after upload verification.
    gh("release", "upload", tag, *(str(path) for path in files), "--clobber")
    published = json.loads(gh("api", f"repos/{{owner}}/{{repo}}/releases/tags/{tag}"))
    by_name = {asset["name"]: asset for asset in published["assets"]}
    for path in files:
        asset = by_name[path.name]
        assert asset["size"] == path.stat().st_size
        assert asset["digest"] == "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()
    note = (f"<!-- refreshed-build -->\n"
            f"下载包已更新 / Packages refreshed: [`{sha[:7]}`](https://github.com/{os.environ['GH_REPO']}/commit/{sha})"
            f" · [CI #{run_id}]({run['html_url']})\n<!-- /refreshed-build -->")
    body = re.sub(r"\n*<!-- refreshed-build -->.*?<!-- /refreshed-build -->", "", release["body"] or "", flags=re.S)
    Path("release-notes.md").write_text(body.rstrip() + "\n\n" + note + "\n", encoding="utf-8")
    gh("release", "edit", tag, "--notes-file", "release-notes.md")
    old_pattern = rf"LucidDesk-{re.escape(tag.removeprefix('v'))}-[0-9a-f]{{7,40}}-windows-x64-[0-9-]+\.zip(?:\.sha256)?"
    for asset in release["assets"]:
        if asset["name"] not in {path.name for path in files} and re.fullmatch(old_pattern, asset["name"]):
            gh("release", "delete-asset", tag, asset["name"], "--yes")
    print(f"Updated {tag} with {len(files)} verified assets from {sha}. Tag unchanged.")


if __name__ == "__main__":
    main()
