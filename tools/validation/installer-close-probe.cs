// An isolated process used by test-installer.ps1. It never touches Explorer or app configuration.
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Threading;

class InstallerCloseProbe {
    delegate IntPtr WindowProc(IntPtr hwnd, uint message, UIntPtr wp, IntPtr lp);
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WindowClass {
        public uint size, style;
        public WindowProc callback;
        public int classBytes, windowBytes;
        public IntPtr instance, icon, cursor, background;
        public string menu, name;
        public IntPtr smallIcon;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct Message {
        public IntPtr hwnd;
        public uint message;
        public UIntPtr wp;
        public IntPtr lp;
        public uint time;
        public int x, y;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern ushort RegisterClassExW(ref WindowClass wc);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr CreateWindowExW(uint ex, string cls, string title, uint style,
        int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr data);
    [DllImport("user32.dll")]
    static extern bool DestroyWindow(IntPtr hwnd);
    [DllImport("user32.dll")]
    static extern void PostQuitMessage(int code);
    [DllImport("user32.dll")]
    static extern int GetMessageW(out Message message, IntPtr hwnd, uint min, uint max);
    [DllImport("user32.dll")]
    static extern bool TranslateMessage(ref Message message);
    [DllImport("user32.dll")]
    static extern IntPtr DispatchMessageW(ref Message message);
    [DllImport("user32.dll")]
    static extern IntPtr DefWindowProcW(IntPtr hwnd, uint message, UIntPtr wp, IntPtr lp);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr GetModuleHandleW(string name);
    static string[] args;
    static readonly WindowProc callback = HandleMessage;
    static IntPtr HandleMessage(IntPtr hwnd, uint message, UIntPtr wp, IntPtr lp) {
        if (message == 0x0010) {
            File.WriteAllText(args[4] + ".requested", "normal close requested");
            if (args[7] != "ignore") DestroyWindow(hwnd);
            return IntPtr.Zero;
        }
        if (message == 0x0002) {
            PostQuitMessage(0);
            return IntPtr.Zero;
        }
        return DefWindowProcW(hwnd, message, wp, lp);
    }
    static void Main(string[] arguments) {
        args = arguments;
        Mutex mutex = null;
        try {
            mutex = new Mutex(false, args[0]);
            var wc = new WindowClass { size = (uint)Marshal.SizeOf(typeof(WindowClass)),
                callback = callback, instance = GetModuleHandleW(null), name = args[1] };
            if (RegisterClassExW(ref wc) == 0 || CreateWindowExW(0x08000080, args[1], args[2],
                0x80000000, 0, 0, 1, 1, IntPtr.Zero, IntPtr.Zero, wc.instance, IntPtr.Zero) == IntPtr.Zero)
                throw new InvalidOperationException("Could not create the test window");
            File.WriteAllText(args[3], "ready");
            Message message;
            while (GetMessageW(out message, IntPtr.Zero, 0, 0) > 0) {
                TranslateMessage(ref message);
                DispatchMessageW(ref message);
            }
            // Release the mutex before teardown finishes. Waiting only for the mutex is insufficient.
            mutex.Dispose(); mutex = null;
            Thread.Sleep(500);
            using (var sha = SHA256.Create()) {
                string hash = BitConverter.ToString(sha.ComputeHash(File.ReadAllBytes(args[5]))).Replace("-", "");
                if (!String.Equals(hash, args[6], StringComparison.OrdinalIgnoreCase))
                    throw new InvalidOperationException("Installer replaced files before process exit");
            }
            File.WriteAllText(args[4], "normal exit completed");
        } catch (Exception error) {
            File.WriteAllText(args[4], "failure: " + error.Message);
            Environment.ExitCode = 1;
        } finally {
            if (mutex != null) mutex.Dispose();
        }
    }
}
