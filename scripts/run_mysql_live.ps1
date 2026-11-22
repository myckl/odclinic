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
    # Open Dental's FreeDentalConfig.xml points at localhost / database "demo" / user root.
    $env:ODCLINIC_DB_URL = 'mysql://root:' + [uri]::EscapeDataString($password) + '@localhost:3306/demo'
}

$manifest = Join-Path $PSScriptRoot '..\Cargo.toml'
& cargo test --manifest-path $manifest -p od-db --test mysql_live -- --ignored --nocapture --test-threads=1
exit $LASTEXITCODE
