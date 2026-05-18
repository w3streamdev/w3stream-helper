# Smoke test for the legacy keystroke path in w3stream-helper.
# Invoked by scripts/poke-helper.bat.
#
# -NoTrigger: run the Native Messaging round-trip but skip firing
#             test_type_hi (the keystroke-typing step). Use this in
#             CI/headless contexts where SendInput would land in
#             whatever happens to be focused.

param([switch]$NoTrigger)

$ErrorActionPreference = 'Stop'

# PS 5.1 + .NET Framework: Process.StandardInput's underlying StreamWriter sets
# AutoFlush=true at construction, which triggers an internal Flush(). That
# Flush writes the encoding preamble (UTF-8 BOM, EF BB BF) to the child's
# stdin BEFORE any of our bytes. Force-clear the preamble by giving the
# console a no-BOM UTF-8 encoding before we spawn — Process.StandardInput
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

# ---- 2. Health check ----
Send-Msg @{ requestId = 'health-1'; command = 'health' }
$resp = Recv-Msg
Write-Host "[+] health: $(($resp.result | ConvertTo-Json -Compress -Depth 5))"

# ---- 3. Actions list ----
Send-Msg @{ requestId = 'list-1'; command = 'actions.list' }
$resp = Recv-Msg
Write-Host "[+] actions: $(($resp.result.actions | ForEach-Object { $_.action_id }) -join ', ')"

# ---- 4. Enable + fire test_type_hi ----
Send-Msg @{ requestId = 'en-1'; command = 'enabled'; params = @{ enabled = $true } }
$resp = Recv-Msg
Write-Host "[+] enabled: $(($resp.result | ConvertTo-Json -Compress -Depth 3))"

if ($NoTrigger) {
    Write-Host "[+] -NoTrigger: skipping test_type_hi keystroke step."
} else {
    Write-Host ""
    Write-Host "[+] In 3 seconds we will fire test_type_hi (sends 'HI' via SendInput)."
    Write-Host "    Focus a Notepad/text window NOW if you want to see the typing."
    for ($i = 3; $i -ge 1; $i--) {
        Write-Host "    $i..."
        Start-Sleep -Seconds 1
    }

    Send-Msg @{
        requestId = 'trig-1'
        command = 'trigger'
        params = @{
            action_id = 'test_type_hi'
            request_id = [guid]::NewGuid().ToString()
        }
    }
    $resp = Recv-Msg
    Write-Host "[+] trigger response: $(($resp | ConvertTo-Json -Compress -Depth 5))"

    if ($resp.error) {
        Write-Host "[!] trigger failed: $($resp.error)"
    }
}

# ---- 5. Panic + exit ----
Send-Msg @{ requestId = 'panic-1'; command = 'panic' }
$resp = Recv-Msg
Write-Host "[+] panic: $(($resp.result | ConvertTo-Json -Compress -Depth 3))"

$in.Close()
$proc.WaitForExit(3000) | Out-Null
Write-Host "[+] Done. Helper exit code: $($proc.ExitCode)"
