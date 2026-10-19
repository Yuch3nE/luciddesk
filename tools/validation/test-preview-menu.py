"""Live regression against two owned, populated desktop panes (Windows only).

Uses the real pane keyboard/posted-action routes and Explorer menu, not a mock
FilterSession. Configure Peek with the Space shortcut and one text fixture per
pane in an isolated app instance before running. Does not create/delete files
or start/stop the app. Native messages make the modal ordering deterministic;
this is not a measurement of physical keyboard timing.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import re
import subprocess
import time

u = c.WinDLL('user32', use_last_error=True)
u.SetProcessDpiAwarenessContext.argtypes = [c.c_void_p]
u.SetProcessDpiAwarenessContext(c.c_void_p(-4))
u.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
u.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, c.c_int]
u.IsWindowVisible.argtypes = [w.HWND]
u.GetWindowRect.argtypes = [w.HWND, c.POINTER(w.RECT)]
u.PostMessageW.argtypes = [w.HWND, w.UINT, c.c_size_t, c.c_ssize_t]
u.GetPropW.argtypes = [w.HWND, w.LPCWSTR]
u.GetPropW.restype = c.c_void_p
u.GetForegroundWindow.restype = w.HWND
u.SetForegroundWindow.argtypes = [w.HWND]
u.GetDpiForWindow.argtypes = [w.HWND]

class Key(c.Structure):
    _fields_ = [('vk',w.WORD),('scan',w.WORD),('flags',w.DWORD),('time',w.DWORD),('extra',c.c_size_t)]
class Mouse(c.Structure):
    _fields_ = [('dx',w.LONG),('dy',w.LONG),('data',w.DWORD),('flags',w.DWORD),('time',w.DWORD),('extra',c.c_size_t)]
class Payload(c.Union):
    _fields_ = [('key',Key),('mouse',Mouse)]
class Input(c.Structure):
    _anonymous_ = ('payload',)
    _fields_ = [('kind',w.DWORD),('payload',Payload)]
u.SendInput.argtypes = [w.UINT,c.POINTER(Input),c.c_int]

def key(vk):
    events = (Input*2)(Input(kind=1,key=Key(vk=vk)),Input(kind=1,key=Key(vk=vk,flags=2)))
    assert u.SendInput(2,events,c.sizeof(Input)) == 2

def focus(hwnd):
    key(0x12)
    time.sleep(.1)
    u.SetForegroundWindow(hwnd)
    wait(lambda: u.GetForegroundWindow() == hwnd, 'Could not focus fixture pane')

def click_fixture(hwnd, right=False):
    rect = w.RECT()
    assert u.GetWindowRect(hwnd,c.byref(rect))
    # First icon in the default grid; inspect the fixture layout before running.
    scale = u.GetDpiForWindow(hwnd) / 96
    u.SetCursorPos(rect.left+round(56*scale),rect.top+round(78*scale))
    events=(Input*2)(Input(kind=0,mouse=Mouse(flags=8 if right else 2)),
                     Input(kind=0,mouse=Mouse(flags=16 if right else 4)))
    assert u.SendInput(2,events,c.sizeof(Input)) == 2
    time.sleep(.2)

def windows():
    rows = []
    @c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    def visit(hwnd, unused):
        pid = w.DWORD()
        u.GetWindowThreadProcessId(hwnd, c.byref(pid))
        title = c.create_unicode_buffer(1024)
        u.GetWindowTextW(hwnd, title, 1024)
        rows.append(dict(hwnd=hwnd, pid=pid.value, title=title.value,
                         visible=bool(u.IsWindowVisible(hwnd))))
        return True
    u.EnumWindows(visit, 0)
    return rows

def post(hwnd, message, key=0, lp=0):
    assert u.PostMessageW(hwnd, message, key, lp), c.get_last_error()

def wait(check, label, timeout=8):
    start = time.monotonic()
    while time.monotonic() - start < timeout:
        result = check()
        if result:
            return result
        time.sleep(.05)
    raise AssertionError(label)

def run(args):
    def status():
        raw = subprocess.run([str(args.cli), 'status', '--data-dir', str(args.data_dir), '--json'],
                             check=True, capture_output=True, encoding='utf-8', timeout=4)
        value = json.loads(raw.stdout)
        assert value['ok'] and value['context']['instance_id'].startswith(str(args.pid)+'-'), value
        assert value['data']['desktop_connected'], value
        return value
    initial = status()
    view = int(re.search(r'view=0x([0-9a-f]+)', initial['data']['desktop_diagnostic']).group(1), 16)
    explorer_pid = int(re.search(r'explorer_pid=(\d+)', initial['data']['desktop_diagnostic']).group(1))
    owner = u.GetPropW(view, 'LucidDesk.Filter.Owner.v1')
    def ack():
        assert u.GetPropW(view, 'LucidDesk.Filter.Owner.v1') == owner, 'Session reconnected'
        return u.GetPropW(view, 'LucidDesk.Filter.Ack.v1')
    panes = []
    for title in args.panes:
        found = [r for r in windows() if r['pid'] == args.pid and r['title'] == 'LucidDesk — '+title]
        assert len(found) == 1, found
        panes.append(found[0]['hwnd'])
    def menu():
        return next((r for r in windows() if r['pid'] == explorer_pid
                     and r['title'] == 'LucidDesk Menu Host' and r['visible']), None)
    def previews():
        return [r for r in windows() if r['visible'] and any(r['title'].startswith(name+' - ') for name in args.fixtures)]
    def clean():
        dialogs = [r for r in windows() if r['pid'] == args.pid and r['visible'] and not r['title'].startswith('LucidDesk — ')]
        assert not dialogs, dialogs
        log = args.data_dir/'logs/diagnostic.log'
        errors = [line for line in log.read_text(encoding='utf-8').splitlines()
                  if re.search(r'\bpid='+str(args.pid)+r'\b', line) and 'level=ERROR' in line] if log.exists() else []
        assert not errors, errors
    assert not previews(), 'Close fixture previews before running'
    assert initial['data']['desktop_sync_status'] == 'applied', 'Close menus and wait for synchronization before running'
    clean()
    results = []
    for cycle in range(args.rounds):
        focus(panes[0])
        click_fixture(panes[0])
        click_fixture(panes[0], True)
        host = wait(menu, 'Explorer menu did not open')
        wait(lambda: status()['data']['desktop_sync_status'] == 'deferred', 'Session did not reserve menu')
        time.sleep(.4)
        for index, pane in enumerate(panes):
            if index:
                post(pane, 0x100, 0x27, 1)  # Select another pane without dismissing the modal menu.
                time.sleep(.2)
            before = ack()
            post(pane, 0x100, 0x20, 1)  # Real Event::Peek -> queued preview action.
            deadline = time.monotonic()+.8
            while time.monotonic() < deadline:
                assert not previews(), 'Peek opened while another menu owned the session'
                assert menu(), 'Preview dismissed the active menu'
                assert ack() == before, 'Busy preview sent a filter request'
                clean()
                time.sleep(.05)
            assert status()['data']['desktop_sync_status'] == 'deferred', 'Preview released another menu reservation'
            results.append(dict(round=cycle+1, pane=args.panes[index], blocked=True, ack=before, owner=owner))
            print(json.dumps(results[-1]), flush=True)
        assert u.GetForegroundWindow() == host['hwnd'], 'Menu lost foreground'
        key(0x1b)
        # Explorer caches its visible menu host even after the popup is gone.
        wait(lambda: status()['data']['desktop_sync_status'] == 'applied', 'Menu reservation leaked')
        for index, pane in enumerate(panes):
            focus(pane)
            click_fixture(pane)
            post(pane, 0x100, 0x20, 1)
            preview = wait(lambda: next((r for r in previews() if r['title'].startswith(args.fixtures[index]+' - ')), None),
                           'Preview did not work after menu dismissal')
            assert u.GetForegroundWindow() == preview['hwnd']
            key(0x1b)
            wait(lambda: not previews(), 'Fixture preview did not close')
            wait(lambda: status()['data']['desktop_sync_status'] == 'applied', 'Preview reservation leaked')
            clean()
            ack()
        print(json.dumps(dict(round=cycle+1, idle_previews=2)), flush=True)
    args.report.write_text(json.dumps(dict(instance=initial['context']['instance_id'],
        cases=results, idle_previews=2*args.rounds, final=status()), ensure_ascii=False, indent=2), encoding='utf-8')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--data-dir', type=Path, required=True)
    parser.add_argument('--panes', nargs=2, required=True)
    parser.add_argument('--fixtures', nargs=2, required=True, help='Exact fixture basenames')
    parser.add_argument('--rounds', type=int, default=5)
    parser.add_argument('--report', type=Path, required=True)
    run(parser.parse_args())
