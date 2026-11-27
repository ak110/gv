using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Automation;

public sealed class BookmarkProbe
{
    private const uint WmClose = 0x0010;
    private const uint WmSetText = 0x000C;
    private const uint WmKeyDown = 0x0100;
    private const uint WmCommand = 0x0111;
    private const int FileNameComboId = 1148;
    private const int FileNameEditId = 1152;
    private const int OkId = 1;
    private const int DesktopUnavailable = 20;

    private readonly IntPtr desktop;
    private readonly bool isolated;
    private ProcessInformation child;
    private IntPtr main;

    public static int Run(string exe, string caseDir, bool isolated)
    {
        var result = 1;
        var desktopName = isolated ? "gv-bookmark-probe-" + Guid.NewGuid().ToString("N") : null;
        var thread = new Thread(() =>
        {
            var desktop = isolated
                ? CreateDesktopW(desktopName, IntPtr.Zero, IntPtr.Zero, 0, 0x01FF, IntPtr.Zero)
                : GetThreadDesktop(GetCurrentThreadId());
            if (desktop == IntPtr.Zero)
            {
                Console.WriteLine("デスクトップへの接続失敗: Win32=" + Marshal.GetLastWin32Error());
                result = isolated ? DesktopUnavailable : 1;
                return;
            }

            try
            {
                if (isolated && !SetThreadDesktop(desktop))
                {
                    Console.WriteLine("隔離デスクトップへの切り替え失敗: Win32=" + Marshal.GetLastWin32Error());
                    result = DesktopUnavailable;
                    return;
                }

                var probe = new BookmarkProbe(desktop, isolated);
                result = probe.Observe(exe, caseDir, desktopName);
            }
            catch (Exception e)
            {
                var detail = e is Win32Exception ? " Win32=" + ((Win32Exception)e).NativeErrorCode : "";
                Console.WriteLine("観測中断: " + e.GetType().Name + detail);
            }
        });
        thread.SetApartmentState(ApartmentState.MTA);
        thread.Start();
        thread.Join();
        // 隔離デスクトップのハンドルは、所属したスレッドが終わった後にワーカープロセスの終了で閉じる。
        return result;
    }

    private BookmarkProbe(IntPtr desktop, bool isolated)
    {
        this.desktop = desktop;
        this.isolated = isolated;
    }

