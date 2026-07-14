using System;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;

namespace BrutalAwake
{
    static class Program
    {
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern uint SetThreadExecutionState(uint esFlags);

        const uint ES_CONTINUOUS        = 0x80000000;
        const uint ES_SYSTEM_REQUIRED   = 0x00000001;
        const uint ES_DISPLAY_REQUIRED  = 0x00000002;
        const uint ES_AWAYMODE_REQUIRED = 0x00000040;

        const string SUB_BUTTONS = "4f971e89-eebd-4455-a8de-9e59040e7347";
        const string LIDACTION   = "5ca83367-6e45-459f-a27b-476b1d01c936";

        static readonly object stateLock = new object();
        static bool isOn = false;
        static int savedLidAc = -1, savedLidDc = -1;
        static ManualResetEvent quitEvent = new ManualResetEvent(false);

        // SetThreadExecutionState is per-thread: keep one dedicated thread
        // owning the state and re-asserting it periodically.
        static AutoResetEvent assertNow = new AutoResetEvent(false);

        static void Main()
        {
            var keeper = new Thread(KeeperLoop) { IsBackground = true };
            keeper.Start();

            int port = GetFreePort();
            var listener = new HttpListener();
            listener.Prefixes.Add("http://127.0.0.1:" + port + "/");
            listener.Start();

            var serverThread = new Thread(() => ServeLoop(listener)) { IsBackground = true };
            serverThread.Start();

            Process browser = LaunchBrowser("http://127.0.0.1:" + port + "/");

            if (browser != null)
            {
                // Exit when either the window closes or the page beacons /api/quit.
                var waitThread = new Thread(() =>
                {
                    try { browser.WaitForExit(); } catch { }
                    quitEvent.Set();
                }) { IsBackground = true };
                waitThread.Start();
            }

            quitEvent.WaitOne();

            lock (stateLock) { if (isOn) TurnOff(); }
            try { listener.Stop(); } catch { }
        }

        static void KeeperLoop()
        {
            while (true)
            {
                bool on;
                lock (stateLock) on = isOn;
                SetThreadExecutionState(on
                    ? ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED | ES_AWAYMODE_REQUIRED
                    : ES_CONTINUOUS);
                assertNow.WaitOne(30000);
            }
        }

        static void ServeLoop(HttpListener listener)
        {
            while (listener.IsListening)
            {
                HttpListenerContext ctx;
                try { ctx = listener.GetContext(); } catch { break; }
                ThreadPool.QueueUserWorkItem(_ => Handle(ctx));
            }
        }

        static void Handle(HttpListenerContext ctx)
        {
            try
            {
                string path = ctx.Request.Url.AbsolutePath;
                if (path == "/")
                {
                    byte[] page = Encoding.UTF8.GetBytes(LoadResource("ui.html", "<h1>ui.html missing</h1>"));
                    ctx.Response.ContentType = "text/html; charset=utf-8";
                    ctx.Response.OutputStream.Write(page, 0, page.Length);
                }
                else if (path == "/liquid-glass.js")
                {
                    byte[] js = Encoding.UTF8.GetBytes(LoadResource("liquid-glass.js", ""));
                    ctx.Response.ContentType = "application/javascript; charset=utf-8";
                    ctx.Response.OutputStream.Write(js, 0, js.Length);
                }
                else if (path == "/api/state")
                {
                    WriteJson(ctx);
                }
                else if (path == "/api/toggle" && ctx.Request.HttpMethod == "POST")
                {
                    lock (stateLock) { if (isOn) TurnOff(); else TurnOn(); }
                    assertNow.Set();
                    WriteJson(ctx);
                }
                else if (path == "/api/quit" && ctx.Request.HttpMethod == "POST")
                {
                    ctx.Response.StatusCode = 204;
                    quitEvent.Set();
                }
                else
                {
                    ctx.Response.StatusCode = 404;
                }
            }
            catch { }
            finally { try { ctx.Response.Close(); } catch { } }
        }

        static void WriteJson(HttpListenerContext ctx)
        {
            bool on; lock (stateLock) on = isOn;
            byte[] body = Encoding.UTF8.GetBytes("{\"on\":" + (on ? "true" : "false") + "}");
            ctx.Response.ContentType = "application/json";
            ctx.Response.OutputStream.Write(body, 0, body.Length);
        }

