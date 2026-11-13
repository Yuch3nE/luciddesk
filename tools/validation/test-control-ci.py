"""Batch pure tests; isolate tests that own native UI state."""
import argparse
import json
import re
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
FILTERS = (
    "pane::fixed_grid::tests::",
    "pane::free_layout::tests::",
    "pane::drag_drop::target::tests::",
    "pane::control::plans::tests::",
    "pane::control::content_layout::tests::",
    "pane::control::geometry::tests::",
    "pane::window::scheduling::tests::",
    "pane::layout_defaults::tests::",
    "pane::settings::tests::desktop_layout_defaults_",
    "pane::sorting::tests::",
    "pane::model::performance_tests::",
    "pane::label::cache_tests::",
)
BATCHED_FILTERS = (
    "pane::control::geometry::tests::",
    "pane::window::scheduling::tests::",
    "pane::label::cache_tests::",
)


def test_batches(tests):
    pure = [name for name in tests if name.startswith(BATCHED_FILTERS)]
    return ([pure] if pure else []) + [[name] for name in tests if name not in pure]


def batch_passed(result, expected):
    summary = re.search(r"test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;", result.stdout)
    return result.returncode == 0 and summary is not None and tuple(map(int, summary.groups())) == (expected, 0)


def select_tests(listing, filters=FILTERS):
    tests = [line.removesuffix(": test") for line in listing.splitlines()
             if line.endswith(": test") and line.startswith(filters)]
    for prefix in filters:
        if not any(name.startswith(prefix) for name in tests):
            raise RuntimeError(f"No tests found for {prefix}")
    return tests


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target-dir")
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--release", action="store_true")
    args = parser.parse_args()
    command = ["cargo", "test", "-p", "luciddesk", "--bin", "luciddesk",
               "--locked", "--no-run", "--message-format=json-render-diagnostics"]
    if args.target_dir:
        command += ["--target-dir", args.target_dir]
    if args.offline:
        command.append("--offline")
    if args.release:
        command.append("--release")
    build = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True, timeout=900)
    executables = []
    for line in build.stdout.splitlines():
        artifact = json.loads(line)
        if (artifact.get("reason") == "compiler-artifact"
                and artifact.get("profile", {}).get("test")
                and artifact.get("target", {}).get("name") == "luciddesk"
                and artifact.get("executable")):
            executables.append(artifact["executable"])
    if len(executables) != 1:
        raise RuntimeError(f"Expected one app test binary, found {executables}")
    executable = executables[0]
    listing = subprocess.check_output([executable, "--list"], cwd=ROOT, text=True, timeout=30)
    tests = select_tests(listing)
    failed = []
    batches = test_batches(tests)
    for batch in batches:
        label = ", ".join(batch)
        try:
            result = subprocess.run([executable, *batch, "--exact", "--test-threads=1"],
                                    cwd=ROOT, capture_output=True, text=True, timeout=60 * len(batch))
            passed = batch_passed(result, len(batch))
            if not passed:
                print(result.stdout + result.stderr, flush=True)
        except subprocess.TimeoutExpired:
            passed = False
            print(f"Timed out after {60 * len(batch)} seconds: {label}", flush=True)
        print(f"{'PASS' if passed else 'FAIL'} {label}", flush=True)
        if not passed:
            failed.extend(batch)
    print(f"{len(tests) - len(failed)} passed; {len(failed)} not verified; {len(batches)} processes", flush=True)
    return bool(failed)


if __name__ == "__main__":
    sys.exit(main())
