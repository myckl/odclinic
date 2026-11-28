$ErrorActionPreference = 'Stop'

# The suite writes fixed demo-DB fixtures; a second concurrent wrapper can make
# one run read another run's temporary rows. `--test-threads=1` serializes the
# tests inside one process, and this mutex serializes whole wrapper invocations.
$mutex = New-Object System.Threading.Mutex($false, 'Global\odclinic-live-mysql')
if (-not $mutex.WaitOne(0)) {
    Write-Output 'Another odclinic live MySQL suite is already running; skipping this duplicate invocation.'
    exit 0
}

try {
    # The stored credential file is generated per request, so accept an explicit
    # override and try the known files in order. The password is never printed.
    $secretPaths = @()
    if (-not [string]::IsNullOrWhiteSpace($env:ODCLINIC_DB_SECRET_PATH)) {
        $secretPaths += $env:ODCLINIC_DB_SECRET_PATH
    }
    $secretPaths += Join-Path $env:USERPROFILE '.config\openchamber\secrets\659cea82ea5e5605b1553d1e214fc5a8'
    $secretPaths += Join-Path $env:USERPROFILE '.config\openchamber\secrets\1d43d56de0ddf1aaf0654378aca9bf45'
    # secret.request uses a request-specific filename, so also try the newest
    # files in that directory. Their contents are read locally only to build
    # the connection URL; they are never printed or copied into the project.
    $secretsDir = Join-Path $env:USERPROFILE '.config\openchamber\secrets'
    if (Test-Path -LiteralPath $secretsDir) {
        Get-ChildItem -LiteralPath $secretsDir -File -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending |
            ForEach-Object { $secretPaths += $_.FullName }
    }

    if ([string]::IsNullOrWhiteSpace($env:ODCLINIC_DB_URL)) {
        $password = $env:MYSQL_ROOT_PASSWORD
        if ([string]::IsNullOrWhiteSpace($password)) {
            foreach ($secretPath in $secretPaths) {
                if (-not (Test-Path -LiteralPath $secretPath)) {
                    continue
                }
                $candidate = (Get-Content -LiteralPath $secretPath -Raw).Trim()
                if (-not [string]::IsNullOrWhiteSpace($candidate)) {
                    $password = $candidate
                    break
                }
            }
        }
        if ([string]::IsNullOrWhiteSpace($password)) {
            throw 'Set ODCLINIC_DB_URL, MYSQL_ROOT_PASSWORD, or ODCLINIC_DB_SECRET_PATH, or place the stored credential at a known secrets path.'
        }
        # Open Dental's FreeDentalConfig.xml points at localhost / database "demo" / user root.
        $env:ODCLINIC_DB_URL = 'mysql://root:' + [uri]::EscapeDataString($password) + '@localhost:3306/demo'
    }

    # This harness exists to exercise OD-owned writes against an untouched
    # schema, so it explicitly opts into them. The application itself still
    # fails closed (WritePolicy::DenyOdWrites) unless an operator opts in.
    $env:ODCLINIC_ALLOW_OD_WRITES = '1'

    $manifest = Join-Path $PSScriptRoot '..\Cargo.toml'
    $previousPreference = $ErrorActionPreference
    try {
        # Cargo writes normal progress to stderr. PowerShell 5.x turns that into a
        # NativeCommandError under ErrorActionPreference=Stop, which aborted the
        # wrapper even when every test passed. Merge stderr into the visible output
        # for this call only, then trust cargo's exit code.
        $ErrorActionPreference = 'Continue'
        & cargo test --manifest-path $manifest -p od-db --test mysql_live -- --ignored --nocapture --test-threads=1 2>&1
        $code = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    if ($null -eq $code) {
        throw 'cargo did not run; check that it is installed and on PATH.'
    }
}
finally {
    $mutex.ReleaseMutex()
    $mutex.Dispose()
}

exit $code
