# Runs INSIDE the build VM. #183 remediation packaged CLI exit-code check (owner disposition
# 2026-11-13). Readiness = the daemon's named pipe exists (its 'listening' log line does not
# reach the redirected stderr file reliably). `models unload ID --wait-ms N` exits 4 while an instance is held, 0 once released;
# plain `models unload` unchanged; wrong token refused. Real Qwen2.5-0.5B, authenticated named pipe.
param([string]$Exe = 'E:\w\target-ggv1-182\release\gg-core-cli.exe', [string]$Out = 'C:\out\r183cli')
$ErrorActionPreference = 'Continue'
Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force $Out | Out-Null
$jsonl = "$Out\results.jsonl"
function Rec($h) { ($h | ConvertTo-Json -Compress -Depth 5) | Add-Content $jsonl }
$bytes = New-Object byte[] 32; [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
$token = [Convert]::ToBase64String($bytes)
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

function Session { $c = New-Ipc; Send-Ipc $c ([ordered]@{ type = 'handshake'; token = $token; protocol_version = 'V1' }); $a = Recv-Ipc $c 15000; if ($a -is [string] -or $a.type -ne 'handshake_ack') { throw "auth failed" }; $c }
function Invoke-GgCli([string[]]$a, [string]$tok = $token) {
    $env:CORE_AUTH_TOKEN = $tok
    $p = Start-Process -FilePath "$root\gg-core-cli.exe" -ArgumentList $a -WorkingDirectory $root -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$Out\cli.out" -RedirectStandardError "$Out\cli.err"
    $env:CORE_AUTH_TOKEN = $token
    $so = [string](Get-Content "$Out\cli.out" -Raw); $se = [string](Get-Content "$Out\cli.err" -Raw)
    @{ args = ($a -join ' '); exit = $p.ExitCode; stdout = ($so -replace '\s+', ' ').Trim(); stderr = ($se -replace '\s+', ' ').Trim() }
}
$root = 'C:\r183cli'; Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$root\models\qwen" | Out-Null
Copy-Item $Exe "$root\gg-core-cli.exe" -Force; Copy-Item C:\host\vcdlls\*.dll $root -Force
Copy-Item C:\Preview\models\qwen2.5-0.5b-instruct-q4_k_m.gguf "$root\models\qwen\model.gguf" -Force
$sha = (Get-FileHash "$root\models\qwen\model.gguf").Hash.ToLower()
@{ model_id = 'qwen'; name = 'Qwen2.5-0.5B-Instruct Q4_K_M'; version = '9217f5db'; capabilities = @('text_generation'); sha256 = $sha; size_bytes = (Get-Item "$root\models\qwen\model.gguf").Length; architecture = 'gguf'; license = 'Apache-2.0' } | ConvertTo-Json | Set-Content "$root\models\qwen\manifest.json" -Encoding ascii
Rec @{ kind = 'identity'; exe_sha256 = (Get-FileHash "$root\gg-core-cli.exe").Hash.ToLower(); model_sha256 = $sha }
$script:pipeName = 'gg183-' + [guid]::NewGuid().ToString('N')
$env:CORE_AUTH_TOKEN = $token; $env:GG_CORE_SOCKET_PATH = "\\.\pipe\$($script:pipeName)"; $env:GG_CORE_STREAM_STALL_MS = '120000'; $env:GG_CORE_N_CTX = '4096'; $env:GG_CORE_MAX_CONTEXT = '4096'; $env:GG_CORE_MAX_CONTEXT_TOKENS = '4096'
Push-Location $root
$d = Start-Process -FilePath "$root\gg-core-cli.exe" -ArgumentList 'serve', '--model', 'models/qwen/model.gguf', '--model-id', 'qwen' -WorkingDirectory $root -RedirectStandardError "$Out\daemon.err" -RedirectStandardOutput "$Out\daemon.out" -NoNewWindow -PassThru
$ready = $false; for ($i = 0; $i -lt 480; $i++) { Start-Sleep -Milliseconds 250; if (@([System.IO.Directory]::GetFiles('\\.\pipe\') | Where-Object { $_ -like "*$($script:pipeName)*" }).Count -gt 0) { Start-Sleep -Seconds 1; $ready = $true; break } }
$gg = @([System.IO.Directory]::GetFiles('\\.\pipe\') | Where-Object { $_ -like '*gg183*' }) -join ';'; Rec @{ kind = 'daemon'; pipes = $gg; want = $script:pipeName; sock = $env:GG_CORE_SOCKET_PATH; errTail = ((Get-Content "$Out\daemon.err" -Tail 2) -join ' / '); ready = $ready; exited = $d.HasExited; exitCode = $(if ($d.HasExited) { $d.ExitCode } else { $null }) }
if ($ready) {
    # 1. Hold the model: a stream whose reader stops after its first frame.
    $hold = Session
    $p = [ordered]@{ max_tokens = 1500; temperature = 0.0; top_p = 1.0; top_k = 1; stream = $true }
    Send-Ipc $hold ([ordered]@{ type = 'inference_request'; request_id = 7; model_id = 'qwen'; prompt = ((1..40) -join ', ') + ','; parameters = $p })
    $first = Recv-Ipc $hold 60000; Rec @{ kind = 'hold'; firstFrame = $first.type }
    Start-Sleep -Seconds 2
    Rec (@{ kind = 'unload_wait_pending'; expect = 4 } + (Invoke-GgCli @('models', 'unload', 'qwen', '--wait-ms', '2000')))
    Rec (@{ kind = 'list_during_release' } + (Invoke-GgCli @('models', 'list', '--json')))
    # 2. Release: disconnect the holder (teardown cancels its stream).
    # Resume reading: count the held stream's frames and its terminal state.
    $frames = 0; $final = $null; $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt 180000) { $f2 = Recv-Ipc $hold 60000; if ($f2 -is [string]) { $final = $f2; break }; $frames++; if ($f2.is_final) { $final = "final error=$($f2.error)"; break } }
    Rec @{ kind = 'hold_drained'; frames = $frames; final = $final }
    Rec (@{ kind = 'unload_wait_after_drain'; expect = 'released, exit 1 (already unloaded)' } + (Invoke-GgCli @('models', 'unload', 'qwen', '--wait-ms', '5000')))
    $hold.Dispose(); Start-Sleep -Seconds 3
    Rec (@{ kind = 'wrong_token_load'; expect = 1 } + (Invoke-GgCli @('models', 'load', 'models/qwen/model.gguf', '--id', 'qwen') 'not-the-token'))
    Rec (@{ kind = 'load'; expect = 0 } + (Invoke-GgCli @('models', 'load', 'models/qwen/model.gguf', '--id', 'qwen')))
    Rec (@{ kind = 'plain_unload'; expect = 0 } + (Invoke-GgCli @('models', 'unload', 'qwen')))
    Rec (@{ kind = 'load_again'; expect = 0 } + (Invoke-GgCli @('models', 'load', 'models/qwen/model.gguf', '--id', 'qwen')))
    Rec (@{ kind = 'unload_wait_released'; expect = 0 } + (Invoke-GgCli @('models', 'unload', 'qwen', '--wait-ms', '30000')))
    Rec @{ kind = 'alive'; alive = (-not $d.HasExited) }
}
if (-not $d.HasExited) { $d.Kill() }
Pop-Location
Set-Content "$Out\DONE" 'done'
