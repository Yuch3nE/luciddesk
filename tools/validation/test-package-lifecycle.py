"""Real Windows portable upgrade and desktop component unload tests.

Test app data is isolated; existing user data is hashed before/after.
MSI lifecycle coverage lives in test-installer.ps1.
"""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import time
import uuid
import zipfile

ROOT = Path(__file__).resolve().parents[2]
user = ctypes.WinDLL("user32", use_last_error=True)
user.FindWindowW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR]
user.FindWindowW.restype = wintypes.HWND
user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
user.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.CloseHandle.argtypes = [wintypes.HANDLE]
kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
kernel.OpenProcess.restype = wintypes.HANDLE
psapi = ctypes.WinDLL("psapi", use_last_error=True)
psapi.EnumProcessModulesEx.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.HMODULE), wintypes.DWORD,
                                      ctypes.POINTER(wintypes.DWORD), wintypes.DWORD]
psapi.GetModuleFileNameExW.argtypes = [wintypes.HANDLE, wintypes.HMODULE, wintypes.LPWSTR, wintypes.DWORD]


def explorer_modules():
    window = user.FindWindowW("Shell_TrayWnd", None)
    pid = wintypes.DWORD()
    user.GetWindowThreadProcessId(window, ctypes.byref(pid))
    assert pid.value, "Explorer taskbar not available"
    process = kernel.OpenProcess(0x410, False, pid.value)
    assert process, "Cannot inspect Explorer modules"
    try:
        modules = (wintypes.HMODULE * 2048)()
        size = wintypes.DWORD()
        assert psapi.EnumProcessModulesEx(process, modules, ctypes.sizeof(modules), ctypes.byref(size), 3)
        names = []
        for index in range(size.value // ctypes.sizeof(wintypes.HMODULE)):
            name = ctypes.create_unicode_buffer(32768)
            if psapi.GetModuleFileNameExW(process, modules[index], name, len(name)):
                names.append(name.value.removeprefix("\\\\?\\").lower())
        return pid.value, names
    finally:
        kernel.CloseHandle(process)


def verify_unloaded(directory, explorer_pid):
    dll = directory / "luciddesk_explorer.dll"
    path = str(dll.resolve()).lower()
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        pid, modules = explorer_modules()
        assert pid == explorer_pid, "Explorer restarted during DLL cleanup"
        if path not in modules:
            # A rename alone is possible even for a loaded image. Delete and rewrite
            # confirm the actual payload is released and ready for replacement.
            payload = dll.read_bytes()
            dll.unlink()
            dll.write_bytes(payload)
            return
        time.sleep(.05)
    raise AssertionError("Packaged DLL remained loaded in Explorer after app exit")


def unload_test(args, root, checks):
    directory = root / "app"
    directory.mkdir()
    for name in ["luciddesk.exe", "luciddesk_explorer.dll"]:
        shutil.copy2(args.source / name, directory / name)
    (directory / "portable").write_text("portable")
    process = None
    try:
        for cycle in range(10):
            process = launch(directory)
            pid, modules = explorer_modules()
            assert str((directory / "luciddesk_explorer.dll").resolve()).lower() in modules, "DLL was not loaded from the app directory"
            close(process)
            verify_unloaded(directory, pid)
        checks.append("10 real app start/exit cycles: app-directory DLL loaded, then absent from Explorer and directly replaceable")
        if args.probe:
            target = root / "menu-test.txt"
            target.write_text("test-owned file")
            for abrupt in [False, True]:
                explorer_pid, _ = explorer_modules()
                subprocess.run([str(args.probe.resolve()), str(directory / "luciddesk_explorer.dll"),
                                str(target), *( ["--abrupt"] if abrupt else [])], check=True, timeout=30)
                verify_unloaded(directory, explorer_pid)
            checks.append("Real native menu worker is cleaned up and DLL unloaded after both normal detach and abrupt owner exit")
    finally:
        close(process)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshot(path):
    return {str(p.relative_to(path)): digest(p) for p in path.rglob("*") if p.is_file()}


def extract(archive, destination):
    destination.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive) as package:
        # Packaging has one top-level directory; extraction cannot escape the test directory.
        for info in package.infolist():
            parts = Path(info.filename).parts
            assert parts and not any(p == ".." for p in parts) and not Path(info.filename).is_absolute()
            relative = Path(*parts[1:])
            if not relative.parts or info.is_dir():
                continue
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(package.read(info))
    verify_payload(destination)


def verify_payload(directory):
    build = json.loads((directory / "build.json").read_text(encoding="utf-8-sig"))
    for entry in build["files"]:
        assert digest(directory / entry["file"]) == entry["sha256"], entry["file"]
    return build


