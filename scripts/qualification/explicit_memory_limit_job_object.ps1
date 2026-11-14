# Runs INSIDE the build VM. GG-CORE #182 packaged qualification: explicit memory limits are
# never relaxed. Real release gg-core-cli.exe, real Qwen2.5-0.5B Q4_K_M, real named-pipe daemon.
# The Windows Job Object JobMemoryLimit is read back from outside the daemon by duplicating its
# job handle (the daemon's job is anonymous and limited to one process).
param(
    [string]$Exe = 'E:\w\target-ggv1-182\release\gg-core-cli.exe',
    [string]$Out = 'C:\out\g182qual'
)
$ErrorActionPreference = 'Continue'
$root = 'C:\g182qual'
Remove-Item -Recurse -Force $root, $Out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$root\models\qwen", $Out | Out-Null
$log = "$Out\qual.log"; $jsonl = "$Out\results.jsonl"
function Note($m) { "$(Get-Date -Format o) $m" | Tee-Object -FilePath $log -Append }
function Rec($name, $h) { $h['test'] = $name; ($h | ConvertTo-Json -Compress -Depth 4) | Add-Content $jsonl; Note ('RESULT ' + ($h | ConvertTo-Json -Compress -Depth 4)) }
function Short($s, [int]$n = 300) { if ($null -eq $s) { return '' }; $s = [string]$s -replace '\s+', ' '; if ($s.Length -gt $n) { $s.Substring(0, $n) + '...' } else { $s } }

Copy-Item $Exe "$root\gg-core-cli.exe" -Force
Copy-Item C:\host\vcdlls\*.dll $root -Force
$model = "$root\models\qwen\model.gguf"
Copy-Item C:\Preview\models\qwen2.5-0.5b-instruct-q4_k_m.gguf $model -Force
$sha = (Get-FileHash $model -Algorithm SHA256).Hash.ToLower(); $size = (Get-Item $model).Length
@{ model_id = 'qwen'; name = 'Qwen2.5-0.5B-Instruct Q4_K_M'; version = '9217f5db'; capabilities = @('text_generation');
   sha256 = $sha; size_bytes = $size; architecture = 'gguf'; license = 'Apache-2.0' } | ConvertTo-Json | Set-Content "$root\models\qwen\manifest.json" -Encoding ascii
$exeSha = (Get-FileHash "$root\gg-core-cli.exe" -Algorithm SHA256).Hash.ToLower()
Rec 'provenance' @{ exe_sha256 = $exeSha; exe_bytes = (Get-Item "$root\gg-core-cli.exe").Length; model_sha256 = $sha; model_bytes = $size
    model_expected = ($sha -eq '74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db'); version = (Short (& "$root\gg-core-cli.exe" version 2>&1 | Out-String)) }

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class JobProbe {
  [StructLayout(LayoutKind.Sequential)] struct HANDLE_ENTRY { public IntPtr Object; public IntPtr Pid; public IntPtr Handle; public uint Access; public ushort Backtrace; public ushort TypeIndex; public uint Attrs; public uint Reserved; }
  [StructLayout(LayoutKind.Sequential)] struct BASIC { public long PerProcessUserTimeLimit, PerJobUserTimeLimit; public uint LimitFlags; public UIntPtr MinWs, MaxWs; public uint ActiveProcessLimit; public UIntPtr Affinity; public uint PriorityClass, SchedulingClass; }
  [StructLayout(LayoutKind.Sequential)] struct IOC { public ulong a, b, c, d, e, f; }
  [StructLayout(LayoutKind.Sequential)] struct EXT { public BASIC Basic; public IOC Io; public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed; }
  [DllImport("ntdll.dll")] static extern int NtQuerySystemInformation(int c, IntPtr b, int l, out int r);
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, int pid);
  [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
  [DllImport("kernel32.dll")] static extern bool DuplicateHandle(IntPtr sp, IntPtr sh, IntPtr tp, out IntPtr th, uint a, bool i, uint o);
  [DllImport("kernel32.dll")] static extern bool QueryInformationJobObject(IntPtr j, int c, out EXT info, int l, out int r);
  [DllImport("kernel32.dll")] static extern bool IsProcessInJob(IntPtr p, IntPtr j, out bool r);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  // Returns "limitBytes|flags|activeProcessLimit|peakJobBytes" for the daemon's job, or an error.
  public static string Read(int pid) {
    int len = 1 << 24; IntPtr buf = Marshal.AllocHGlobal(len); int ret;
    try {
      int st;
      while ((st = NtQuerySystemInformation(64, buf, len, out ret)) == unchecked((int)0xC0000004)) { Marshal.FreeHGlobal(buf); len *= 2; buf = Marshal.AllocHGlobal(len); }
      if (st != 0) return "error: NtQuerySystemInformation " + st.ToString("X");
      IntPtr proc = OpenProcess(0x0040 | 0x1000, false, pid); // DUP_HANDLE | QUERY_LIMITED
      if (proc == IntPtr.Zero) return "error: OpenProcess " + Marshal.GetLastWin32Error();
      long count = Marshal.ReadIntPtr(buf).ToInt64(); int size = Marshal.SizeOf(typeof(HANDLE_ENTRY));
      IntPtr first = IntPtr.Add(buf, IntPtr.Size * 2);
      for (long i = 0; i < count; i++) {
        var e = (HANDLE_ENTRY)Marshal.PtrToStructure(new IntPtr(first.ToInt64() + i * size), typeof(HANDLE_ENTRY));
        if (e.Pid.ToInt64() != pid) continue;
        IntPtr dup;
        if (!DuplicateHandle(proc, e.Handle, GetCurrentProcess(), out dup, 0x0004, false, 0)) continue; // JOB_OBJECT_QUERY
        EXT info; int r2; bool inJob;
        bool ok = QueryInformationJobObject(dup, 9, out info, Marshal.SizeOf(typeof(EXT)), out r2);
        bool mine = ok && IsProcessInJob(proc, dup, out inJob) && inJob;
        CloseHandle(dup);
        if (mine) { CloseHandle(proc); return info.JobMemoryLimit.ToUInt64() + "|" + info.Basic.LimitFlags.ToString("X") + "|" + info.Basic.ActiveProcessLimit + "|" + info.PeakJobMemoryUsed.ToUInt64(); }
      }
      CloseHandle(proc); return "error: no job handle found";
    } finally { Marshal.FreeHGlobal(buf); }
  }
}
'@

