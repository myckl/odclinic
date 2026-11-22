$ErrorActionPreference = 'Stop'

$secretPath = Join-Path $env:USERPROFILE '.config\openchamber\secrets\659cea82ea5e5605b1553d1e214fc5a8'
if ([string]::IsNullOrWhiteSpace($env:ODCLINIC_DB_URL)) {
    $password = $env:MYSQL_ROOT_PASSWORD
    if ([string]::IsNullOrWhiteSpace($password) -and (Test-Path -LiteralPath $secretPath)) {
        $password = (Get-Content -LiteralPath $secretPath -Raw).Trim()
    }
    if ([string]::IsNullOrWhiteSpace($password)) {
        throw 'Set ODCLINIC_DB_URL or MYSQL_ROOT_PASSWORD, or place the stored credential at the expected secrets path.'
    }
    $env:ODCLINIC_DB_URL = 'mysql://root:' + [uri]::EscapeDataString($password) + '@localhost:3306/demo'
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$exe = Join-Path $repoRoot 'target\debug\odclinic.exe'
if (-not (Test-Path -LiteralPath $exe)) {
    & cargo build --manifest-path (Join-Path $repoRoot 'Cargo.toml') --workspace
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

$process = Start-Process -FilePath $exe -PassThru
Start-Sleep -Seconds 3
$alive = -not $process.HasExited
if ($alive) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    Write-Output "SMOKE OK: odclinic.exe stayed running for 3 seconds with the MySQL backend."
    exit 0
} else {
    Write-Output "SMOKE FAILED: odclinic.exe exited early with code $($process.ExitCode)."
    exit 1
}