    private int Observe(string exe, string caseDir, string desktopName)
    {
        var bookmarkDir = Path.Combine(Path.GetDirectoryName(exe), "bookmarks");
        Directory.CreateDirectory(bookmarkDir);
        Directory.CreateDirectory(caseDir);

        var colors = new[] { Color.Red, Color.Green, Color.Blue };
        var images = new string[3];
        for (var i = 0; i < images.Length; i++)
        {
            images[i] = Path.Combine(caseDir, (i + 1) + ".png");
            using (var bitmap = new Bitmap(150 + i * 10, 100))
            {
                using (var graphics = Graphics.FromImage(bitmap))
                    graphics.Clear(colors[i]);
                bitmap.Save(images[i], ImageFormat.Png);
            }
        }

        var bookmark = Path.Combine(caseDir, "saved.gvbm");
        if (File.Exists(bookmark))
            throw new InvalidOperationException("出力先に既存のしおりがある。");

        var args = new StringBuilder(string.Join(" ", new[] { exe }.Concat(images).Select(p => "\"" + p + "\"")));
        var startup = new StartupInfo { Size = Marshal.SizeOf(typeof(StartupInfo)), Desktop = desktopName };
        if (!CreateProcessW(exe, args, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero,
            Path.GetDirectoryName(exe), ref startup, out child))
            throw new Win32Exception();

        try
        {
            Console.WriteLine("起動後の表示位置を確認");
            WaitFor(() => (main = OwnWindows().FirstOrDefault(h => GetMenu(h) != IntPtr.Zero)) != IntPtr.Zero);
            ExpectImage(images[0], "[1/3]");
            SelectMenu("最後へ");
            ExpectImage(images[2], "[3/3]");
            Console.WriteLine("保存前: 3.png [3/3]");

            if (!SelectBookmark("Save As", "Save", 0x78, bookmark))
                return isolated ? DesktopUnavailable : 1;

            WaitFor(() => File.Exists(bookmark) && IsWindowEnabled(main));
            var lines = File.ReadAllLines(bookmark, new UTF8Encoding(false, true));
            if (lines.Length != 5 || lines[0] != "# gv3 bookmark v1" || lines[1] != "# index: 2" ||
                !images.Select(p => "file\t" + p).SequenceEqual(lines.Skip(2), StringComparer.OrdinalIgnoreCase))
            {
                Console.WriteLine("保存内容の不一致: 行数=" + lines.Length +
                    " index:2=" + (lines.Length > 1 && lines[1] == "# index: 2"));
                return 1;
            }
            Console.WriteLine("保存確認: saved.gvbm、1.png → 2.png → 3.png、# index: 2");

            SelectMenu("最初へ");
            ExpectImage(images[0], "[1/3]");
            Console.WriteLine("読み込み前: 1.png [1/3]");

            if (!SelectBookmark("Open", "Open", 0x7B, bookmark))
                return isolated ? DesktopUnavailable : 1;

            ExpectImage(images[2], "[3/3]");
            Console.WriteLine("読み込み確認: 3.png [3/3]");
            return 0;
        }
        finally
        {
            try
            {
                if (main != IntPtr.Zero)
                    PostMessageW(main, WmClose, IntPtr.Zero, IntPtr.Zero);
                if (WaitForSingleObject(child.Process, 2500) != 0)
                {
                    if (!TerminateProcess(child.Process, 1) && WaitForSingleObject(child.Process, 0) != 0)
                        throw new Win32Exception();
                    if (WaitForSingleObject(child.Process, 5000) != 0)
                        throw new TimeoutException("gvの終了を確認できない。");
                    Console.WriteLine("gv を強制終了して回収");
                }
            }
            finally
            {
                CloseHandle(child.Thread);
                CloseHandle(child.Process);
            }
        }
    }

    private bool SelectBookmark(string title, string buttonName, int key, string bookmark)
    {
        Console.WriteLine("標準ダイアログを待機: " + title);
        Post(main, WmKeyDown, key, (IntPtr)1);

        IntPtr dialog = IntPtr.Zero;
        if (!Poll(() => (dialog = OwnWindows().FirstOrDefault(
            h => GetWindow(h, 4) == main && IsWindowVisible(h) &&
                 WindowClass(h) == "#32770" && WindowText(h) == title)) != IntPtr.Zero))
        {
            Console.WriteLine("標準ダイアログを確認できなかった: " + title +
                " gvの表示ウィンドウ数=" + OwnWindows().Count(IsWindowVisible));
            return false;
        }

        Console.WriteLine("標準ダイアログ確認: #32770 / " + title);
        if (Poll(() => TrySubmit(dialog, buttonName, bookmark)))
        {
            WaitFor(() => !IsWindowVisible(dialog));
            Console.WriteLine("ダイアログ確定: " + title);
            return true;
        }

        DumpControls(dialog);
        throw new InvalidOperationException("ファイル名と確定ボタンを識別できない。");
    }

