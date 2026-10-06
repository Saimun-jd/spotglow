import ctypes
from ctypes import wintypes
user32 = ctypes.windll.user32
hDesk = user32.OpenDesktopW('Default', 0, False, 0x01FF)
if hDesk:
    user32.SetThreadDesktop(hDesk)

user32.IsWindow.argtypes = [wintypes.HWND]
user32.IsWindow.restype = wintypes.BOOL
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
user32.GetWindowThreadProcessId.restype = wintypes.DWORD

import subprocess
out = subprocess.check_output(['powershell', '-Command', '(Get-Process *spotglow*).Id']).decode().strip()
pids = [int(p) for p in out.split() if p.isdigit()]
print("Current SpotGlow PIDs:", pids)

def enum_proc(hwnd, lparam):
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    if pid.value in pids:
        vis = user32.IsWindowVisible(hwnd)
        r = wintypes.RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(r))
        buf = ctypes.create_unicode_buffer(256)
        user32.GetWindowTextW(hwnd, buf, 256)
        cls = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, cls, 256)
        print(f"HWND: {hex(hwnd)} | Vis: {vis} | Bounds: [{r.left}, {r.top}, {r.right}, {r.bottom}] ({r.right-r.left}x{r.bottom-r.top}) | Title: '{buf.value}' | Class: '{cls.value}'")
    return True

WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)
user32.EnumWindows(WNDENUMPROC(enum_proc), 0)
