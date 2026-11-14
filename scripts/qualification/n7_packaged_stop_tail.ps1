# Runs INSIDE the build VM. GG-CORE #184 packaged stop-tail and throughput comparison of
# exact daemons (C1 fixed-512, C2 time-paced, N7 deterministic successor) over the real
# authenticated named-pipe IPC, real Qwen2.5-0.5B Q4_K_M. For each daemon x context x threads:
# prefill-dominated throughput, cancel_request during prefill, deadline during prefill, and the
# post-stop CPU tail until the daemon process is quiescent; then a recovery request.
# IPC client functions are those of ggcore-qual-smoke.ps1. No prompt text is logged.
param(
    [string[]]$Daemons,              # label=path pairs, e.g. 'c2=C:\out\ggn7b1\bin\gg-core-cli.exe'
    [string]$Out = 'C:\out\g184stop',
    [int[]]$Threads = @(1, 2, 0),    # 0 = daemon auto (min(logical,16)/2 workers)
    [int[]]$Contexts = @(8192, 16384),
    [int]$Reps = 3
)
$ErrorActionPreference = 'Continue'
# powershell -File passes a comma list as one string; accept both forms.
$Daemons = @($Daemons | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Out | Out-Null
$log = "$Out\stop.log"; $jsonl = "$Out\results.jsonl"
function Note($m) { "$(Get-Date -Format o) $m" | Tee-Object -FilePath $log -Append | Out-Null }
function Rec($h) { ($h | ConvertTo-Json -Compress -Depth 5) | Add-Content $jsonl; Note ('RESULT ' + ($h | ConvertTo-Json -Compress -Depth 5)) }

$bytes = New-Object byte[] 32; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
$token = [Convert]::ToBase64String($bytes)
$fox = 'The brown fox walks beside a clear river under a quiet blue sky. '
$long = 'Reply with the word READY after reading this note: ' + ($fox * 179)

function New-Ipc { $c = New-Object IO.Pipes.NamedPipeClientStream('.', $script:pipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous); $c.Connect(10000); $c }
function Send-Ipc($c, $obj) { $data = [Text.Encoding]::UTF8.GetBytes(($obj | ConvertTo-Json -Compress -Depth 6)); $c.Write([BitConverter]::GetBytes([uint32]$data.Length), 0, 4); $c.Write($data, 0, $data.Length); $c.Flush() }
function Read-Exact($c, [int]$n, [int]$ms) {
    $buf = New-Object byte[] $n; $off = 0; $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($off -lt $n) { $left = $ms - [int]$sw.ElapsedMilliseconds; if ($left -le 0) { return $null }; $t = $c.ReadAsync($buf, $off, $n - $off); if (-not $t.Wait($left)) { return $null }; if ($t.Result -le 0) { return 'EOF' }; $off += $t.Result }
    , $buf
}
function Recv-Ipc($c, [int]$ms = 240000) {
    $h = Read-Exact $c 4 $ms; if ($null -eq $h) { return 'TIMEOUT' }; if ($h -is [string]) { return $h }
    $b = Read-Exact $c ([int][BitConverter]::ToUInt32($h, 0)) $ms; if ($null -eq $b) { return 'TIMEOUT' }; if ($b -is [string]) { return $b }
    [Text.Encoding]::UTF8.GetString($b) | ConvertFrom-Json
}
$script:rid = 1000
function Req([string]$prompt, [int]$max, [int]$timeoutMs = 0) {
    $script:rid++; $p = [ordered]@{ max_tokens = $max; temperature = 0.0; top_p = 1.0; top_k = 1; stream = $false }
    if ($timeoutMs -gt 0) { $p['timeout_ms'] = $timeoutMs }
    [ordered]@{ type = 'inference_request'; request_id = $script:rid; model_id = 'qwen'; prompt = $prompt; parameters = $p }
}
function Session { $c = New-Ipc; Send-Ipc $c ([ordered]@{ type = 'handshake'; token = $token; protocol_version = 'V1' }); $a = Recv-Ipc $c 15000; if ($a -is [string] -or $a.type -ne 'handshake_ack') { throw "auth failed" }; $c }
function Cpu { $script:d.Refresh(); $script:d.TotalProcessorTime.TotalMilliseconds }
# Wall and CPU time from $t0 (stop sent) until two consecutive 100 ms samples add < 15 ms CPU.
function Quiesce($t0, $cpu0) {
    $prev = Cpu; $calm = 0; $n = 0
    while ($calm -lt 2 -and $n -lt 300) { Start-Sleep -Milliseconds 100; $now = Cpu; $n++; if (($now - $prev) -lt 15) { $calm++ } else { $calm = 0; $last = $t0.ElapsedMilliseconds }; $prev = $now }
    if ($null -eq $last) { $last = 0 }
    @{ tailWallMs = $last; tailCpuMs = [math]::Round((Cpu) - $cpu0); quiesced = ($calm -ge 2); samples = $n }
}
function Infer([string]$prompt, [int]$max, [int]$timeoutMs = 0) {
    $c = Session; try { $sw = [Diagnostics.Stopwatch]::StartNew(); Send-Ipc $c (Req $prompt $max $timeoutMs); $r = Recv-Ipc $c; @{ r = $r; ms = $sw.ElapsedMilliseconds } } finally { $c.Dispose() }
}
function Prefill-Cancel([int]$afterMs) {
    $c = Session
    try {
        $req = Req $long 64; Send-Ipc $c $req; Start-Sleep -Milliseconds $afterMs
        $t0 = [Diagnostics.Stopwatch]::StartNew(); $cpu0 = Cpu
        Send-Ipc $c ([ordered]@{ type = 'cancel_request'; request_id = $req.request_id })
        $frames = @(); $code = $null; $tokens = $null
        while ($frames.Count -lt 2) { $f = Recv-Ipc $c 120000; if ($f -is [string]) { $frames += $f; break }; $frames += $f.type; if ($f.type -eq 'inference_response') { $code = $f.error_code; $tokens = $f.tokens_generated } }
        $q = Quiesce $t0 $cpu0
        $q + @{ kind = 'prefillCancel'; afterMs = $afterMs; terminalMs = $t0.ElapsedMilliseconds; frames = ($frames -join ','); errorCode = $code; tokens = $tokens }
    } finally { $c.Dispose() }
}
function Prefill-Deadline([int]$timeoutMs) {
    $c = Session
    try {
        $sw = [Diagnostics.Stopwatch]::StartNew(); Send-Ipc $c (Req $long 64 $timeoutMs)
        $f = Recv-Ipc $c 120000; $answered = $sw.ElapsedMilliseconds
        $t0 = [Diagnostics.Stopwatch]::StartNew(); $cpu0 = Cpu
        $q = Quiesce $t0 $cpu0
        $q + @{ kind = 'prefillDeadline'; timeoutMs = $timeoutMs; answeredAfterMs = $answered; errorCode = $f.error_code; tokens = $f.tokens_generated }
    } finally { $c.Dispose() }
}

foreach ($pair in $Daemons) {
    $label, $exe = $pair -split '=', 2
    $root = "C:\g184stop-$label"; Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force "$root\models\qwen" | Out-Null
    if (-not (Test-Path $exe)) { Rec @{ kind = 'identity'; label = $label; error = "missing $exe" }; continue }
    Copy-Item $exe "$root\gg-core-cli.exe" -Force; Copy-Item C:\host\vcdlls\*.dll $root -Force
    Copy-Item C:\Preview\models\qwen2.5-0.5b-instruct-q4_k_m.gguf "$root\models\qwen\model.gguf" -Force
    $sha = (Get-FileHash "$root\models\qwen\model.gguf").Hash.ToLower()
    @{ model_id = 'qwen'; name = 'Qwen2.5-0.5B-Instruct Q4_K_M'; version = '9217f5db'; capabilities = @('text_generation'); sha256 = $sha; size_bytes = (Get-Item "$root\models\qwen\model.gguf").Length; architecture = 'gguf'; license = 'Apache-2.0' } | ConvertTo-Json | Set-Content "$root\models\qwen\manifest.json" -Encoding ascii
    Rec @{ kind = 'identity'; label = $label; exe = $exe; exe_sha256 = (Get-FileHash "$root\gg-core-cli.exe").Hash.ToLower(); model_sha256 = $sha }
    foreach ($ctx in $Contexts) { foreach ($t in $Threads) {
        $tag = "$label-ctx$ctx-t$t"; $script:pipeName = "gg184-$tag-" + [guid]::NewGuid().ToString('N')
        $env:CORE_AUTH_TOKEN = $token; $env:GG_CORE_SOCKET_PATH = "\\.\pipe\$($script:pipeName)"
        $env:GG_CORE_N_CTX = "$ctx"; $env:GG_CORE_MAX_CONTEXT = "$ctx"; $env:GG_CORE_MAX_CONTEXT_TOKENS = "$ctx"; $env:GG_CORE_N_THREADS = "$t"
        $script:d = Start-Process -FilePath "$root\gg-core-cli.exe" -ArgumentList 'serve', '--model', 'models/qwen/model.gguf', '--model-id', 'qwen' -WorkingDirectory $root -RedirectStandardError "$Out\$tag.err" -RedirectStandardOutput "$Out\$tag.out" -NoNewWindow -PassThru
        foreach ($k in 'CORE_AUTH_TOKEN', 'GG_CORE_SOCKET_PATH', 'GG_CORE_N_CTX', 'GG_CORE_MAX_CONTEXT', 'GG_CORE_MAX_CONTEXT_TOKENS', 'GG_CORE_N_THREADS') { Remove-Item "Env:$k" -ErrorAction SilentlyContinue }
        $null = $script:d.Handle; $ready = $false
        for ($i = 0; $i -lt 480; $i++) { Start-Sleep -Milliseconds 250; if ((Get-Content "$Out\$tag.err" -Raw -ErrorAction SilentlyContinue) -match 'IPC server listening') { $ready = $true; break }; if ($script:d.HasExited) { break } }
        Rec @{ kind = 'daemon'; tag = $tag; ready = $ready }
        if (-not $ready) { continue }
        for ($r = 0; $r -lt $Reps; $r++) {
            $tp = Infer $long 24; $outHash = if ($tp.r.output) { ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes([string]$tp.r.output)))).Replace('-', '').Substring(0, 16) } else { $null }
            Rec @{ kind = 'throughput'; tag = $tag; rep = $r; ms = $tp.ms; tokens = $tp.r.tokens_generated; ok = (-not $tp.r.error); outputSha = $outHash }
            Rec ((Prefill-Cancel (1500 + 500 * $r)) + @{ tag = $tag; rep = $r })
            Rec ((Prefill-Deadline (2000 + 500 * $r)) + @{ tag = $tag; rep = $r })
            $rc = Infer 'Reply with one word: ok' 4; Rec @{ kind = 'recover'; tag = $tag; rep = $r; ok = (-not $rc.r.error); ms = $rc.ms; alive = (-not $script:d.HasExited) }
        }
        if (-not $script:d.HasExited) { $script:d.Kill() }; Start-Sleep 1
    } }
}
Note 'done'; Set-Content "$Out\DONE" 'done'
