# Smoke test for the gamepad path in w3stream-helper.
# Invoked by scripts/poke-gamepad.bat.

$ErrorActionPreference = 'Stop'

# PS 5.1 + .NET Framework: Process.StandardInput's underlying StreamWriter sets
# AutoFlush=true at construction, which triggers an internal Flush(). That
# Flush writes the encoding preamble (UTF-8 BOM, EF BB BF) to the child's
# stdin BEFORE any of our bytes. Force-clear the preamble by giving the
# console a no-BOM UTF-8 encoding before we spawn - Process.StandardInput
# reads Console.InputEncoding when it constructs the StreamWriter.
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false

$helper = Join-Path $env:LOCALAPPDATA 'w3stream\helper.exe'
if (-not (Test-Path $helper)) {
    Write-Host "[!] helper.exe not found at $helper. Install w3stream-helper first."
    exit 2
}

Write-Host "[+] Spawning $helper"
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $helper
$psi.UseShellExecute = $false
$psi.RedirectStandardInput  = $true
$psi.RedirectStandardOutput = $true
$psi.CreateNoWindow = $true

$proc = [System.Diagnostics.Process]::Start($psi)
$in  = $proc.StandardInput.BaseStream
$out = $proc.StandardOutput.BaseStream

function Send-Msg([hashtable]$msg) {
    $json = $msg | ConvertTo-Json -Compress -Depth 8
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
    $len = [System.BitConverter]::GetBytes([UInt32]$bytes.Length)
    $in.Write($len, 0, 4)
    $in.Write($bytes, 0, $bytes.Length)
    $in.Flush()
}

function Recv-Msg() {
    $lenBuf = New-Object byte[] 4
    $read = 0
    while ($read -lt 4) {
        $n = $out.Read($lenBuf, $read, 4 - $read)
        if ($n -le 0) { throw "helper closed stdout" }
        $read += $n
    }
    $len = [System.BitConverter]::ToUInt32($lenBuf, 0)
    $bodyBuf = New-Object byte[] $len
    $read = 0
    while ($read -lt $len) {
        $n = $out.Read($bodyBuf, $read, $len - $read)
        if ($n -le 0) { throw "helper closed stdout mid-body" }
        $read += $n
    }
    return [System.Text.Encoding]::UTF8.GetString($bodyBuf) | ConvertFrom-Json
}

# ---- 1. Read the hello frame ----
$hello = Recv-Msg
Write-Host "[+] hello: version=$($hello.version) gamepad.available=$($hello.gamepad.available)"
Write-Host "       vigem_status=$($hello.gamepad.vigem_status)"
Write-Host "       hidhide_status=$($hello.gamepad.hidhide_status)"
if (-not $hello.gamepad.available) {
    Write-Host "[!] gamepad.available=false; aborting. Check that ViGEmBus is installed and you rebooted after install."
    $in.Close()
    $proc.WaitForExit(2000) | Out-Null
    exit 3
}

# ---- 2. Health check ----
Send-Msg @{ requestId = 'health-1'; command = 'health' }
$resp = Recv-Msg
Write-Host "[+] health: $(($resp.result | ConvertTo-Json -Compress -Depth 5))"

# ---- 3. Enable + fire fortnite_emote_1 ----
Send-Msg @{ requestId = 'en-1'; command = 'enabled'; params = @{ enabled = $true } }
$resp = Recv-Msg
Write-Host "[+] enabled: $(($resp.result | ConvertTo-Json -Compress -Depth 3))"

Write-Host ""
Write-Host "[+] Firing fortnite_emote_1. During the suspend window (~3s)"
Write-Host "    push the physical stick - gamepad-tester should stay neutral."
Write-Host ""

Send-Msg @{
    requestId = 'trig-1'
    command = 'trigger'
    params = @{
        action_id = 'fortnite_emote_1'
        request_id = [guid]::NewGuid().ToString()
    }
}
$resp = Recv-Msg
Write-Host "[+] trigger response: $(($resp | ConvertTo-Json -Compress -Depth 5))"

if ($resp.error) {
    Write-Host "[!] trigger failed: $($resp.error)"
    if ($resp.error -match 'disabled in library') {
        Write-Host "    -> Flip fortnite_emote_1 to enabled=true in"
        Write-Host "       $env:LOCALAPPDATA\w3stream\actions.json and rerun."
    }
}

# ---- 4. Panic to disable, then exit ----
Send-Msg @{ requestId = 'panic-1'; command = 'panic' }
Recv-Msg | Out-Null

$in.Close()
$proc.WaitForExit(3000) | Out-Null
Write-Host "[+] Done. Helper exit code: $($proc.ExitCode)"