$bytes = New-Object byte[] 32; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
$token = [Convert]::ToBase64String($bytes)
$memVars = 'GG_CORE_MAX_TOTAL_MEMORY', 'GG_CORE_MAX_MEMORY_PER_CALL'

function Run-Cli([string[]]$cliArgs, [hashtable]$vars = @{}, [string]$pipe = '') {
    $psi = New-Object Diagnostics.ProcessStartInfo "$root\gg-core-cli.exe"
    $psi.Arguments = ($cliArgs | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }) -join ' '
    $psi.WorkingDirectory = $root; $psi.UseShellExecute = $false; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
    foreach ($k in $memVars) { $psi.EnvironmentVariables.Remove($k) }
    $psi.EnvironmentVariables['CORE_AUTH_TOKEN'] = $token
    if ($pipe) { $psi.EnvironmentVariables['GG_CORE_SOCKET_PATH'] = $pipe }
    foreach ($k in $vars.Keys) { $psi.EnvironmentVariables[$k] = $vars[$k] }
    $sw = [Diagnostics.Stopwatch]::StartNew(); $p = [Diagnostics.Process]::Start($psi)
    $o = $p.StandardOutput.ReadToEndAsync(); $e = $p.StandardError.ReadToEndAsync()
    if (-not $p.WaitForExit(240000)) { $p.Kill(); return @{ code = 'timeout'; ms = $sw.ElapsedMilliseconds; out = ''; err = '' } }
    @{ code = $p.ExitCode; ms = $sw.ElapsedMilliseconds; out = $o.Result.Trim(); err = $e.Result.Trim() }
}

# 1. Invalid settings: serve must exit before listening; config show/validate exit 1.
$invalid = @(
    @{ GG_CORE_MAX_TOTAL_MEMORY = '268435456'; GG_CORE_MAX_MEMORY_PER_CALL = '536870912' },
    @{ GG_CORE_MAX_TOTAL_MEMORY = '0' }, @{ GG_CORE_MAX_MEMORY_PER_CALL = '0' },
    @{ GG_CORE_MAX_TOTAL_MEMORY = 'abc' }, @{ GG_CORE_MAX_TOTAL_MEMORY = '18446744073709551616' },
    @{ GG_CORE_MAX_MEMORY_PER_CALL = '10' }, @{ GG_CORE_MAX_MEMORY_PER_CALL = '99999999999999' })
foreach ($v in $invalid) {
    $pipe = '\\.\pipe\gg182-invalid-' + [guid]::NewGuid().ToString('N')
    $s = Run-Cli @('serve', '--model', 'models/qwen/model.gguf', '--model-id', 'qwen') $v $pipe
    $show = Run-Cli @('config', 'show') $v; $val = Run-Cli @('config', 'validate') $v
    Rec 'invalid_refused' @{ vars = ($v | ConvertTo-Json -Compress); serve_code = $s.code; serve_ms = $s.ms
        serve_refused = ($s.code -ne 0 -and $s.err -match 'serve: invalid configuration' -and $s.err -notmatch 'IPC server listening|Sandbox|Startup preload')
        serve_err = (Short $s.err); show_code = $show.code; show_values_printed = ($show.out -match 'GG_CORE_MAX_TOTAL_MEMORY='); validate_code = $val.code; validate_err = (Short $val.err) }
}
$show = Run-Cli @('config', 'show') @{ GG_CORE_MAX_TOTAL_MEMORY = '268435456' }
Rec 'show_total_only_256mib' @{ code = $show.code; exact = ($show.out -split "`r?`n" -contains 'GG_CORE_MAX_TOTAL_MEMORY=268435456'); per_call = (Short (($show.out -split "`r?`n") -match 'PER_CALL')) }