        static string LoadResource(string name, string fallback)
        {
            // Embedded at compile time via /res:; fall back to the file on disk.
            var asm = Assembly.GetExecutingAssembly();
            using (Stream s = asm.GetManifestResourceStream(name))
            {
                if (s != null)
                    using (var r = new StreamReader(s, Encoding.UTF8)) return r.ReadToEnd();
            }
            string beside = Path.Combine(Path.GetDirectoryName(asm.Location), name);
            if (File.Exists(beside)) return File.ReadAllText(beside, Encoding.UTF8);
            return fallback;
        }

        static Process LaunchBrowser(string url)
        {
            string[] candidates =
            {
                Environment.ExpandEnvironmentVariables(@"%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe"),
                Environment.ExpandEnvironmentVariables(@"%ProgramFiles%\Microsoft\Edge\Application\msedge.exe"),
                Environment.ExpandEnvironmentVariables(@"%ProgramFiles%\Google\Chrome\Application\chrome.exe"),
                Environment.ExpandEnvironmentVariables(@"%LocalAppData%\Google\Chrome\Application\chrome.exe"),
            };
            // Dedicated profile dir forces a new browser process we can wait on.
            string profile = Path.Combine(Path.GetTempPath(), "BrutalAwakeUI");
            foreach (string exe in candidates)
            {
                if (!File.Exists(exe)) continue;
                try
                {
                    return Process.Start(new ProcessStartInfo
                    {
                        FileName = exe,
                        Arguments = "--app=" + url +
                                    " --user-data-dir=\"" + profile + "\"" +
                                    " --window-size=420,620 --disable-extensions --no-first-run --no-default-browser-check",
                        UseShellExecute = false
                    });
                }
                catch { }
            }
            // Last resort: default browser tab (blur-only fallback outside Chromium).
            try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); } catch { }
            return null;
        }

        static int GetFreePort()
        {
            var l = new TcpListener(IPAddress.Loopback, 0);
            l.Start();
            int port = ((IPEndPoint)l.LocalEndpoint).Port;
            l.Stop();
            return port;
        }

        static void TurnOn()
        {
            SaveLidSettings();
            SetLidAction(0); // 0 = do nothing when the lid closes
            isOn = true;
        }

        static void TurnOff()
        {
            RestoreLidSettings();
            isOn = false;
        }

        static void SaveLidSettings()
        {
            string output = RunPowercfg("/query SCHEME_CURRENT " + SUB_BUTTONS + " " + LIDACTION);
            Match ac = Regex.Match(output, @"Current AC Power Setting Index:\s*0x([0-9a-fA-F]+)");
            Match dc = Regex.Match(output, @"Current DC Power Setting Index:\s*0x([0-9a-fA-F]+)");
            if (ac.Success) savedLidAc = Convert.ToInt32(ac.Groups[1].Value, 16);
            if (dc.Success) savedLidDc = Convert.ToInt32(dc.Groups[1].Value, 16);
        }

        static void SetLidAction(int value)
        {
            RunPowercfg("/setacvalueindex SCHEME_CURRENT " + SUB_BUTTONS + " " + LIDACTION + " " + value);
            RunPowercfg("/setdcvalueindex SCHEME_CURRENT " + SUB_BUTTONS + " " + LIDACTION + " " + value);
            RunPowercfg("/setactive SCHEME_CURRENT");
        }

        static void RestoreLidSettings()
        {
            if (savedLidAc >= 0)
                RunPowercfg("/setacvalueindex SCHEME_CURRENT " + SUB_BUTTONS + " " + LIDACTION + " " + savedLidAc);
            if (savedLidDc >= 0)
                RunPowercfg("/setdcvalueindex SCHEME_CURRENT " + SUB_BUTTONS + " " + LIDACTION + " " + savedLidDc);
            RunPowercfg("/setactive SCHEME_CURRENT");
        }

        static string RunPowercfg(string args)
        {
            try
            {
                var psi = new ProcessStartInfo("powercfg", args)
                {
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    RedirectStandardOutput = true
                };
                using (var p = Process.Start(psi))
                {
                    string output = p.StandardOutput.ReadToEnd();
                    p.WaitForExit();
                    return output;
                }
            }
            catch { return ""; }
        }
    }
}
