"""Launch the real Windows app with isolated empty data and verify startup diagnostics.

Requires access to the interactive desktop, Explorer COM and the notification area.
Only windows belonging to the test process are closed. User configuration is not used.
"""
import argparse
import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import sqlite3
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]


def owned_windows(pid):
    user = ctypes.WinDLL("user32", use_last_error=True)
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user.GetClassNameW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    windows = []

    @callback_type
    def capture(hwnd, _):
        owner = wintypes.DWORD()
        user.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid:
            title, kind = ctypes.create_unicode_buffer(256), ctypes.create_unicode_buffer(256)
            user.GetWindowTextW(hwnd, title, len(title))
            user.GetClassNameW(hwnd, kind, len(kind))
            windows.append((hwnd, title.value, kind.value))
        return True

    user.EnumWindows(capture, 0)
    return windows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    args = parser.parse_args()
    executable = args.executable.resolve(strict=True)
    data = ROOT / "target" / "startup-diagnostics" / str(uuid.uuid4())
    data.mkdir(parents=True)
    print(f"Isolated startup data: {data}", flush=True)
    (data / "config.toml").write_text(
        'config_version = 1\n[diagnostics]\nlevel = "trace"\n[updates]\nauto_check = false\n',
        encoding="utf-8",
    )
    # Existing empty data avoids creating a first-run desktop panel.
    with sqlite3.connect(data / "workspace.db"):
        pass
    environment = dict(os.environ, LUCIDDESK_DATA_DIR=str(data), RUST_BACKTRACE="1")
    stderr_path = data / "stderr.log"
    user = ctypes.WinDLL("user32", use_last_error=True)
    user.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
    with (data / "stdout.log").open("wb") as stdout, stderr_path.open("wb") as stderr:
        process = subprocess.Popen([str(executable)], env=environment, stdout=stdout, stderr=stderr,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            started = time.monotonic()
            while True:
                if process.poll() is not None:
                    raise AssertionError(f"App exited during startup: {process.returncode}; "
                                         f"stderr={stderr_path.read_text(encoding='utf-8', errors='replace')}")
                windows = owned_windows(process.pid)
                assert not any(kind == "#32770" for _, _, kind in windows), "Startup displayed an error dialog"
                if any(title == "LucidDesk Tray" for _, title, _ in windows) and time.monotonic() - started >= 3:
                    break
                assert time.monotonic() - started < 15, "App never reached its tray/message loop"
                time.sleep(.05)
            text = (data / "logs" / "diagnostic.log").read_text(encoding="utf-8")
            for operation in ("desktop.hook.attach", "desktop.audit.start_worker"):
                assert any(f'component="{operation}"' in line and "outcome=ok" in line
                           for line in text.splitlines()), f"Operation did not complete: {operation}"
            assert 'component="app.startup"' not in text, "Startup logged a failure"
            assert "RefCell already borrowed" not in stderr_path.read_text(encoding="utf-8", errors="replace")
            print("PASS: real app reached desktop diagnostics, audit worker and tray", flush=True)
        finally:
            if process.poll() is None:
                for hwnd, title, kind in owned_windows(process.pid):
                    if title == "LucidDesk Tray" or kind == "#32770":
                        user.PostMessageW(hwnd, 0x10, 0, 0)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.terminate()  # Only the subprocess created by this test.
                    process.wait(timeout=5)
                    raise AssertionError("Test app did not exit gracefully")
        assert process.returncode == 0, f"App shutdown failed: {process.returncode}"
        print("PASS: test app exited gracefully", flush=True)


if __name__ == "__main__":
    main()
