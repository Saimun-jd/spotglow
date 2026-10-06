import ctypes, sys, time
from ctypes import wintypes

kernel32 = ctypes.windll.kernel32

exe_path = r"C:\Users\user\Documents\contraband\spotglow\src-tauri\target\debug\spotglow.exe"

hReadPipe, hWritePipe = wintypes.HANDLE(), wintypes.HANDLE()
class SECURITY_ATTRIBUTES(ctypes.Structure):
    _fields_ = [('nLength', wintypes.DWORD), ('lpSecurityDescriptor', ctypes.c_void_p), ('bInheritHandle', wintypes.BOOL)]

sa = SECURITY_ATTRIBUTES()
sa.nLength = ctypes.sizeof(SECURITY_ATTRIBUTES)
sa.bInheritHandle = True
sa.lpSecurityDescriptor = None

kernel32.CreatePipe(ctypes.byref(hReadPipe), ctypes.byref(hWritePipe), ctypes.byref(sa), 0)
kernel32.SetHandleInformation(hReadPipe, 1, 0)

class STARTUPINFOW(ctypes.Structure):
    _fields_ = [
        ('cb', wintypes.DWORD),
        ('lpReserved', wintypes.LPWSTR),
        ('lpDesktop', wintypes.LPWSTR),
        ('lpTitle', wintypes.LPWSTR),
        ('dwX', wintypes.DWORD),
        ('dwY', wintypes.DWORD),
        ('dwXSize', wintypes.DWORD),
        ('dwYSize', wintypes.DWORD),
        ('dwXCountChars', wintypes.DWORD),
        ('dwYCountChars', wintypes.DWORD),
        ('dwFillAttribute', wintypes.DWORD),
        ('dwFlags', wintypes.DWORD),
        ('wShowWindow', wintypes.WORD),
        ('cbReserved2', wintypes.WORD),
        ('lpReserved2', ctypes.POINTER(ctypes.c_byte)),
        ('hStdInput', wintypes.HANDLE),
        ('hStdOutput', wintypes.HANDLE),
        ('hStdError', wintypes.HANDLE),
    ]

class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [
        ('hProcess', wintypes.HANDLE),
        ('hThread', wintypes.HANDLE),
        ('dwProcessId', wintypes.DWORD),
        ('dwThreadId', wintypes.DWORD),
    ]

si = STARTUPINFOW()
si.cb = ctypes.sizeof(STARTUPINFOW)
si.lpDesktop = r"WinSta0\Default"
si.dwFlags = 0x00000100 # STARTF_USESTDHANDLES
si.hStdOutput = hWritePipe
si.hStdError = hWritePipe

pi = PROCESS_INFORMATION()

print(f"[Launcher] Launching {exe_path} on WinSta0\\Default...", flush=True)
res = kernel32.CreateProcessW(
    exe_path,
    None,
    None,
    None,
    True,
    0,
    None,
    r"C:\Users\user\Documents\contraband\spotglow\src-tauri",
    ctypes.byref(si),
    ctypes.byref(pi),
)

kernel32.CloseHandle(hWritePipe)

if not res:
    err = kernel32.GetLastError()
    print(f"[Launcher] CreateProcessW failed: {err}", flush=True)
    sys.exit(1)

print(f"[Launcher] SpotGlow started with PID {pi.dwProcessId} on WinSta0\\Default", flush=True)

buf = ctypes.create_string_buffer(4096)
bytesRead = wintypes.DWORD()

while True:
    avail = wintypes.DWORD()
    if kernel32.PeekNamedPipe(hReadPipe, None, 0, None, ctypes.byref(avail), None) and avail.value > 0:
        if kernel32.ReadFile(hReadPipe, buf, min(4096, avail.value), ctypes.byref(bytesRead), None):
            sys.stdout.write(buf.raw[:bytesRead.value].decode('utf-8', errors='replace'))
            sys.stdout.flush()
    code = wintypes.DWORD()
    kernel32.GetExitCodeProcess(pi.hProcess, ctypes.byref(code))
    if code.value != 259:
        print(f"[Launcher] SpotGlow exited with code {code.value}", flush=True)
        break
    time.sleep(0.1)

kernel32.CloseHandle(pi.hProcess)
kernel32.CloseHandle(pi.hThread)
kernel32.CloseHandle(hReadPipe)