function Start-Daemon([hashtable]$vars, [string[]]$extra, [string]$tag) {
    $pipe = '\\.\pipe\gg182-' + $tag + '-' + [guid]::NewGuid().ToString('N')
    foreach ($k in $memVars) { Remove-Item "Env:$k" -ErrorAction SilentlyContinue }
    $env:CORE_AUTH_TOKEN = $token; $env:GG_CORE_SOCKET_PATH = $pipe
    foreach ($k in $vars.Keys) { Set-Item "Env:$k" $vars[$k] }
    $d = Start-Process -FilePath "$root\gg-core-cli.exe" -ArgumentList (@('serve') + $extra) -WorkingDirectory $root -RedirectStandardError "$Out\$tag.stderr.txt" -RedirectStandardOutput "$Out\$tag.stdout.txt" -NoNewWindow -PassThru
    foreach ($k in @('CORE_AUTH_TOKEN', 'GG_CORE_SOCKET_PATH') + @($vars.Keys)) { Remove-Item "Env:$k" -ErrorAction SilentlyContinue }
    $null = $d.Handle; $ready = $false
    for ($i = 0; $i -lt 480; $i++) { Start-Sleep -Milliseconds 250; if ((Get-Content "$Out\$tag.stderr.txt" -Raw -ErrorAction SilentlyContinue) -match 'IPC server listening') { $ready = $true; break }; if ($d.HasExited) { break } }
    @{ proc = $d; pipe = $pipe; ready = $ready }
}

# 2. Daemon with an explicit 256 MiB total and no per-call value (#182 reproduction).
$a = Start-Daemon @{ GG_CORE_MAX_TOTAL_MEMORY = '268435456' } @() 'a256'
$job = if ($a.ready) { [JobProbe]::Read($a.proc.Id) } else { 'not ready' }
Rec 'daemon_256mib_start' @{ ready = $a.ready; job = $job; job_limit_exact = ($job -like '268435456|*'); stderr = (Short ((Get-Content "$Out\a256.stderr.txt" -Raw) -replace $token, '<token>') 600) }
if ($a.ready) {
    $load = Run-Cli @('models', 'load', 'models/qwen/model.gguf', '--id', 'qwen') @{} $a.pipe
    Rec 'load_491mb_model_refused' @{ code = $load.code; refused = ($load.code -ne 0); limit_256mib = ("$($load.out) $($load.err)" -match 'limit 268435456'); out = (Short $load.out); err = (Short $load.err) }
    $live = Run-Cli @('live') @{} $a.pipe; $list = Run-Cli @('models', 'list') @{} $a.pipe; $st = Run-Cli @('status') @{} $a.pipe
    $a.proc.Refresh()
    Rec 'daemon_256mib_after_refusal' @{ alive = (-not $a.proc.HasExited); live_code = $live.code; list_code = $list.code; status_code = $st.code; list = (Short $list.out); job_after = [JobProbe]::Read($a.proc.Id) }
}
if (-not $a.proc.HasExited) { $a.proc.Kill() }

# 3. Control: explicit 2 GiB total with the model preloaded and a real generation.
$b = Start-Daemon @{ GG_CORE_MAX_TOTAL_MEMORY = '2147483648' } @('--model', 'models/qwen/model.gguf', '--model-id', 'qwen') 'b2g'
$job = if ($b.ready) { [JobProbe]::Read($b.proc.Id) } else { 'not ready' }
Rec 'daemon_2gib_start' @{ ready = $b.ready; job = $job; job_limit_exact = ($job -like '2147483648|*') }
if ($b.ready) {
    $r = Run-Cli @('infer', '--model', 'qwen', '--prompt', 'What is the capital of France? Answer in one word.', '--max-tokens', '16') @{} $b.pipe
    $rd = Run-Cli @('ready') @{} $b.pipe
    Rec 'daemon_2gib_infer' @{ code = $r.code; ms = $r.ms; out = (Short $r.out); ready_code = $rd.code }
}
if (-not $b.proc.HasExited) { $b.proc.Kill() }

# 4. Default (no memory variables): host-default total, 1 GiB fallback, unchanged behaviour.
$c = Start-Daemon @{} @() 'cdef'
$job = if ($c.ready) { [JobProbe]::Read($c.proc.Id) } else { 'not ready' }
$cs = Run-Cli @('config', 'show')
Rec 'daemon_default_start' @{ ready = $c.ready; job = $job; show_total = (Short (($cs.out -split "`r?`n") -match 'TOTAL')); show_per_call = (Short (($cs.out -split "`r?`n") -match 'PER_CALL')) }
if (-not $c.proc.HasExited) { $c.proc.Kill() }
Start-Sleep 1
Note 'done'; Set-Content "$Out\DONE" 'done'
