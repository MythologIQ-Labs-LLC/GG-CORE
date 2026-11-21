# Runs INSIDE the build VM. GG-CORE #184 packaged stop-tail and throughput comparison of
# exact daemons (C1 fixed-512, C2 time-paced, N7 deterministic successor) over the real
# authenticated named-pipe IPC, real Qwen2.5-0.5B Q4_K_M. For each daemon x context x threads:
# prefill-dominated throughput, cancel_request during prefill, deadline during prefill, and the
# post-stop CPU tail until the daemon process is quiescent; then a recovery request.
# Stop-stage revision (N7_STOP_STAGE_TRACE_PLAN.md): decode-phase and queued cancels, two workers,
# and GG_CORE_STOP_TRACE=1 for -TraceLabels so the daemon's stage events land in <tag>.err.
# IPC client functions are those of ggcore-qual-smoke.ps1. No prompt text is logged.
param(
    [string[]]$Daemons,              # label=path pairs, e.g. 'c2=C:\out\ggn7b1\bin\gg-core-cli.exe'
    [string]$Out = 'C:\out\g184stop',
    [int[]]$Threads = @(1, 2, 0),    # 0 = daemon auto (min(logical,16)/2 workers)
    [int[]]$Contexts = @(8192, 16384),
    [int]$Reps = 3,
    [string[]]$TraceLabels = @(),    # labels launched with GG_CORE_STOP_TRACE=1 (stage events on stderr)
    [string]$ModelPath = 'C:\Preview\models\qwen2.5-0.5b-instruct-q4_k_m.gguf',
    [string]$RuntimeDir = 'C:\host\vcdlls'   # VC runtime DLLs copied beside each daemon ('' = none)
)
$ErrorActionPreference = 'Stop'
# PowerShell -File accepts a comma-separated array as a single argument.
$Daemons = @($Daemons | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$TraceLabels = @($TraceLabels | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$Out = [IO.Path]::GetFullPath($Out)

# Refuse wrong or missing fixture BEFORE creating any measurements or staging.
# Logging a digest without comparing it to the approved model is not provenance.
$expectedModelBytes = 491400032
$expectedModelSha256 = '74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db'
if (Test-Path -LiteralPath $Out) { throw "Refusing existing evidence directory: $Out" }
if (-not (Test-Path -LiteralPath $ModelPath -PathType Leaf)) {
    throw 'Approved GGUF model file missing'
}
$modelFile = Get-Item -LiteralPath $ModelPath
if ($modelFile.Length -ne $expectedModelBytes) {
    throw "Unapproved GGUF model size: expected $expectedModelBytes bytes"
}
$modelHash = (Get-FileHash -LiteralPath $ModelPath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($modelHash -ne $expectedModelSha256) { throw 'Unapproved GGUF model SHA-256' }
if ($Reps -lt 1 -or $Threads.Count -eq 0 -or $Contexts.Count -eq 0) {
    throw 'Qualification matrix cannot be empty or have zero repetitions'
}
if ($Daemons.Count -eq 0) { throw 'No packaged daemons specified' }
$labels = @{}
foreach ($pair in $Daemons) {
    if ($pair -notmatch '^([a-zA-Z0-9_-]+)=(.+)$') {
        throw 'Invalid daemon label/path pair (expected safe-label=exe-path)'
    }
    if ($labels.ContainsKey($Matches[1])) { throw 'Duplicate daemon label' }
    if (-not (Test-Path -LiteralPath $Matches[2] -PathType Leaf)) {
        throw 'Missing packaged daemon executable'
    }
    $labels[$Matches[1]] = $true
}
foreach ($label in $TraceLabels) {
    if (-not $labels.ContainsKey($label)) { throw 'Trace label has no matching daemon' }
}
# Never recursively erase an arbitrary caller-supplied output directory.
# A new run owns a fresh tree, with binaries staged only beneath it.
New-Item -ItemType Directory -Path $Out | Out-Null
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
function Req([string]$prompt, [int]$max, [int]$timeoutMs = 0, [bool]$stream = $false) {
    $script:rid++; $p = [ordered]@{ max_tokens = $max; temperature = 0.0; top_p = 1.0; top_k = 1; stream = $stream }
    if ($timeoutMs -gt 0) { $p['timeout_ms'] = $timeoutMs }
    [ordered]@{ type = 'inference_request'; request_id = $script:rid; model_id = 'qwen'; prompt = $prompt; parameters = $p }
}
function Session { $c = New-Ipc; Send-Ipc $c ([ordered]@{ type = 'handshake'; token = $token; protocol_version = 'V1' }); $a = Recv-Ipc $c 15000; if ($a -is [string] -or $a.type -ne 'handshake_ack') { throw "auth failed" }; $c }
function Cpu { $script:d.Refresh(); $script:d.TotalProcessorTime.TotalMilliseconds }
# Wall and CPU time from $t0 (stop sent) until two consecutive 100 ms samples add < 15 ms CPU.
function Quiesce($t0, $cpu0) {
    # A CPU observation starting after reply/late-frame reads cannot know
    # the precise stop instant during that gap. Keep a conservative upper
    # bound from the original stop stopwatch, never manufacture a zero tail.
    $observedMs = $t0.ElapsedMilliseconds
    $observedUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $prev = Cpu; $calm = 0; $n = 0; $last = $observedMs
    while ($calm -lt 2 -and $n -lt 300) { Start-Sleep -Milliseconds 100; $now = Cpu; $n++; if (($now - $prev) -lt 15) { $calm++ } else { $calm = 0; $last = $t0.ElapsedMilliseconds }; $prev = $now }
    @{ tailWallMs = $last; tailBound = 'conservativeUpperBound'; cpuObservationStartMs = $observedMs; cpuObservationStartUnixMs = $observedUnix; tailCpuMs = [math]::Round((Cpu) - $cpu0); quiesced = ($calm -ge 2); samples = $n }
}
function Infer([string]$prompt, [int]$max, [int]$timeoutMs = 0) {
    $c = Session; try { $sw = [Diagnostics.Stopwatch]::StartNew(); Send-Ipc $c (Req $prompt $max $timeoutMs); $r = Recv-Ipc $c; @{ r = $r; ms = $sw.ElapsedMilliseconds } } finally { $c.Dispose() }
}
function Prefill-Cancel([int]$afterMs) {
    $c = Session
    try {
        $req = Req $long 64; Send-Ipc $c $req; Start-Sleep -Milliseconds $afterMs
        $t0 = [Diagnostics.Stopwatch]::StartNew(); $cpu0 = Cpu; $sentUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        Send-Ipc $c ([ordered]@{ type = 'cancel_request'; request_id = $req.request_id })
        $frames = @(); $code = $null; $tokens = $null
        while ($frames.Count -lt 2) { $f = Recv-Ipc $c 120000; if ($f -is [string]) { $frames += $f; break }; $frames += $f.type; if ($f.type -eq 'inference_response') { $code = $f.error_code; $tokens = $f.tokens_generated } }
        # Caller response latency must be observed before the CPU-stop sample.
        $terminalMs = $t0.ElapsedMilliseconds
        $q = Quiesce $t0 $cpu0
        $q + @{ kind = 'prefillCancel'; stopSentUnixMs = $sentUnix; tailClockOriginUnixMs = $sentUnix; afterMs = $afterMs; requestId = $req.request_id; terminalMs = $terminalMs; frames = ($frames -join ','); errorCode = $code; tokens = $tokens }
    } finally { $c.Dispose() }
}
# Cancel while generating: short prompt, long output, stop after prefill is done.
function Decode-Cancel([int]$afterMs) {
    $c = Session
    try {
        $req = Req 'Write a long, detailed story about a river and the towns along it.' 400; Send-Ipc $c $req; Start-Sleep -Milliseconds $afterMs
        $t0 = [Diagnostics.Stopwatch]::StartNew(); $cpu0 = Cpu; $sentUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        Send-Ipc $c ([ordered]@{ type = 'cancel_request'; request_id = $req.request_id })
        $frames = @(); $code = $null; $tokens = $null
        while ($frames.Count -lt 2) { $f = Recv-Ipc $c 120000; if ($f -is [string]) { $frames += $f; break }; $frames += $f.type; if ($f.type -eq 'inference_response') { $code = $f.error_code; $tokens = $f.tokens_generated } }
        # Caller response latency must be observed before the CPU-stop sample.
        $terminalMs = $t0.ElapsedMilliseconds
        $q = Quiesce $t0 $cpu0
        $q + @{ kind = 'decodeCancel'; stopSentUnixMs = $sentUnix; tailClockOriginUnixMs = $sentUnix; afterMs = $afterMs; requestId = $req.request_id; terminalMs = $terminalMs; frames = ($frames -join ','); errorCode = $code; tokens = $tokens }
    } finally { $c.Dispose() }
}
# Both workers busy with a mid-length prompt; a third, queued request is cancelled.
# The busy requests must still succeed; the queued one must end cancelled with no work.
function Queued-Cancel {
    # Busy requests use the long prompt so they are still prefilling when the queued one is
    # cancelled (review); the report checks the queued request never reached the backend.
    $mid = $long
    $busy = @(Session; Session); $q = Session
    try {
        $b = @(); foreach ($c in $busy) { $r = Req $mid 4; Send-Ipc $c $r; $b += $r }
        Start-Sleep -Milliseconds 300; $qr = Req $mid 4; Send-Ipc $q $qr; Start-Sleep -Milliseconds 300
        $t0 = [Diagnostics.Stopwatch]::StartNew()
        Send-Ipc $q ([ordered]@{ type = 'cancel_request'; request_id = $qr.request_id })
        $frames = @(); $code = $null
        while ($frames.Count -lt 2) { $f = Recv-Ipc $q 120000; if ($f -is [string]) { $frames += $f; break }; $frames += $f.type; if ($f.type -eq 'inference_response') { $code = $f.error_code } }
        $qMs = $t0.ElapsedMilliseconds; $busyOk = 0
        foreach ($c in $busy) { $f = Recv-Ipc $c 240000; if ($f -isnot [string] -and $f.type -eq 'inference_response' -and -not $f.error) { $busyOk++ } }
        @{ kind = 'queuedCancel'; requestId = $qr.request_id; terminalMs = $qMs; frames = ($frames -join ','); errorCode = $code; busyOk = $busyOk }
    } finally { foreach ($c in $busy + $q) { $c.Dispose() } }
}
function Prefill-Deadline([int]$timeoutMs) {
    $c = Session
    try {
        $sw = [Diagnostics.Stopwatch]::StartNew(); $req = Req $long 64 $timeoutMs; Send-Ipc $c $req
        $f = Recv-Ipc $c 120000; $answered = $sw.ElapsedMilliseconds
        # This CPU observation starts AFTER the terminal, not at deadline.
        $t0 = [Diagnostics.Stopwatch]::StartNew(); $cpu0 = Cpu; $observedUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        $q = Quiesce $t0 $cpu0
        $q + @{ kind = 'prefillDeadline'; stopSentUnixMs = $observedUnix; tailClockOriginUnixMs = $observedUnix; quiescenceOrigin = 'afterTerminal'; requestId = $req.request_id; timeoutMs = $timeoutMs; answeredAfterMs = $answered; errorCode = $f.error_code; tokens = $f.tokens_generated }
    } finally { $c.Dispose() }
}

# Real active-stream cancellation, no generated text logged. The first
# sanitized text frame proves decoding began before authenticated cancel.
function Stream-Cancel {
    $c = Session
    try {
        $req = Req 'Tell a long story about a river and its towns.' 400 0 $true
        $first = [Diagnostics.Stopwatch]::StartNew()
        Send-Ipc $c $req
        $started = $false; $firstMs = $null; $prefix = 0; $early = ''
        while ($prefix -lt 1000 -and $first.ElapsedMilliseconds -lt 60000) {
            $f = Recv-Ipc $c 15000
            if ($f -is [string]) { $early = $f; break }
            if ($f.type -ne 'stream_chunk' -or $f.request_id -ne $req.request_id -or $f.is_final -or $f.error) {
                $early = 'no active sanitized text stream'; break
            }
            $prefix++
            if ($f.text) { $started = $true; $firstMs = $first.ElapsedMilliseconds; break }
        }
        if (-not $started) {
            return @{ kind = 'streamCancel'; requestId = $req.request_id; started = $false
                prefixFrames = $prefix; firstTextMs = $firstMs; early = $early
                ack = $false; ackCount = 0; terminalCount = 0; terminalError = ''
                terminalMs = $null; ackMs = $null; lateFrames = 0
                dataAfterAck = 0; unexpectedFrames = 0; quiesced = $false }
        }
        $t0 = [Diagnostics.Stopwatch]::StartNew(); $cpu0 = Cpu
        $stopUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        Send-Ipc $c ([ordered]@{ type = 'cancel_request'; request_id = $req.request_id })
        $ack = $false; $ackCount = 0; $ackMs = $null
        $ends = 0; $terminalMs = $null; $err = ''; $terminalBeforeAck = $false
        $afterAck = 0; $afterFinal = 0; $unexpected = 0; $readOutcome = 'OK'
        $frames = @()
        for ($i = 0; $i -lt 100; $i++) {
            $f = Recv-Ipc $c 15000
            if ($f -is [string]) { $readOutcome = $f; break }
            if ($f.request_id -ne $req.request_id) { $unexpected++; break }
            $frames += $f.type
            if ($f.type -eq 'cancel_response') {
                $ackCount++
                $ack = ($ackCount -eq 1 -and $f.cancelled -eq $true)
                if ($null -eq $ackMs) { $ackMs = $t0.ElapsedMilliseconds }
            } elseif ($f.type -eq 'stream_chunk') {
                if ($f.is_final) {
                    $ends++
                    if ($null -eq $terminalMs) { $terminalMs = $t0.ElapsedMilliseconds }
                    $err = [string]$f.error
                    if (-not $ack) { $terminalBeforeAck = $true }
                } else {
                    if ($ack) { $afterAck++ }
                    if ($ends -gt 0) { $afterFinal++ }
                }
            } else { $unexpected++ }
            if ($ackCount -gt 1 -or $ends -gt 1 -or ($ack -and $ends -eq 1)) { break }
        }
        $late = 0; $post = 'SKIPPED'
        if ($ack -and $ends -eq 1) {
            $extra = Recv-Ipc $c 350
            if ($extra -is [string]) { $post = $extra }
            else { $late++; $post = [string]$extra.type }
        }
        $q = Quiesce $t0 $cpu0
        $q + @{ kind = 'streamCancel'; stopSentUnixMs = $stopUnix
            tailClockOriginUnixMs = $stopUnix; requestId = $req.request_id
            started = $started; firstTextMs = $firstMs; prefixFrames = $prefix
            ack = $ack; ackCount = $ackCount; ackMs = $ackMs
            terminalCount = $ends; terminalMs = $terminalMs
            terminalBeforeAck = $terminalBeforeAck; terminalError = $err
            dataAfterAck = $afterAck; dataAfterFinal = $afterFinal; lateFrames = $late; unexpectedFrames = $unexpected
            readOutcome = $readOutcome; postTerminalRead = $post; frames = ($frames -join ',') }
    } finally { $c.Dispose() }
}

# Stage every daemon first, then run configuration-outer / daemon-inner so the daemons
# (notably trace off vs on of one binary) interleave per configuration (audit F7).
$staged = @()
foreach ($pair in $Daemons) {
    $label, $exe = $pair -split '=', 2
    $root = Join-Path $Out "stage-$label"
    New-Item -ItemType Directory -Path "$root\models\qwen" -Force | Out-Null
    Copy-Item -LiteralPath $exe -Destination "$root\gg-core-cli.exe"
    if ($RuntimeDir) { Copy-Item "$RuntimeDir\*.dll" $root }
    Copy-Item -LiteralPath $ModelPath -Destination "$root\models\qwen\model.gguf"
    $stagedModel = "$root\models\qwen\model.gguf"
    $stagedSize = (Get-Item -LiteralPath $stagedModel).Length
    $sha = (Get-FileHash -LiteralPath $stagedModel -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($stagedSize -ne $expectedModelBytes -or $sha -ne $expectedModelSha256) {
        throw 'Staged GGUF model identity changed during copy'
    }
    @{ model_id = 'qwen'; name = 'Qwen2.5-0.5B-Instruct Q4_K_M'; version = '9217f5db'; capabilities = @('text_generation'); sha256 = $sha; size_bytes = (Get-Item "$root\models\qwen\model.gguf").Length; architecture = 'gguf'; license = 'Apache-2.0' } | ConvertTo-Json | Set-Content "$root\models\qwen\manifest.json" -Encoding ascii
    Rec @{ kind = 'identity'; label = $label; exe = $exe; exe_sha256 = (Get-FileHash "$root\gg-core-cli.exe").Hash.ToLower(); model_sha256 = $sha }
    $staged += , @($label, $root)
}
foreach ($ctx in $Contexts) { foreach ($t in $Threads) { foreach ($sd in $staged) {
    $label, $root = $sd
    $tag = "$label-ctx$ctx-t$t"; $script:pipeName = "gg184-$tag-" + [guid]::NewGuid().ToString('N')
    $env:CORE_AUTH_TOKEN = $token; $env:GG_CORE_SOCKET_PATH = "\\.\pipe\$($script:pipeName)"
    $env:GG_CORE_N_CTX = "$ctx"; $env:GG_CORE_MAX_CONTEXT = "$ctx"; $env:GG_CORE_MAX_CONTEXT_TOKENS = "$ctx"; $env:GG_CORE_N_THREADS = "$t"; $env:GG_CORE_MAX_CONCURRENT = '2'
    if ($TraceLabels -contains $label) { $env:GG_CORE_STOP_TRACE = '1' }
    $script:d = Start-Process -FilePath "$root\gg-core-cli.exe" -ArgumentList 'serve', '--model', 'models/qwen/model.gguf', '--model-id', 'qwen' -WorkingDirectory $root -RedirectStandardError "$Out\$tag.err" -RedirectStandardOutput "$Out\$tag.out" -NoNewWindow -PassThru
    foreach ($k in 'CORE_AUTH_TOKEN', 'GG_CORE_SOCKET_PATH', 'GG_CORE_N_CTX', 'GG_CORE_MAX_CONTEXT', 'GG_CORE_MAX_CONTEXT_TOKENS', 'GG_CORE_N_THREADS', 'GG_CORE_MAX_CONCURRENT', 'GG_CORE_STOP_TRACE') { Remove-Item "Env:$k" -ErrorAction SilentlyContinue }
    $null = $script:d.Handle; $ready = $false
    for ($i = 0; $i -lt 480; $i++) { Start-Sleep -Milliseconds 250; if ((Get-Content "$Out\$tag.err" -Raw -ErrorAction SilentlyContinue) -match 'IPC server listening') { $ready = $true; break }; if ($script:d.HasExited) { break } }
    Rec @{ kind = 'daemon'; tag = $tag; ready = $ready }
    if (-not $ready) { if (-not $script:d.HasExited) { $script:d.Kill() }; throw "Native daemon not ready for $tag" }
    for ($r = 0; $r -lt $Reps; $r++) {
        $tp = Infer $long 24; $outHash = if ($tp.r.output) { ([BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([Text.Encoding]::UTF8.GetBytes([string]$tp.r.output)))).Replace('-', '').Substring(0, 16) } else { $null }
        Rec @{ kind = 'throughput'; tag = $tag; rep = $r; ms = $tp.ms; tokens = $tp.r.tokens_generated; ok = (-not $tp.r.error); outputSha = $outHash }
        Rec ((Prefill-Cancel (1500 + 500 * $r)) + @{ tag = $tag; rep = $r })
        Rec ((Prefill-Deadline (2000 + 500 * $r)) + @{ tag = $tag; rep = $r })
        Rec ((Decode-Cancel (2500 + 500 * $r)) + @{ tag = $tag; rep = $r })
        Rec ((Stream-Cancel) + @{ tag = $tag; rep = $r })
        Rec ((Queued-Cancel) + @{ tag = $tag; rep = $r })
        $rc = Infer 'Reply with one word: ok' 4; Rec @{ kind = 'recover'; tag = $tag; rep = $r; ok = (-not $rc.r.error); ms = $rc.ms; alive = (-not $script:d.HasExited) }
    }
    if (-not $script:d.HasExited) { $script:d.Kill() }; Start-Sleep 1
} } }
Note 'done'; Set-Content "$Out\DONE" 'done'
