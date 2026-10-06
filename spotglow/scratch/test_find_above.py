import ctypes, time
from ctypes import wintypes
user32 = ctypes.windll.user32
hDesk = user32.OpenDesktopW('Default', 0, False, 0x01FF)
if hDesk:
    user32.SetThreadDesktop(hDesk)

user32.GetWindow.argtypes = [wintypes.HWND, wintypes.UINT]
user32.GetWindow.restype = wintypes.HWND

def find_window_above(spotify_hwnd):
    curr = spotify_hwnd
    GW_HWNDPREV = 3
    while True:
        prev = user32.GetWindow(curr, GW_HWNDPREV)
        if not prev:
            return None
        vis = user32.IsWindowVisible(prev)
        cls = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(prev, cls, 256)
        title = ctypes.create_unicode_buffer(256)
        user32.GetWindowTextW(prev, title, 256)
        # Skip invisible, IME, and tooltip windows
        if vis and cls.value not in ['IME', 'MSCTFIME UI', 'tooltips_class32']:
            return prev
        curr = prev

spotify_hwnd = 0xd0d40
above = find_window_above(spotify_hwnd)
print("Above window:", hex(above) if above else "None (Spotify is top)")