def tray(process):
    window = user.FindWindowW("windows-window.Window", "LucidDesk Tray")
    pid = wintypes.DWORD()
    if window:
        user.GetWindowThreadProcessId(window, ctypes.byref(pid))
    return window if pid.value == process.pid else None


def launch(directory, data=None, title=None):
    environment = os.environ.copy()
    environment.pop("LUCIDDESK_DATA_DIR", None)
    environment.pop("LUCIDDESK_DATA_DIR", None)
    if data is not None:
        environment["LUCIDDESK_DATA_DIR"] = str(data)
    command = [str(directory / "luciddesk.exe")]
    if title:
        command += ["--title", title]
    process = subprocess.Popen(command, cwd=directory, env=environment)
    data = data or directory / "data"
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        assert process.poll() is None, f"App exited during startup ({process.returncode})"
        if tray(process) and (data / "config.toml").exists():
            time.sleep(1)
            return process
        time.sleep(.1)
    raise AssertionError("App did not initialize tray and data")


def close(process):
    if process and process.poll() is None:
        window = tray(process)
        assert window and user.PostMessageW(window, 0x10, 0, 0), "Cannot request normal app exit"
        assert process.wait(timeout=25) == 0, "App failed during normal exit"


def verify_data(data, title, config_hash=None):
    with sqlite3.connect(data / "workspace.db") as database:
        assert database.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
        assert database.execute("SELECT count(*) FROM panels WHERE title=?", (title,)).fetchone()[0] == 1
    assert (data / "backups" / "keep.txt").read_text() == "preserve backup"
    if config_hash:
        assert digest(data / "config.toml") == config_hash, "Configuration changed during package replacement"


def seed(data):
    (data / "backups").mkdir(exist_ok=True)
    (data / "backups" / "keep.txt").write_text("preserve backup")
    return digest(data / "config.toml")


def portable(args, root, checks):
    directory = root / "portable-app"
    extract(args.old_zip, directory)
    data = directory / "data"
    title = "Portable upgrade preserved " + root.name
    process = None
    try:
        process = launch(directory, title=title)
        explorer_pid, modules = explorer_modules()
        assert str((directory / "luciddesk_explorer.dll").resolve()).lower() in modules
        close(process)
        verify_unloaded(directory, explorer_pid)
        config_hash = seed(data)
        extract(args.portable_zip, directory)
        verify_data(data, title, config_hash)
        checks.append("0.13.0 test build upgrades to 0.14.0 by direct ZIP overwrite, with DLL unloaded and data preserved")
        process = launch(directory)
        explorer_pid, modules = explorer_modules()
        assert str((directory / "luciddesk_explorer.dll").resolve()).lower() in modules
        package = root / "LucidDesk-0.14.0-windows-x64-portable.zip"
        shutil.copy2(args.portable_zip, package)
        close(process)
        verify_unloaded(directory, explorer_pid)
        extract(package, directory)
        assert verify_payload(directory)["version"] == "0.14.0"
        assert (directory / "portable").exists() and not (directory / "installed").exists()
        verify_data(data, title, config_hash)
        process = launch(directory)
        verify_data(data, title, config_hash)
        close(process)
        checks.append("Current app ZIP overwrite needs no DLL workaround and keeps portable mode, pane/config/backups")
    finally:
        close(process)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["portable", "unload"])
    parser.add_argument("--old-zip", type=Path)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--probe", type=Path)
    parser.add_argument("--portable-zip", type=Path)
    args = parser.parse_args()
    assert not user.FindWindowW("windows-window.Window", "LucidDesk Tray"), "Exit existing LucidDesk before testing"
    root = ROOT / "target/package-lifecycle" / (args.mode + "-" + str(uuid.uuid4()))
    root.mkdir(parents=True)
    protected = [Path(os.environ["LOCALAPPDATA"]) / "LucidDesk", Path(os.environ["LOCALAPPDATA"]) / "LucidDesk"]
    before = [snapshot(p) for p in protected]
    checks = []
    try:
        {"portable": portable, "unload": unload_test}[args.mode](args, root, checks)
        assert before == [snapshot(p) for p in protected], "Original user data changed"
        checks.append("Original LocalAppData user data remains byte-for-byte unchanged")
        report = {"mode": args.mode, "passed": True, "checks": checks,
                  "artifacts": {name: str(getattr(args, name)) for name in ["old_zip", "portable_zip", "source"] if getattr(args, name)}}
        (root / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(json.dumps(report, indent=2))
        print(f"Report: {root / 'report.json'}")
    except BaseException:
        (root / "report.json").write_text(json.dumps({"mode": args.mode, "passed": False, "completed_checks": checks}, indent=2))
        raise


if __name__ == "__main__":
    main()
