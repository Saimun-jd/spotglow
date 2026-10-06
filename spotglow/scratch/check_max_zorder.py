import ctypes, time
from ctypes import wintypes
user32 = ctypes.windll.user32
hDesk = user32.OpenDesktopW('Default', 0, False, 0x01FF)
if hDesk:
    user32.SetThreadDesktop(hDesk)

user32.GetWindow.argtypes = [wintypes.HWND, wintypes.UINT]
user32.GetWindow.restype = wintypes.HWND

spotify_hwnd = 0xd0d40
GW_HWNDPREV = 3

curr = spotify_hwnd
found = None
while True:
    prev = user32.GetWindow(curr, GW_HWNDPREV)
    if not prev:
        break
    vis = user32.IsWindowVisible(prev)
    buf = ctypes.create_unicode_buffer(256)
    user32.GetWindowTextW(prev, buf, 256)
    cls = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(prev, cls, 256)
    ex = user32.GetWindowLongPtrW(prev, -20)
    print(f'Candidate: {hex(prev)} | Vis: {vis} | Title: "{buf.value}" | Class: "{cls.value}"')
    if vis and cls.value != 'IME':
        found = prev
        break
    curr = prev

print('Found visible window above Spotify:', hex(found) if found else 'None (Spotify is top!)')
