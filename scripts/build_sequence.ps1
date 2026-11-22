param(
    [ValidateSet('all','check','test','live','smoke','build','release')]
    [string]$Phase = 'all',
    [switch]$SkipLive,
    [switch]$SkipSmoke,
    [switch]$List
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$manifest = Join-Path $repoRoot 'Cargo.toml'
$secretPath = Join-Path $env:USERPROFILE '.config\openchamber\secrets\659cea82ea5e5605b1553d1e214fc5a8'
$results = New-Object System.Collections.Generic.List[object]

function Write-Phase([string]$Name, [string]$Status, [string]$Detail = '') {
    $row = [pscustomobject]@{ Phase = $Name; Status = $Status; Detail = $Detail }
    $results.Add($row) | Out-Null
    $color = switch ($Status) { 'OK' { 'Green' } 'SKIP' { 'Yellow' } 'FAIL' { 'Red' } default { 'Cyan' } }
    $msg = if ($Detail) { "[$Status] $Name - $Detail" } else { "[$Status] $Name" }
    Write-Host $msg -ForegroundColor $color
}

function Invoke-Step([string]$Name, [scriptblock]$Action) {
    Write-Host "`n=== $Name ===" -ForegroundColor Cyan
    try {
        & $Action
        if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) {
            throw "exit code $LASTEXITCODE"
        }
        Write-Phase $Name 'OK'
        return $true
    } catch {
        Write-Phase $Name 'FAIL' $_.Exception.Message
        return $false
    }
}

function Ensure-DbUrl {
    if (-not [string]::IsNullOrWhiteSpace($env:ODCLINIC_DB_URL)) { return $true }
    if (-not [string]::IsNullOrWhiteSpace($env:MYSQL_ROOT_PASSWORD)) {
        $env:ODCLINIC_DB_URL = 'mysql://root:' + [uri]::EscapeDataString($env:MYSQL_ROOT_PASSWORD) + '@localhost:3306/demo'
        return $true
    }
    if (Test-Path -LiteralPath $secretPath) {
        $pw = (Get-Content -LiteralPath $secretPath -Raw).Trim()
        if (-not [string]::IsNullOrWhiteSpace($pw)) {
            $env:ODCLINIC_DB_URL = 'mysql://root:' + [uri]::EscapeDataString($pw) + '@localhost:3306/demo'
            return $true
        }
    }
    return $false
}

$plan = @(
    @{ Name = 'workspace-tests'; Phases = @('all','test'); Action = { & cargo test --manifest-path $manifest --workspace } },
    @{ Name = 'live-mysql'; Phases = @('all','live'); Action = {
        if (-not (Ensure-DbUrl)) { throw 'ODCLINIC_DB_URL/MYSQL_ROOT_PASSWORD not set and stored credential not found' }
        & cargo test --manifest-path $manifest -p od-db --test mysql_live -- --ignored --nocapture --test-threads=1
    } },
    @{ Name = 'check-all-targets'; Phases = @('all','check'); Action = { & cargo check --manifest-path $manifest --workspace --all-targets } },
    @{ Name = 'build-workspace'; Phases = @('all','build'); Action = { & cargo build --manifest-path $manifest --workspace } },
    @{ Name = 'build-release'; Phases = @('release'); Action = { & cargo build --manifest-path $manifest --workspace --release } },
    @{ Name = 'smoke-mysql'; Phases = @('all','smoke'); Action = {
        if (-not (Ensure-DbUrl)) { throw 'ODCLINIC_DB_URL/MYSQL_ROOT_PASSWORD not set and stored credential not found' }
        $exe = Join-Path $repoRoot 'target\debug\odclinic.exe'
        if (-not (Test-Path -LiteralPath $exe)) { & cargo build --manifest-path $manifest --workspace }
        $proc = Start-Process -FilePath $exe -PassThru
        Start-Sleep -Seconds 3
        $alive = -not $proc.HasExited
        if ($alive) {
            Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
            Write-Host 'SMOKE OK: odclinic.exe stayed running for 3 seconds with MySQL backend.'
        } else {
            throw "SMOKE FAILED: odclinic.exe exited early with code $($proc.ExitCode)"
        }
    } }
)

if ($List) {
    $plan | ForEach-Object { [pscustomobject]@{ Name = $_.Name; Phases = ($_.Phases -join ',') } } | Format-Table -AutoSize
    exit 0
}

$selected = $plan | Where-Object { $_.Phases -contains $Phase }
if ($Phase -eq 'all') {
    if ($SkipLive) { $selected = $selected | Where-Object { $_.Name -ne 'live-mysql' } }
    if ($SkipSmoke) { $selected = $selected | Where-Object { $_.Name -ne 'smoke-mysql' } }
}

$failed = $false
foreach ($step in $selected) {
    if ($Phase -eq 'all' -and $SkipLive -and $step.Name -eq 'live-mysql') { Write-Phase $step.Name 'SKIP' 'SkipLive'; continue }
    if ($Phase -eq 'all' -and $SkipSmoke -and $step.Name -eq 'smoke-mysql') { Write-Phase $step.Name 'SKIP' 'SkipSmoke'; continue }
    $ok = Invoke-Step $step.Name $step.Action
    if (-not $ok) { $failed = $true; break }
}

Write-Host "`n=== Build Sequence Summary ===" -ForegroundColor Cyan
$results | Format-Table -AutoSize
if ($failed) { exit 1 } else { exit 0 }