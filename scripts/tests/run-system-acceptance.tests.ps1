[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "../run-system-acceptance.ps1")

function Assert-Equal {
    param(
        [Parameter(Mandatory = $true)]$Expected,
        [Parameter(Mandatory = $true)]$Actual,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if ($Expected -ne $Actual) {
        throw "$Message; expected='$Expected'; actual='$Actual'"
    }
}

function Assert-Throws {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$Message
    )

    $threw = $false
    try {
        & $Action
    }
    catch {
        $threw = $true
    }
    if (-not $threw) {
        throw $Message
    }
}

$repositoryRoot = Get-EamRepositoryRoot
$summary = Test-AcceptanceMatrix -RepositoryRoot $repositoryRoot
Assert-Equal 33 $summary.DeterministicCriteria "DET count drifted"
Assert-Equal 12 $summary.FunctionalRequirements "FR count drifted"
Assert-Equal 51 $summary.AcceptedAdrs "accepted ADR count drifted"
Assert-Equal 8 $summary.ThreatBoundaries "threat count drifted"
Assert-Equal 5 $summary.MigrationContracts "migration count drifted"
Assert-Equal 33 $summary.EvidenceEntries "evidence registry count drifted"

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("eam-acceptance-tests-" + [guid]::NewGuid().ToString("N"))
[System.IO.Directory]::CreateDirectory($tempRoot) | Out-Null
try {
    $matrixPath = Join-Path $repositoryRoot "docs/system-acceptance-v1.md"
    $brokenMatrix = Join-Path $tempRoot "broken-matrix.md"
    $matrixText = Read-Utf8Text -Path $matrixPath
    $brokenText = [regex]::Replace($matrixText, '(?m)^\| ADR-0053 .*\r?\n', '', 1)
    $encoding = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($brokenMatrix, $brokenText, $encoding)
    Assert-Throws {
        Test-AcceptanceMatrix -RepositoryRoot $repositoryRoot -MatrixPath $brokenMatrix | Out-Null
    } "missing accepted ADR did not fail the matrix gate"

    $missingProfileEvidence = Join-Path $tempRoot "missing-runtime-profile-evidence.md"
    $missingProfileText = [regex]::Replace(
        $matrixText,
        '(?m)^(\| ADR-0053 \|[^|]+\|)\s*[^|]+?(\| automated \|)$',
        '$1 EV-RUNTIME, EV-VAULT, EV-DESKTOP $2',
        1
    )
    if ($missingProfileText -eq $matrixText) {
        throw "runtime profile rejection fixture did not change ADR-0053"
    }
    [System.IO.File]::WriteAllText($missingProfileEvidence, $missingProfileText, $encoding)
    Assert-Throws {
        Test-AcceptanceMatrix -RepositoryRoot $repositoryRoot -MatrixPath $missingProfileEvidence | Out-Null
    } "ADR-0053 without runtime profile evidence did not fail the matrix gate"

    $staleMigration = Join-Path $tempRoot "stale-runtime-profile-migration.md"
    $staleMigrationText = [regex]::Replace(
        $matrixText,
        '(?m)^(\| MIG-01 \|[^|]+\|)\s*[^|]+?(\| automated \|)$',
        '$1 EV-SCHEMA $2',
        1
    )
    if ($staleMigrationText -eq $matrixText) {
        throw "runtime profile rejection fixture did not change MIG-01"
    }
    [System.IO.File]::WriteAllText($staleMigration, $staleMigrationText, $encoding)
    Assert-Throws {
        Test-AcceptanceMatrix -RepositoryRoot $repositoryRoot -MatrixPath $staleMigration | Out-Null
    } "schema v26 migration without runtime profile evidence did not fail the matrix gate"

    $fakeInstaller = Join-Path $tempRoot "evrything-about-me_0.1.0_x64-setup.exe"
    [System.IO.File]::WriteAllBytes($fakeInstaller, [byte[]](0, 1, 2, 3))
    $metadata = Get-InstallerMetadata -Path $fakeInstaller -Version "0.1.0"
    Assert-Equal 4 $metadata.Bytes "installer byte count drifted"
    Assert-Equal 64 $metadata.Sha256.Length "installer SHA-256 length drifted"
}
finally {
    if (Test-Path -LiteralPath $tempRoot) {
        $resolved = [System.IO.Path]::GetFullPath($tempRoot)
        $tempBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        if (-not $resolved.StartsWith($tempBase, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "refusing to clean a self-test directory outside TEMP"
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}

$secret = '-----BEGIN ' + 'PRIVATE KEY-----'
$matches = @(Get-SensitiveTextMatches -Text $secret -Path "fixture")
Assert-Equal 1 $matches.Count "private-key branch was not detected"
Assert-Equal 0 @(Get-SensitiveTextMatches -Text "synthetic fixture" -Path "fixture").Count "safe text was rejected"

$privacy = Test-PrivacyBoundary -RepositoryRoot $repositoryRoot
Assert-Equal 0 $privacy.Violations "repository privacy scan failed"
$static = Test-StaticSecurityBoundary -RepositoryRoot $repositoryRoot
Assert-Equal "0.1.0" $static.Version "release version drifted"
$links = Test-MarkdownLocalLinks -RepositoryRoot $repositoryRoot
Assert-Equal 0 $links.MissingLinks "Markdown links are missing"

$successOutput = @(
    Invoke-ExternalGate -Name "expected-success" -FilePath "cmd.exe" -Arguments @("/d", "/c", "exit", "0") -WorkingDirectory $repositoryRoot
)
Assert-Equal 0 $successOutput.Count "successful native output leaked into structured results"

Assert-Throws {
    Invoke-ExternalGate -Name "expected-failure" -FilePath "cmd.exe" -Arguments @("/d", "/c", "exit", "7") -WorkingDirectory $repositoryRoot
} "non-zero native command did not fail the gate"

Write-Host "run-system-acceptance.tests.ps1: PASS"
