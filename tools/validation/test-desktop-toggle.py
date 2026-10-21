"""Exercise blank-desktop gestures using isolated data and owned test windows.

Requires no running LucidDesk instance and an interactive Windows desktop.
Only the test process is closed; native icons are verified after shutdown.
Mouse messages target verified blank desktop coordinates without moving the cursor.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
import uuid

from importlib.util import spec_from_file_location, module_from_spec

ROOT = Path(__file__).resolve().parents[2]
spec = spec_from_file_location("startup", Path(__file__).with_name("test-startup-diagnostics.py"))
startup = module_from_spec(spec)
spec.loader.exec_module(startup)
user, kernel = c.WinDLL("user32", use_last_error=True), c.WinDLL("kernel32", use_last_error=True)
user.FindWindowExW.argtypes = [w.HWND, w.HWND, w.LPCWSTR, w.LPCWSTR]
user.FindWindowExW.restype = w.HWND
user.GetParent.argtypes = [w.HWND]
user.GetParent.restype = w.HWND
user.IsWindowVisible.argtypes = [w.HWND]
user.IsWindow.argtypes = [w.HWND]
user.GetPropW.argtypes = [w.HWND, w.LPCWSTR]
user.GetPropW.restype = w.HANDLE
user.GetClientRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
user.ClientToScreen.argtypes = [w.HWND, c.POINTER(w.POINT)]
user.ScreenToClient.argtypes = [w.HWND, c.POINTER(w.POINT)]
user.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
user.ShowWindow.argtypes = [w.HWND, c.c_int]
user.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
user.SendMessageTimeoutW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM, w.UINT, w.UINT, c.POINTER(c.c_size_t)]
kernel.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
kernel.OpenProcess.restype = w.HANDLE
kernel.VirtualAllocEx.argtypes = [w.HANDLE, c.c_void_p, c.c_size_t, w.DWORD, w.DWORD]
kernel.VirtualAllocEx.restype = c.c_void_p
kernel.WriteProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
kernel.VirtualFreeEx.argtypes = [w.HANDLE, c.c_void_p, c.c_size_t, w.DWORD]
kernel.CloseHandle.argtypes = [w.HANDLE]


class ProcessEntry(c.Structure):
    _fields_ = [("size", w.DWORD), ("usage", w.DWORD), ("pid", w.DWORD),
        ("heap", c.c_size_t), ("module", w.DWORD), ("threads", w.DWORD),
        ("parent", w.DWORD), ("priority", w.LONG), ("flags", w.DWORD), ("name", w.WCHAR * 260)]


kernel.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
kernel.CreateToolhelp32Snapshot.restype = w.HANDLE
kernel.Process32FirstW.argtypes = [w.HANDLE, c.POINTER(ProcessEntry)]
kernel.Process32NextW.argtypes = [w.HANDLE, c.POINTER(ProcessEntry)]


def children(pid):
    snapshot = kernel.CreateToolhelp32Snapshot(2, 0)
    assert snapshot and snapshot != c.c_void_p(-1).value
    result, entry = [], ProcessEntry()
    entry.size = c.sizeof(entry)
    try:
        more = kernel.Process32FirstW(snapshot, c.byref(entry))
        while more:
            if entry.parent == pid:
                result.append(entry.pid)
            more = kernel.Process32NextW(snapshot, c.byref(entry))
    finally:
        kernel.CloseHandle(snapshot)
    return result


def wait(check, message, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError(message)


def desktop():
    found = []
    callback_type = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)

    @callback_type
    def inspect(hwnd, _):
        parent = user.FindWindowExW(hwnd, None, "SHELLDLL_DefView", None)
        if parent:
            view = user.FindWindowExW(parent, None, "SysListView32", None)
            if view:
                found.append((view, parent))
        return True

    user.EnumWindows(inspect, 0)
    assert len(found) == 1, f"Expected one desktop view, found {len(found)}"
    return found[0]


def blank_point(view):
    """LVM_HITTEST runs in Explorer; marshal its tiny buffer into that process."""
    pid = w.DWORD()
    user.GetWindowThreadProcessId(view, c.byref(pid))
    process = kernel.OpenProcess(0x0008 | 0x0020, False, pid.value)
    assert process, "Cannot open desktop for read-only hit testing"
    buffer = kernel.VirtualAllocEx(process, None, 24, 0x3000, 0x04)
    try:
        assert buffer
        bounds = w.RECT()
        user.GetClientRect(view, c.byref(bounds))
        for y in range(bounds.bottom - 12, 10, -80):
            for x in range(bounds.right - 12, 10, -80):
                hit = (c.c_int32 * 6)(x, y, 0, -1, 0, 0)
                written = c.c_size_t()
                assert kernel.WriteProcessMemory(process, buffer, hit, 24, c.byref(written)) and written.value == 24
                result = c.c_size_t()
                assert user.SendMessageTimeoutW(view, 0x1012, 0, buffer, 2, 1000, c.byref(result))
                if c.c_ssize_t(result.value).value == -1:
                    point = w.POINT(x, y)
                    user.ClientToScreen(view, c.byref(point))
                    return point
        raise AssertionError("No blank desktop area found")
    finally:
        if buffer:
            kernel.VirtualFreeEx(process, buffer, 0, 0x8000)
        kernel.CloseHandle(process)


def double_click(hwnd, screen):
    point = w.POINT(screen.x, screen.y)
    user.ScreenToClient(hwnd, c.byref(point))
    position = (point.x & 0xffff) | ((point.y & 0xffff) << 16)
    # Match real Windows double-click delivery, at a verified blank point.
    for index, message in enumerate([0x201, 0x202, 0x203, 0x202]):
        posted = time.perf_counter()
        assert user.PostMessageW(hwnd, message, 1 if message in (0x201, 0x203) else 0, position)
        if index != 3:
            time.sleep(.035)
    return posted


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--with-monitor", action="store_true", help="Require the bundled monitor subprocess and check its visibility")
    parser.add_argument("--expect-consumed-double-clicks", action="store_true", help="Check debug-build hook counters")
    args = parser.parse_args()
    executable, cli_path = args.executable.resolve(strict=True), args.cli.resolve(strict=True)
    data = ROOT / "target" / "desktop-toggle-tests" / str(uuid.uuid4())
    data.mkdir(parents=True)
    (data / "config.toml").write_text(
        'config_version=1\n[desktop]\ndouble_click_hide=true\n[diagnostics]\nlevel="info"\n[updates]\nauto_check=false\n', encoding="utf-8")
    with sqlite3.connect(data / "workspace.db"):
        pass
    if args.with_monitor:
        plugin_config = data / "plugins" / "system-monitor"
        plugin_config.mkdir(parents=True)
        (plugin_config / "config.toml").write_text('[options]\nenabled=true\n', encoding="utf-8")
    environment = dict(os.environ, LUCIDDESK_DATA_DIR=str(data), RUST_BACKTRACE="1")
    print(f"Isolated desktop toggle data: {data}", flush=True)
    view, parent = desktop()
    original_visible = bool(user.IsWindowVisible(view))
    point = blank_point(view)

    def cli(*args, input=None):
        result = subprocess.run([str(cli_path), *args, "--data-dir", str(data), "--json"],
            input=input, env=environment, capture_output=True, encoding="utf-8", timeout=20)
        assert result.returncode == 0, result.stdout + result.stderr
        result = json.loads(result.stdout)
        assert result.get("ok", result.get("success", True)), result
        return result["data"]

    def runtime():
        return cli("settings", "get")["runtime"]

    def ready():
        try:
            return runtime()["desktop_double_click_hide"]
        except AssertionError as error:
            if "APP_NOT_RUNNING" in str(error) or '"code":"BUSY"' in str(error):
                return False
            raise

    def pane(title):
        cli("pane", "create", "--title", title)
        return wait(lambda: next((hwnd for hwnd, text, _ in startup.owned_windows(process.pid) if title in text), None), "Pane HWND missing")

    def tray():
        return next((hwnd for hwnd, text, _ in startup.owned_windows(process.pid) if text == "LucidDesk Tray"), None)

    with (data / "stdout.log").open("wb") as stdout, (data / "stderr.log").open("wb") as stderr:
        process = subprocess.Popen([str(executable)], env=environment, stdout=stdout, stderr=stderr,
            creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            wait(lambda: process.poll() is not None or tray(), "No tray after startup")
            assert process.poll() is None, "Another instance may hold the single-instance mutex"
            wait(ready, "Feature not enabled or CLI server not ready")
            monitor = None
            if args.with_monitor:
                monitor = wait(lambda: next((hwnd for pid in children(process.pid)
                    for hwnd, title, _ in startup.owned_windows(pid)
                    if title.startswith("LucidDesk — ") and user.IsWindowVisible(hwnd)), None), "Monitor child window missing")
                # Full configuration and visibility use separate IPC variants.
                # Exercise both without changing the user's real preferences.
                for language in ["zh-TW", "en-US"]:
                    cli("settings", "update", "--input", "-", input=json.dumps({"language": language}))
                    time.sleep(.15)
                assert user.IsWindow(monitor), "Full configuration update replaced or closed the renderer"
            shown = pane("Desktop toggle shown fixture")
            prior_hidden = pane("Desktop toggle prior hidden fixture")
            user.ShowWindow(prior_hidden, 0)
            controller = next(hwnd for hwnd, title, _ in startup.owned_windows(process.pid)
                if title == "LucidDesk Hybrid Controller")
            old_time = (kernel.GetTickCount() - 5_000) & 0xFFFFFFFF
            assert user.PostMessageW(controller, 0x8000 + 0x4a3, view, old_time)
            log = data / "logs" / "diagnostic.log"
            wait(lambda: log.exists() and "expired on receipt" in log.read_text(encoding="utf-8"),
                "Stale desktop gesture was not discarded and logged")
            assert not runtime()["desktop_hidden"], "Stale gesture hid the desktop"
            assert user.IsWindowVisible(shown), "Stale gesture hid a pane"
            print("PASS: stale gesture discarded without hiding desktop", flush=True)
            consumed_before = user.GetPropW(view, "LucidDesk.Desktop.DoubleClick.Consumed") or 0
            double_click(view if user.IsWindowVisible(view) else parent, point)
            wait(lambda: runtime()["desktop_hidden"], "Blank double click did not hide")
            assert not user.IsWindowVisible(view), "Native icons remain visible"
            assert not user.IsWindowVisible(shown) and not user.IsWindowVisible(prior_hidden)
            if monitor:
                wait(lambda: not user.IsWindowVisible(monitor), "Monitor child escaped hidden mode")
            created_hidden = pane("Desktop toggle created hidden fixture")
            assert not user.IsWindowVisible(created_hidden), "New pane escaped hidden mode"
            double_click(parent, point)
            wait(lambda: not runtime()["desktop_hidden"], "Background double click did not restore")
            assert bool(user.IsWindowVisible(view)) == original_visible
            assert user.IsWindowVisible(shown) and user.IsWindowVisible(created_hidden)
            assert not user.IsWindowVisible(prior_hidden), "Originally hidden pane was revealed"
            if monitor:
                wait(lambda: user.IsWindowVisible(monitor), "Monitor child did not restore")
                print("PASS: monitor subprocess hide and restore", flush=True)
            if args.expect_consumed_double_clicks:
                consumed = user.GetPropW(view, "LucidDesk.Desktop.DoubleClick.Consumed") or 0
                assert consumed >= consumed_before + 2, "Blank double-clicks leaked into native Shell dispatch"
                print("PASS: real double-click messages consumed before Shell activation", flush=True)
            print("PASS: hide, background restore, preserved visibility, new pane gate", flush=True)
            double_click(view if user.IsWindowVisible(view) else parent, point)
            wait(lambda: runtime()["desktop_hidden"], "Second hide failed")
            cli("settings", "update", "--input", "-", input=json.dumps({"desktop.double_click_hide": False}))
            wait(lambda: not runtime()["desktop_hidden"], "Disabling did not restore")
            assert bool(user.IsWindowVisible(view)) == original_visible
            cli("settings", "update", "--input", "-", input=json.dumps({"desktop.double_click_hide": True}))
            time.sleep(.3)
            double_click(view if user.IsWindowVisible(view) else parent, point)
            wait(lambda: runtime()["desktop_hidden"], "Third hide failed")
            assert user.PostMessageW(tray(), 0x8000 + 80, 0, (1 << 16) | 0x400)
            wait(lambda: not runtime()["desktop_hidden"], "Tray reveal did not restore")
            assert bool(user.IsWindowVisible(view)) == original_visible
            print("PASS: settings disable and tray restore", flush=True)
            durations = []
            for hidden in [True, False] * 3:
                started = double_click(view if user.IsWindowVisible(view) else parent, point)
                deadline = time.monotonic() + 5
                while True:
                    settled = bool(user.IsWindowVisible(view)) == (original_visible and not hidden)
                    settled &= bool(user.IsWindowVisible(shown)) == (not hidden)
                    if monitor:
                        settled &= bool(user.IsWindowVisible(monitor)) == (not hidden)
                    if settled:
                        break
                    assert time.monotonic() < deadline, (
                        f"Repeated visibility transition did not settle: hidden={hidden}, "
                        f"icons={bool(user.IsWindowVisible(view))}, pane={bool(user.IsWindowVisible(shown))}, "
                        f"monitor={bool(user.IsWindowVisible(monitor)) if monitor else None}, "
                        f"runtime={runtime()}")
                    time.sleep(.001)
                durations.append(round((time.perf_counter() - started) * 1000, 1))
                time.sleep(.1)
            print(f"PASS: repeated native visibility transitions, milliseconds after final mouse-up: {durations}", flush=True)
            double_click(view if user.IsWindowVisible(view) else parent, point)
            wait(lambda: runtime()["desktop_hidden"], "Final hide failed")
            assert user.PostMessageW(tray(), 0x10, 0, 0)
            assert process.wait(timeout=15) == 0
            if monitor:
                wait(lambda: not user.IsWindow(monitor), "Monitor survived parent shutdown")
            wait(lambda: bool(user.IsWindowVisible(view)) == original_visible, "Shutdown did not restore icon visibility")
            print("PASS: graceful shutdown restores icons", flush=True)
        finally:
            if process.poll() is None:
                if tray():
                    user.PostMessageW(tray(), 0x10, 0, 0)
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.terminate()  # Only this script's owned test process.
                    process.wait(timeout=5)
            wait(lambda: bool(user.IsWindowVisible(view)) == original_visible, "Test cleanup did not restore original icon visibility")
        # Only the child created by this test is terminated. The existing
        # Explorer owner watcher must restore its saved window visibility after that crash.
        process = subprocess.Popen([str(executable)], env=environment, stdout=stdout, stderr=stderr,
            creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            wait(lambda: process.poll() is not None or tray(), "No tray in crash fixture")
            assert process.poll() is None
            wait(ready, "Crash fixture CLI not ready")
            crash_monitor = None
            if args.with_monitor:
                crash_monitor = wait(lambda: next((hwnd for pid in children(process.pid)
                    for hwnd, title, _ in startup.owned_windows(pid)
                    if title.startswith("LucidDesk — ")), None), "Crash fixture monitor missing")
            double_click(view if user.IsWindowVisible(view) else parent, point)
            wait(lambda: runtime()["desktop_hidden"], "Crash fixture did not hide")
            process.terminate()
            process.wait(timeout=5)
            if crash_monitor:
                wait(lambda: not user.IsWindow(crash_monitor), "Monitor survived parent termination")
                print("PASS: parent termination reclaims its monitor renderer", flush=True)
            wait(lambda: bool(user.IsWindowVisible(view)) == original_visible, "Owner watcher did not restore icons after process termination")
            print("PASS: owned test process termination restores icons", flush=True)
        finally:
            if process.poll() is None:
                if tray():
                    user.PostMessageW(tray(), 0x10, 0, 0)
                process.wait(timeout=15)
            wait(lambda: bool(user.IsWindowVisible(view)) == original_visible, "Crash fixture cleanup did not restore icon visibility")
    assert not (data / "stderr.log").read_text(encoding="utf-8", errors="replace").strip(), "Unexpected stderr"


if __name__ == "__main__":
    main()
