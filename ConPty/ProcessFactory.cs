using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

namespace AwayTerminal.ConPty;

/// <summary>以指定的 ConPTY 啟動子行程。</summary>
internal static class ProcessFactory
{
    public static PROCESS_INFORMATION Start(string commandLine, IntPtr hPC, string? workingDirectory)
    {
        var startupInfo = new STARTUPINFOEX();
        startupInfo.StartupInfo.cb = Marshal.SizeOf<STARTUPINFOEX>();

        // 先問所需的屬性清單大小（第一次呼叫預期回傳 false）
        IntPtr attrListSize = IntPtr.Zero;
        NativeMethods.InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref attrListSize);
        startupInfo.lpAttributeList = Marshal.AllocHGlobal(attrListSize);

        try
        {
            if (!NativeMethods.InitializeProcThreadAttributeList(startupInfo.lpAttributeList, 1, 0, ref attrListSize))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "InitializeProcThreadAttributeList 失敗");

            // 綁定 pseudoconsole 屬性
            if (!NativeMethods.UpdateProcThreadAttribute(
                    startupInfo.lpAttributeList,
                    0,
                    NativeMethods.PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                    hPC,
                    (IntPtr)IntPtr.Size,
                    IntPtr.Zero,
                    IntPtr.Zero))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "UpdateProcThreadAttribute 失敗");

            string? cwd = string.IsNullOrWhiteSpace(workingDirectory) ? null : workingDirectory;

            bool ok = NativeMethods.CreateProcess(
                null,
                new StringBuilder(commandLine),
                IntPtr.Zero,
                IntPtr.Zero,
                false,
                NativeMethods.EXTENDED_STARTUPINFO_PRESENT,
                IntPtr.Zero,
                cwd,
                ref startupInfo,
                out PROCESS_INFORMATION procInfo);

            if (!ok)
                throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateProcess 失敗");

            return procInfo;
        }
        finally
        {
            NativeMethods.DeleteProcThreadAttributeList(startupInfo.lpAttributeList);
            Marshal.FreeHGlobal(startupInfo.lpAttributeList);
        }
    }
}