    private static bool TrySubmit(IntPtr dialog, string buttonName, string bookmark)
    {
        ValuePattern value = null;
        InvokePattern invoke = null;
        try
        {
            var root = AutomationElement.FromHandle(dialog);
            var edits = root.FindAll(TreeScope.Descendants,
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Edit));
            var buttons = root.FindAll(TreeScope.Descendants,
                new AndCondition(
                    new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Button),
                    new OrCondition(
                        new PropertyCondition(AutomationElement.NameProperty, buttonName),
                        new PropertyCondition(AutomationElement.AutomationIdProperty, "1"))));
            if (buttons.Count == 1)
            {
                var candidates = edits.Cast<AutomationElement>().Where(IsFileNameEdit).ToArray();
                object valuePattern, invokePattern;
                if (candidates.Length == 1 &&
                    candidates[0].TryGetCurrentPattern(ValuePattern.Pattern, out valuePattern) &&
                    buttons[0].TryGetCurrentPattern(InvokePattern.Pattern, out invokePattern))
                {
                    value = (ValuePattern)valuePattern;
                    invoke = (InvokePattern)invokePattern;
                }
            }
        }
        catch (ElementNotAvailableException)
        {
        }
        catch (COMException e)
        {
            Console.WriteLine("UI Automation の要素取得失敗: HRESULT=0x" + e.HResult.ToString("X8"));
        }

        if (value != null)
        {
            value.SetValue(bookmark);
            if (!string.Equals(value.Current.Value, bookmark, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("ファイル名欄に入力したパスが一致しない。");
            invoke.Invoke();
            Console.WriteLine("確定操作: UI Automation File name / " + buttonName);
            return true;
        }

        var children = ChildWindows(dialog).Where(IsWindowVisible).ToArray();
        var combo = FindControl(children, FileNameComboId, "ComboBox");
        var editsInCombo = combo == IntPtr.Zero
            ? new IntPtr[0]
            : ChildWindows(combo).Where(h => WindowClass(h) == "Edit" && IsWindowVisible(h)).ToArray();
        var edit = editsInCombo.Length == 1 ? editsInCombo[0] : FindControl(children, FileNameEditId, "Edit");
        var button = GetDlgItem(dialog, OkId);
        if (edit == IntPtr.Zero || button == IntPtr.Zero || WindowClass(button) != "Button" ||
            !IsWindowVisible(button) || !IsWindowEnabled(button))
            return false;

        IntPtr applied;
        if (SendMessageTimeoutW(edit, WmSetText, IntPtr.Zero, bookmark, 2, 2000, out applied) == IntPtr.Zero ||
            applied == IntPtr.Zero)
            throw new Win32Exception();

        // 非入力デスクトップでは BM_CLICK が失敗し得る。対象ボタンの BN_CLICKED 通知を親にだけ届ける。
        Post(dialog, WmCommand, OkId, button);
        Console.WriteLine("確定操作: Win32 File name / IDOK");
        return true;
    }

    private static IntPtr FindControl(IEnumerable<IntPtr> children, int id, string className)
    {
        var match = children.Where(h => GetDlgCtrlID(h) == id && WindowClass(h) == className).ToArray();
        return match.Length == 1 ? match[0] : IntPtr.Zero;
    }

    private static bool IsFileNameEdit(AutomationElement element)
    {
        var name = element.Current.Name.Replace("&", "").TrimEnd(':').Trim();
        if (string.Equals(name, "File name", StringComparison.OrdinalIgnoreCase))
            return true;

        var parent = TreeWalker.RawViewWalker.GetParent(element);
        return parent != null && parent.Current.AutomationId == FileNameComboId.ToString();
    }

    private void ExpectImage(string image, string position)
    {
        WaitFor(() => WindowText(main).StartsWith(image + " " + position + " ", StringComparison.OrdinalIgnoreCase));
    }

    private void SelectMenu(string label)
    {
        var menu = GetMenu(main);
        for (var i = 0; i < GetMenuItemCount(menu); i++)
        {
            var submenu = GetSubMenu(menu, i);
            for (var j = 0; j < GetMenuItemCount(submenu); j++)
            {
                var text = new StringBuilder(256);
                GetMenuStringW(submenu, (uint)j, text, text.Capacity, 0x0400);
                if (text.ToString().Split('\t')[0] != label)
                    continue;

                var id = GetMenuItemID(submenu, j);
                if (id == uint.MaxValue)
                    break;
                Post(main, WmCommand, checked((int)id), IntPtr.Zero);
                return;
            }
        }
        throw new InvalidOperationException("移動メニューが見つからない。");
    }

    private List<IntPtr> OwnWindows()
    {
        var windows = new List<IntPtr>();
        if (!EnumDesktopWindows(desktop, (h, _) =>
        {
            uint pid;
            GetWindowThreadProcessId(h, out pid);
            if (pid == child.Id)
                windows.Add(h);
            return true;
        }, IntPtr.Zero))
            throw new Win32Exception();
        return windows;
    }

    private static List<IntPtr> ChildWindows(IntPtr parent)
    {
        var windows = new List<IntPtr>();
        EnumChildWindows(parent, (h, _) => { windows.Add(h); return true; }, IntPtr.Zero);
        return windows;
    }

    private static void DumpControls(IntPtr dialog)
    {
        foreach (var h in ChildWindows(dialog).Where(IsWindowVisible))
            Console.WriteLine("dialog control: class=" + WindowClass(h) + " id=" + GetDlgCtrlID(h));
    }

    private static void WaitFor(Func<bool> ready)
    {
        if (!Poll(ready))
            throw new TimeoutException("観測箇所が待機時間内に所定の状態にならない。");
    }

    private static bool Poll(Func<bool> ready)
    {
        var elapsed = Stopwatch.StartNew();
        do
        {
            if (ready())
                return true;
            Thread.Sleep(100);
        } while (elapsed.ElapsedMilliseconds < 12000);
        return false;
    }

    private static void Post(IntPtr window, uint message, int wparam, IntPtr lparam)
    {
        if (!PostMessageW(window, message, (IntPtr)wparam, lparam))
            throw new Win32Exception();
    }

    private static string WindowText(IntPtr window)
    {
        var text = new StringBuilder(1024);
        GetWindowTextW(window, text, text.Capacity);
        return text.ToString();
    }

    private static string WindowClass(IntPtr window)
    {
        var text = new StringBuilder(256);
        GetClassNameW(window, text, text.Capacity);
        return text.ToString();
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct StartupInfo
    {
        public int Size;
        public string Reserved, Desktop, Title;
        public int X, Y, Width, Height, CharsX, CharsY, FillAttribute, Flags;
        public short ShowWindow, ReservedSize;
        public IntPtr ReservedData, StdInput, StdOutput, StdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessInformation
    {
        public IntPtr Process, Thread;
        public uint Id, ThreadId;
    }

    private delegate bool EnumWindowCallback(IntPtr window, IntPtr data);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    private static extern bool CreateProcessW(string app, StringBuilder args, IntPtr processAttrs, IntPtr threadAttrs,
        bool inheritHandles, uint flags, IntPtr environment, string cwd, ref StartupInfo startup, out ProcessInformation result);
    [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
    [DllImport("kernel32.dll")] private static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateProcess(IntPtr process, uint exitCode);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint WaitForSingleObject(IntPtr handle, uint millis);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    private static extern IntPtr CreateDesktopW(string name, IntPtr device, IntPtr devmode, uint flags, uint access, IntPtr attrs);
    [DllImport("user32.dll", SetLastError = true)] private static extern IntPtr GetThreadDesktop(uint thread);
    [DllImport("user32.dll", SetLastError = true)] private static extern bool SetThreadDesktop(IntPtr desktop);
    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool EnumDesktopWindows(IntPtr desktop, EnumWindowCallback callback, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumChildWindows(IntPtr window, EnumWindowCallback callback, IntPtr data);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] private static extern IntPtr GetWindow(IntPtr window, uint command);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] private static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int GetWindowTextW(IntPtr window, StringBuilder text, int chars);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int GetClassNameW(IntPtr window, StringBuilder text, int chars);
    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    private static extern bool PostMessageW(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    private static extern IntPtr SendMessageTimeoutW(IntPtr window, uint msg, IntPtr wparam, string lparam, uint flags,
        uint millis, out IntPtr result);
    [DllImport("user32.dll")] private static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll")] private static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] private static extern IntPtr GetMenu(IntPtr window);
    [DllImport("user32.dll")] private static extern IntPtr GetSubMenu(IntPtr menu, int pos);
    [DllImport("user32.dll")] private static extern int GetMenuItemCount(IntPtr menu);
    [DllImport("user32.dll")] private static extern uint GetMenuItemID(IntPtr menu, int pos);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int GetMenuStringW(IntPtr menu, uint item, StringBuilder text, int chars, uint flags);
}
