using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace AwayTerminal;

/// <summary>用 Toolhelp 快照判斷哪些行程「至少有一個子行程」（用來判斷分頁是綠/橘）。</summary>
internal static class ProcessTree
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct PROCESSENTRY32
    {
        public int dwSize;
        public int cntUsage;
        public int th32ProcessID;
        public IntPtr th32DefaultHeapID;
        public int th32ModuleID;
        public int cntThreads;
        public int th32ParentProcessID;
        public int pcPriClassBase;
        public int dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
        public string szExeFile;
    }

    private const uint TH32CS_SNAPPROCESS = 0x00000002;

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr CreateToolhelp32Snapshot(uint dwFlags, uint th32ProcessID);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool Process32First(IntPtr hSnapshot, ref PROCESSENTRY32 lppe);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool Process32Next(IntPtr hSnapshot, ref PROCESSENTRY32 lppe);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr hObject);

    /// <summary>回傳「有子行程」的父 PID 集合。session 的 PID 若在其中，代表正在跑外部程式。</summary>
    public static HashSet<int> ParentsWithChildren()
    {
        var parents = new HashSet<int>();
        IntPtr snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if (snap == IntPtr.Zero || snap == new IntPtr(-1)) return parents;
        try
        {
            var pe = new PROCESSENTRY32 { dwSize = Marshal.SizeOf<PROCESSENTRY32>() };
            if (Process32First(snap, ref pe))
            {
                do
                {
                    if (pe.th32ParentProcessID != 0)
                        parents.Add(pe.th32ParentProcessID);
                }
                while (Process32Next(snap, ref pe));
            }
        }
        finally { CloseHandle(snap); }
        return parents;
    }
}
