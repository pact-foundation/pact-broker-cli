# Deprecation shim — delegates to the cargo-dist-generated installer.
#
# DEPRECATED: will be removed after 2027-01-01.
# Use the new installer instead:
#
#   powershell -ExecutionPolicy ByPass -c "irm https://github.com/pact-foundation/pact-broker-cli/releases/latest/download/pact-broker-cli-installer.ps1 | iex"
#
# To install a specific version:
#   powershell -ExecutionPolicy ByPass -c "irm https://github.com/pact-foundation/pact-broker-cli/releases/download/<VERSION>/pact-broker-cli-installer.ps1 | iex"

Write-Warning @"
DEPRECATION NOTICE

This install script is deprecated and will stop working following 2027-01-01.
Use the new installer instead:

  powershell -ExecutionPolicy ByPass -c ``"irm https://github.com/pact-foundation/pact-broker-cli/releases/latest/download/pact-broker-cli-installer.ps1 | iex``"
"@

$version = $env:PACT_BROKER_CLI_VERSION
if ([string]::IsNullOrEmpty($version) -or $version -eq "vlatest") {
    $installerUrl = "https://github.com/pact-foundation/pact-broker-cli/releases/latest/download/pact-broker-cli-installer.ps1"
} else {
    $installerUrl = "https://github.com/pact-foundation/pact-broker-cli/releases/download/$version/pact-broker-cli-installer.ps1"
}

try {
    $installerScript = Invoke-RestMethod -Uri $installerUrl -ErrorAction Stop
    Invoke-Expression $installerScript
} catch {
    if ($_.Exception.Response -ne $null -and $_.Exception.Response.StatusCode -eq 404) {
        Write-Error "Failed to download installer from: $installerUrl"
        Write-Error "Versions older than v0.8.0 do not have a cargo-dist installer."
        $versionTag = if ([string]::IsNullOrEmpty($version)) { 'latest' } else { $version }
        Write-Error "Download manually from: https://github.com/pact-foundation/pact-broker-cli/releases/tag/$versionTag"
    } else {
        Write-Error "Failed to download installer: $_"
    }
    exit 1
}
