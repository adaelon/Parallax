[CmdletBinding()]
param(
    [ValidateSet("Validate", "Full", "Package", "Smoke")]
    [string]$Mode = "Full",
    [string]$InstallerPath,
    [string]$ResultsPath,
    [switch]$KeepSmokeDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$script:AcceptanceStepResults = @()

function Get-EamRepositoryRoot {
    return [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
}

function Read-Utf8Lines {
    param([Parameter(Mandatory = $true)][string]$Path)

    return [System.IO.File]::ReadAllLines($Path, [System.Text.Encoding]::UTF8)
}

function Read-Utf8Text {
    param([Parameter(Mandatory = $true)][string]$Path)

    return [System.IO.File]::ReadAllText($Path, [System.Text.Encoding]::UTF8)
}

function Get-ExpectedDeterministicCriteria {
    param([Parameter(Mandatory = $true)][string]$ProductSpecPath)

    $inside = $false
    $labels = @()
    foreach ($line in (Read-Utf8Lines -Path $ProductSpecPath)) {
        if ($line -match '^###\s+6\.1\s+') {
            $inside = $true
            continue
        }
        if ($inside -and $line -match '^###\s+6\.2\s+') {
            break
        }
        if ($inside -and $line -match '^\|\s*([^|]+?)\s*\|') {
            $label = $Matches[1].Trim()
            if ($label -notmatch '^-+$') {
                $labels += $label
            }
        }
    }

    if ($labels.Count -lt 2) {
        throw "product section 6.1 has no acceptance table"
    }

    $result = @()
    for ($index = 1; $index -lt $labels.Count; $index++) {
        $result += [pscustomobject]@{
            Id = "DET-{0:D2}" -f $index
            Name = $labels[$index]
        }
    }
    return $result
}

function Get-ExpectedFunctionalRequirements {
    param([Parameter(Mandatory = $true)][string]$ProductSpecPath)

    $result = @()
    foreach ($line in (Read-Utf8Lines -Path $ProductSpecPath)) {
        if ($line -match '^###\s+(FR-\d{2})\s+(.+?)\s*$') {
            $result += [pscustomobject]@{
                Id = $Matches[1]
                Name = $Matches[2].Trim()
            }
        }
    }
    return $result
}

function Get-ExpectedAcceptedAdrs {
    param([Parameter(Mandatory = $true)][string]$AdrDirectory)

    $result = @()
    foreach ($file in (Get-ChildItem -LiteralPath $AdrDirectory -File -Filter "*.md" | Sort-Object Name)) {
        $lines = Read-Utf8Lines -Path $file.FullName
        if (-not ($lines -match '^status:\s*accepted\s*$')) {
            continue
        }
        if ($file.BaseName -notmatch '^(\d{4})-') {
            throw "accepted ADR has a non-canonical filename: $($file.Name)"
        }
        $adrNumber = $Matches[1]
        $title = @($lines | Where-Object { $_ -match '^#\s+' } | Select-Object -First 1)
        if ($title.Count -ne 1) {
            throw "accepted ADR has no title: $($file.Name)"
        }
        $result += [pscustomobject]@{
            Id = "ADR-$adrNumber"
            Name = ($title[0] -replace '^#\s+', '').Trim()
        }
    }
    return $result
}

function Get-AcceptanceMatrixRows {
    param([Parameter(Mandatory = $true)][string]$MatrixPath)

    $rows = @()
    foreach ($line in (Read-Utf8Lines -Path $MatrixPath)) {
        $match = [regex]::Match(
            $line,
            '^\|\s*((?:DET|FR|ADR|THR|MIG)-\d+)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|$'
        )
        if (-not $match.Success) {
            continue
        }
        $evidence = @(
            [regex]::Matches($match.Groups[3].Value, '\bEV-[A-Z0-9-]+\b') |
                ForEach-Object { $_.Value }
        )
        $rows += [pscustomobject]@{
            Id = $match.Groups[1].Value.Trim()
            Name = $match.Groups[2].Value.Trim()
            Evidence = $evidence
            Status = $match.Groups[4].Value.Trim()
        }
    }
    return $rows
}

function Get-EvidenceRegistryIds {
    param([Parameter(Mandatory = $true)][string]$MatrixPath)

    $ids = @()
    foreach ($line in (Read-Utf8Lines -Path $MatrixPath)) {
        if ($line -match '^\|\s*(EV-[A-Z0-9-]+)\s*\|') {
            $ids += $Matches[1]
        }
    }
    return $ids
}

function Assert-ExactIdSet {
    param(
        [Parameter(Mandatory = $true)][string]$Group,
        [Parameter(Mandatory = $true)][string[]]$Expected,
        [Parameter(Mandatory = $true)][string[]]$Actual
    )

    $missing = @($Expected | Where-Object { $_ -notin $Actual })
    $unexpected = @($Actual | Where-Object { $_ -notin $Expected })
    $duplicates = @(
        $Actual | Group-Object | Where-Object { $_.Count -ne 1 } | ForEach-Object { $_.Name }
    )
    if ($missing.Count -gt 0 -or $unexpected.Count -gt 0 -or $duplicates.Count -gt 0) {
        throw "$Group matrix mismatch; missing=[$($missing -join ',')]; unexpected=[$($unexpected -join ',')]; duplicates=[$($duplicates -join ',')]"
    }
}

function Test-AcceptanceMatrix {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [string]$MatrixPath = (Join-Path $RepositoryRoot "docs/system-acceptance-v1.md")
    )

    $det = @(Get-ExpectedDeterministicCriteria -ProductSpecPath (Join-Path $RepositoryRoot "docs/product-spec.md"))
    $fr = @(Get-ExpectedFunctionalRequirements -ProductSpecPath (Join-Path $RepositoryRoot "docs/product-spec.md"))
    $adr = @(Get-ExpectedAcceptedAdrs -AdrDirectory (Join-Path $RepositoryRoot "docs/adr"))
    $rows = @(Get-AcceptanceMatrixRows -MatrixPath $MatrixPath)
    $registry = @(Get-EvidenceRegistryIds -MatrixPath $MatrixPath)

    if (@($registry | Group-Object | Where-Object { $_.Count -ne 1 }).Count -gt 0) {
        throw "evidence registry contains duplicate IDs"
    }

    $expectedThreats = @(1..8 | ForEach-Object { "THR-{0:D2}" -f $_ })
    $expectedMigrations = @(1..5 | ForEach-Object { "MIG-{0:D2}" -f $_ })
    $groups = @(
        [pscustomobject]@{ Name = "DET"; Expected = @($det.Id); Rows = @($rows | Where-Object { $_.Id -like 'DET-*' }) },
        [pscustomobject]@{ Name = "FR"; Expected = @($fr.Id); Rows = @($rows | Where-Object { $_.Id -like 'FR-*' }) },
        [pscustomobject]@{ Name = "ADR"; Expected = @($adr.Id); Rows = @($rows | Where-Object { $_.Id -like 'ADR-*' }) },
        [pscustomobject]@{ Name = "THR"; Expected = $expectedThreats; Rows = @($rows | Where-Object { $_.Id -like 'THR-*' }) },
        [pscustomobject]@{ Name = "MIG"; Expected = $expectedMigrations; Rows = @($rows | Where-Object { $_.Id -like 'MIG-*' }) }
    )

    foreach ($group in $groups) {
        Assert-ExactIdSet -Group $group.Name -Expected $group.Expected -Actual @($group.Rows.Id)
    }

    $expectedNames = @{}
    foreach ($item in @($det + $fr)) {
        $expectedNames[$item.Id] = $item.Name
    }
    foreach ($row in @($rows | Where-Object { $_.Id -like 'DET-*' -or $_.Id -like 'FR-*' })) {
        if ($row.Name -ne $expectedNames[$row.Id]) {
            throw "matrix label drift for $($row.Id): '$($row.Name)'"
        }
    }

    foreach ($row in $rows) {
        if ($row.Status -ne "automated") {
            throw "matrix row $($row.Id) is not automated"
        }
        if ($row.Evidence.Count -eq 0) {
            throw "matrix row $($row.Id) has no evidence"
        }
        $undefined = @($row.Evidence | Where-Object { $_ -notin $registry })
        if ($undefined.Count -gt 0) {
            throw "matrix row $($row.Id) references undefined evidence: $($undefined -join ',')"
        }
    }

    return [pscustomobject]@{
        DeterministicCriteria = $det.Count
        FunctionalRequirements = $fr.Count
        AcceptedAdrs = $adr.Count
        ThreatBoundaries = $expectedThreats.Count
        MigrationContracts = $expectedMigrations.Count
        EvidenceEntries = $registry.Count
    }
}

function Get-SensitiveTextMatches {
    param(
        [Parameter(Mandatory = $true)][string]$Text,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $patterns = @(
        [pscustomobject]@{ Name = "private-key"; Pattern = ('-----BEGIN ' + '(?:RSA |EC |OPENSSH )?' + 'PRIVATE KEY-----') },
        [pscustomobject]@{ Name = "openai-token"; Pattern = ('\b' + 'sk-' + '[A-Za-z0-9_-]{20,}\b') },
        [pscustomobject]@{ Name = "github-token"; Pattern = ('\b' + 'ghp_' + '[A-Za-z0-9]{20,}\b') },
        [pscustomobject]@{ Name = "aws-access-key"; Pattern = ('\b' + 'AKIA' + '[0-9A-Z]{16}\b') },
        [pscustomobject]@{ Name = "windows-user-path"; Pattern = ('[A-Za-z]:' + '\\Users\\' + '[^\\\s]+') },
        [pscustomobject]@{ Name = "unix-user-path"; Pattern = ('/' + '(?:home|Users)/' + '[^/\s]+') }
    )

    $matches = @()
    foreach ($candidate in $patterns) {
        if ([regex]::IsMatch($Text, $candidate.Pattern, [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)) {
            $matches += "${Path}:$($candidate.Name)"
        }
    }
    return $matches
}

function Get-RepositoryFilesForCommit {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $output = @(& git -C $RepositoryRoot ls-files --cached --others --exclude-standard)
    if ($LASTEXITCODE -ne 0) {
        throw "git ls-files failed"
    }
    return @($output | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
}

function Test-PrivacyBoundary {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    foreach ($probe in @(
        ".local/g10-personal-baseline/private.md",
        ".local/longitudinal-observations/day-01.md",
        ".local/system-acceptance/latest.json"
    )) {
        & git -C $RepositoryRoot check-ignore --quiet --no-index -- $probe
        if ($LASTEXITCODE -ne 0) {
            throw "private local path is not ignored: $probe"
        }
    }

    $tracked = @(Get-RepositoryFilesForCommit -RepositoryRoot $RepositoryRoot)
    $forbiddenPaths = @(
        $tracked | Where-Object {
            $_ -match '^(?i:\.local/)' -or
            $_ -match '(?i:(?:^|/)(?:g10-personal-baseline|longitudinal-observations)(?:/|$))'
        }
    )
    if ($forbiddenPaths.Count -gt 0) {
        throw "private baseline paths are tracked: $($forbiddenPaths -join ',')"
    }

    $textExtensions = @(".css", ".gitignore", ".html", ".json", ".lock", ".md", ".mjs", ".ps1", ".rs", ".toml", ".ts", ".tsx")
    $violations = @()
    $scanned = 0
    foreach ($relative in $tracked) {
        $absolute = Join-Path $RepositoryRoot $relative
        if (-not (Test-Path -LiteralPath $absolute -PathType Leaf)) {
            continue
        }
        $extension = [System.IO.Path]::GetExtension($relative).ToLowerInvariant()
        if ($relative -ne ".gitignore" -and $extension -notin $textExtensions) {
            continue
        }
        $text = Read-Utf8Text -Path $absolute
        $violations += @(Get-SensitiveTextMatches -Text $text -Path $relative)
        $scanned++
    }
    if ($violations.Count -gt 0) {
        throw "tracked privacy scan failed: $($violations -join ',')"
    }

    return [pscustomobject]@{
        CandidateFiles = $tracked.Count
        ScannedTextFiles = $scanned
        Violations = 0
    }
}

function Get-ReleaseVersion {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $desktop = (Read-Utf8Text -Path (Join-Path $RepositoryRoot "apps/desktop/package.json") | ConvertFrom-Json).version
    $extension = (Read-Utf8Text -Path (Join-Path $RepositoryRoot "apps/browser-extension/package.json") | ConvertFrom-Json).version
    $tauri = (Read-Utf8Text -Path (Join-Path $RepositoryRoot "apps/desktop/src-tauri/tauri.conf.json") | ConvertFrom-Json).version
    $cargoText = Read-Utf8Text -Path (Join-Path $RepositoryRoot "apps/desktop/src-tauri/Cargo.toml")
    $cargoMatch = [regex]::Match($cargoText, '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"')
    if (-not $cargoMatch.Success) {
        throw "desktop Cargo package version is missing"
    }
    $versions = @($desktop, $extension, $tauri, $cargoMatch.Groups[1].Value)
    if (@($versions | Sort-Object -Unique).Count -ne 1) {
        throw "release version drift: $($versions -join ',')"
    }
    return [string]$tauri
}

function Test-StaticSecurityBoundary {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $capability = Read-Utf8Text -Path (Join-Path $RepositoryRoot "apps/desktop/src-tauri/capabilities/main.json") | ConvertFrom-Json
    $permissions = @($capability.permissions)
    if ($permissions.Count -ne 1 -or $permissions[0] -ne "core:default") {
        throw "desktop WebView capability is broader than core:default"
    }

    $tauri = Read-Utf8Text -Path (Join-Path $RepositoryRoot "apps/desktop/src-tauri/tauri.conf.json") | ConvertFrom-Json
    if (-not $tauri.bundle.active -or @($tauri.bundle.targets) -notcontains "nsis") {
        throw "Tauri NSIS bundling is not active"
    }
    if ($tauri.bundle.createUpdaterArtifacts -ne $false) {
        throw "local S31 builds must not request updater artifacts without release signing material"
    }
    if (@($tauri.app.security.capabilities).Count -ne 1 -or $tauri.app.security.capabilities[0] -ne "main") {
        throw "desktop security capabilities drifted"
    }
    if ([string]$tauri.app.security.csp -match '(?:^|[;\s])\*(?:[;\s]|$)') {
        throw "desktop CSP contains a wildcard source"
    }

    $manifest = Read-Utf8Text -Path (Join-Path $RepositoryRoot "apps/browser-extension/public/manifest.json") | ConvertFrom-Json
    if (@($manifest.host_permissions).Count -ne 1 -or $manifest.host_permissions[0] -ne "http://127.0.0.1:43129/*") {
        throw "browser host permission is not pinned to the loopback endpoint"
    }
    $forbidden = @("cookies", "debugger", "nativeMessaging", "webRequest", "webRequestBlocking")
    $granted = @($manifest.permissions)
    if (@($granted | Where-Object { $_ -in $forbidden }).Count -gt 0) {
        throw "browser manifest contains a forbidden broad permission"
    }

    return [pscustomobject]@{
        Version = Get-ReleaseVersion -RepositoryRoot $RepositoryRoot
        DesktopPermissions = $permissions.Count
        BrowserNamedPermissions = $granted.Count
        NsisActive = $true
    }
}

function Test-MarkdownLocalLinks {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $missing = @()
    $checked = 0
    foreach ($relative in @(
        Get-RepositoryFilesForCommit -RepositoryRoot $RepositoryRoot |
            Where-Object { $_ -like "*.md" -and $_ -notmatch '(?i)(?:^|/)tests/fixtures/' }
    )) {
        $absolute = Join-Path $RepositoryRoot $relative
        $base = Split-Path -Parent $absolute
        $content = Read-Utf8Text -Path $absolute
        foreach ($match in [regex]::Matches($content, '\[[^\]]+\]\(([^)]+)\)')) {
            $target = $match.Groups[1].Value.Trim()
            if ($target -match '^(?:[a-z]+:|#)' -or [string]::IsNullOrWhiteSpace($target)) {
                continue
            }
            $pathPart = ($target -split '#', 2)[0]
            $pathPart = [System.Uri]::UnescapeDataString($pathPart.Trim('<', '>'))
            $candidate = [System.IO.Path]::GetFullPath((Join-Path $base $pathPart))
            $checked++
            if (-not (Test-Path -LiteralPath $candidate)) {
                $missing += "$relative->$target"
            }
        }
    }
    if ($missing.Count -gt 0) {
        throw "missing local Markdown links: $($missing -join ',')"
    }
    return [pscustomobject]@{ CheckedLinks = $checked; MissingLinks = 0 }
}

function Get-InstallerMetadata {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Version
    )

    $resolved = (Resolve-Path -LiteralPath $Path).Path
    $file = Get-Item -LiteralPath $resolved
    if ($file.Extension -ne ".exe" -or $file.Name -notmatch [regex]::Escape($Version)) {
        throw "installer filename does not carry release version ${Version}: $($file.Name)"
    }
    $hash = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
    return [pscustomobject]@{
        Path = $resolved
        FileName = $file.Name
        Version = $Version
        Sha256 = $hash
        Bytes = $file.Length
    }
}

function Find-LatestNsisInstaller {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $directory = Join-Path $RepositoryRoot "target/release/bundle/nsis"
    $candidate = Get-ChildItem -LiteralPath $directory -File -Filter "*-setup.exe" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    if ($null -eq $candidate) {
        throw "no NSIS installer found under $directory"
    }
    return $candidate.FullName
}

function Invoke-InstallerSmoke {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string]$Installer,
        [switch]$KeepDirectory
    )

    $smokeBase = [System.IO.Path]::GetFullPath((Join-Path $RepositoryRoot ".local/system-acceptance/smoke"))
    [System.IO.Directory]::CreateDirectory($smokeBase) | Out-Null
    $smokeRoot = [System.IO.Path]::GetFullPath((Join-Path $smokeBase ([guid]::NewGuid().ToString("N"))))
    if (-not $smokeRoot.StartsWith($smokeBase + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "refusing to create smoke directory outside the private smoke root"
    }
    $installRoot = Join-Path $smokeRoot "install"
    $vaultRoot = Join-Path $smokeRoot "vault"
    [System.IO.Directory]::CreateDirectory($installRoot) | Out-Null

    $appProcess = $null
    $uninstaller = $null
    $closeHidWindow = $false
    try {
        $install = Start-Process -FilePath $Installer -ArgumentList @("/S", "/D=$installRoot") -Wait -PassThru
        if ($install.ExitCode -ne 0) {
            throw "NSIS silent install failed with exit code $($install.ExitCode)"
        }

        $app = Get-ChildItem -LiteralPath $installRoot -Recurse -File -Filter "evrything-about-me.exe" | Select-Object -First 1
        $uninstaller = Get-ChildItem -LiteralPath $installRoot -Recurse -File -Filter "uninstall.exe" | Select-Object -First 1
        if ($null -eq $app -or $null -eq $uninstaller) {
            throw "installed application or uninstaller is missing"
        }

        Push-Location $RepositoryRoot
        try {
            & cargo run --quiet -p vault --example seed_installer_smoke_vault -- $vaultRoot | Out-Host
            $seedExitCode = $LASTEXITCODE
        }
        finally {
            Pop-Location
        }
        if ($seedExitCode -ne 0) {
            throw "installer smoke vault seeding failed with exit code $seedExitCode"
        }
        if (-not (Test-Path -LiteralPath (Join-Path $vaultRoot "bundle.meta") -PathType Leaf)) {
            throw "installer smoke vault metadata was not committed"
        }
        if (Test-Path -LiteralPath (Join-Path $vaultRoot "self.db")) {
            throw "installer smoke seed unexpectedly created the encrypted database"
        }

        $oldVaultRoot = $env:EAM_VAULT_ROOT
        $env:EAM_VAULT_ROOT = $vaultRoot
        try {
            $appProcess = Start-Process -FilePath $app.FullName -ArgumentList @("--background") -PassThru
        }
        finally {
            if ($null -eq $oldVaultRoot) {
                Remove-Item Env:EAM_VAULT_ROOT -ErrorAction SilentlyContinue
            }
            else {
                $env:EAM_VAULT_ROOT = $oldVaultRoot
            }
        }

        Start-Sleep -Seconds 3
        if ($appProcess.HasExited) {
            throw "installed application exited during startup smoke"
        }
        if (-not (Test-Path -LiteralPath (Join-Path $vaultRoot "self.db") -PathType Leaf)) {
            throw "installed application stayed alive without opening the seeded vault"
        }
        $closeHidWindow = $appProcess.CloseMainWindow()
        Start-Sleep -Milliseconds 750
        if ($appProcess.HasExited) {
            throw "window close terminated the tray-resident process"
        }

        Stop-Process -Id $appProcess.Id -Force
        $appProcess.WaitForExit(10000) | Out-Null
        $appProcess = $null

        $uninstall = Start-Process -FilePath $uninstaller.FullName -ArgumentList @("/S") -Wait -PassThru
        if ($uninstall.ExitCode -ne 0) {
            throw "NSIS silent uninstall failed with exit code $($uninstall.ExitCode)"
        }
        Start-Sleep -Milliseconds 750
        if (Test-Path -LiteralPath $app.FullName) {
            throw "installed executable remains after uninstall"
        }

        return [pscustomobject]@{
            Install = "pass"
            Start = "pass"
            CloseKeepsTrayHost = if ($closeHidWindow) { "pass" } else { "not-signaled-background" }
            Exit = "pass-existing-secure-shutdown-tests-plus-process-cleanup"
            Uninstall = "pass"
            SeededVaultMetadata = "pass"
            InstalledAppCreatedDatabase = "pass"
            EphemeralVaultRoot = $vaultRoot
        }
    }
    finally {
        if ($null -ne $appProcess -and -not $appProcess.HasExited) {
            Stop-Process -Id $appProcess.Id -Force -ErrorAction SilentlyContinue
        }
        if (-not $KeepDirectory -and (Test-Path -LiteralPath $smokeRoot)) {
            $resolvedSmoke = [System.IO.Path]::GetFullPath($smokeRoot)
            if (-not $resolvedSmoke.StartsWith($smokeBase + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
                throw "refusing to remove a smoke directory outside the private smoke root"
            }
            Remove-Item -LiteralPath $resolvedSmoke -Recurse -Force
        }
    }
}

function Add-AcceptanceStepResult {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Status,
        [Parameter(Mandatory = $true)][double]$Seconds
    )

    $script:AcceptanceStepResults += [pscustomobject]@{
        name = $Name
        status = $Status
        seconds = [math]::Round($Seconds, 3)
    }
}

function Invoke-InternalGate {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )

    Write-Host "[acceptance] $Name"
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $result = & $Action
        $timer.Stop()
        Add-AcceptanceStepResult -Name $Name -Status "pass" -Seconds $timer.Elapsed.TotalSeconds
        return $result
    }
    catch {
        $timer.Stop()
        Add-AcceptanceStepResult -Name $Name -Status "fail" -Seconds $timer.Elapsed.TotalSeconds
        throw
    }
}

function Invoke-ExternalGate {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    Write-Host "[acceptance] $Name"
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    Push-Location $WorkingDirectory
    try {
        & $FilePath @Arguments | Out-Host
        $exitCode = $LASTEXITCODE
        if ($exitCode -ne 0) {
            throw "$Name failed with exit code $exitCode"
        }
        $timer.Stop()
        Add-AcceptanceStepResult -Name $Name -Status "pass" -Seconds $timer.Elapsed.TotalSeconds
    }
    catch {
        $timer.Stop()
        Add-AcceptanceStepResult -Name $Name -Status "fail" -Seconds $timer.Elapsed.TotalSeconds
        throw
    }
    finally {
        Pop-Location
    }
}

function Write-AcceptanceResult {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][hashtable]$Result
    )

    $fullPath = [System.IO.Path]::GetFullPath($Path)
    [System.IO.Directory]::CreateDirectory((Split-Path -Parent $fullPath)) | Out-Null
    $json = $Result | ConvertTo-Json -Depth 8
    $encoding = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($fullPath, $json + [Environment]::NewLine, $encoding)
}

function Invoke-ValidationGates {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $matrix = Invoke-InternalGate -Name "acceptance-matrix" -Action {
        Test-AcceptanceMatrix -RepositoryRoot $RepositoryRoot
    }
    $privacy = Invoke-InternalGate -Name "privacy-boundary" -Action {
        Test-PrivacyBoundary -RepositoryRoot $RepositoryRoot
    }
    $static = Invoke-InternalGate -Name "static-security-boundary" -Action {
        Test-StaticSecurityBoundary -RepositoryRoot $RepositoryRoot
    }
    $links = Invoke-InternalGate -Name "markdown-local-links" -Action {
        Test-MarkdownLocalLinks -RepositoryRoot $RepositoryRoot
    }
    Invoke-ExternalGate -Name "git-diff-check" -FilePath "git" -Arguments @("diff", "--check") -WorkingDirectory $RepositoryRoot

    return [pscustomobject]@{
        Matrix = $matrix
        Privacy = $privacy
        Static = $static
        Links = $links
    }
}

function Invoke-ScriptSelfTests {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    Invoke-ExternalGate -Name "acceptance-script-tests" -FilePath "powershell.exe" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", (Join-Path $RepositoryRoot "scripts/tests/run-system-acceptance.tests.ps1")
    ) -WorkingDirectory $RepositoryRoot
}

function Invoke-FullRepositoryGates {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    Invoke-ExternalGate -Name "cargo-fmt" -FilePath "cargo" -Arguments @("fmt", "--all", "--", "--check") -WorkingDirectory $RepositoryRoot
    Invoke-ExternalGate -Name "cargo-clippy" -FilePath "cargo" -Arguments @("clippy", "--workspace", "--all-targets", "--", "-D", "warnings") -WorkingDirectory $RepositoryRoot
    Invoke-ExternalGate -Name "cargo-test-workspace" -FilePath "cargo" -Arguments @("test", "--workspace", "--no-fail-fast") -WorkingDirectory $RepositoryRoot
    Invoke-ExternalGate -Name "desktop-rust-check" -FilePath "cargo" -Arguments @("check", "-p", "desktop-app", "--all-targets") -WorkingDirectory $RepositoryRoot

    $desktop = Join-Path $RepositoryRoot "apps/desktop"
    Invoke-ExternalGate -Name "desktop-react-tests" -FilePath "npm.cmd" -Arguments @("test") -WorkingDirectory $desktop
    Invoke-ExternalGate -Name "desktop-typecheck" -FilePath "npm.cmd" -Arguments @("run", "typecheck") -WorkingDirectory $desktop
    Invoke-ExternalGate -Name "desktop-production-build" -FilePath "npm.cmd" -Arguments @("run", "build") -WorkingDirectory $desktop

    $extension = Join-Path $RepositoryRoot "apps/browser-extension"
    Invoke-ExternalGate -Name "extension-tests" -FilePath "npm.cmd" -Arguments @("test") -WorkingDirectory $extension
    Invoke-ExternalGate -Name "extension-typecheck" -FilePath "npm.cmd" -Arguments @("run", "typecheck") -WorkingDirectory $extension
    Invoke-ExternalGate -Name "extension-production-build" -FilePath "npm.cmd" -Arguments @("run", "build") -WorkingDirectory $extension
}

function Invoke-PackageBuild {
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    Invoke-ExternalGate -Name "tauri-nsis-build" -FilePath "npm.cmd" -Arguments @("run", "tauri", "--", "build") -WorkingDirectory (Join-Path $RepositoryRoot "apps/desktop")
    $path = Find-LatestNsisInstaller -RepositoryRoot $RepositoryRoot
    $version = Get-ReleaseVersion -RepositoryRoot $RepositoryRoot
    return Get-InstallerMetadata -Path $path -Version $version
}

function Invoke-SystemAcceptanceMain {
    param(
        [Parameter(Mandatory = $true)][string]$SelectedMode,
        [string]$SelectedInstallerPath,
        [string]$SelectedResultsPath,
        [switch]$KeepSmoke
    )

    $repositoryRoot = Get-EamRepositoryRoot
    if ([string]::IsNullOrWhiteSpace($SelectedResultsPath)) {
        $SelectedResultsPath = Join-Path $repositoryRoot ".local/system-acceptance/latest.json"
    }
    $startedAt = [DateTimeOffset]::Now
    $script:AcceptanceStepResults = @()
    $state = [ordered]@{
        schema = "eam-system-acceptance-result-v1"
        mode = $SelectedMode
        status = "running"
        started_at = $startedAt.ToString("o")
        completed_at = $null
        git_head = $null
        release_version = $null
        validation = $null
        installer = $null
        smoke = $null
        steps = @()
        error = $null
    }

    try {
        $head = @(& git -C $repositoryRoot rev-parse HEAD)
        if ($LASTEXITCODE -ne 0 -or $head.Count -ne 1) {
            throw "cannot resolve git HEAD"
        }
        $state.git_head = $head[0]
        $state.release_version = Get-ReleaseVersion -RepositoryRoot $repositoryRoot

        if ($SelectedMode -in @("Validate", "Full", "Package")) {
            $state.validation = Invoke-ValidationGates -RepositoryRoot $repositoryRoot
            Invoke-ScriptSelfTests -RepositoryRoot $repositoryRoot
        }
        if ($SelectedMode -eq "Full") {
            Invoke-FullRepositoryGates -RepositoryRoot $repositoryRoot
        }
        if ($SelectedMode -in @("Full", "Package")) {
            $state.installer = Invoke-PackageBuild -RepositoryRoot $repositoryRoot
            $SelectedInstallerPath = $state.installer.Path
        }
        if ($SelectedMode -in @("Full", "Smoke")) {
            if ([string]::IsNullOrWhiteSpace($SelectedInstallerPath)) {
                $SelectedInstallerPath = Find-LatestNsisInstaller -RepositoryRoot $repositoryRoot
                $state.installer = Get-InstallerMetadata -Path $SelectedInstallerPath -Version $state.release_version
            }
            elseif ($null -eq $state.installer) {
                $state.installer = Get-InstallerMetadata -Path $SelectedInstallerPath -Version $state.release_version
            }
            $state.smoke = Invoke-InternalGate -Name "nsis-install-smoke" -Action {
                Invoke-InstallerSmoke -RepositoryRoot $repositoryRoot -Installer $SelectedInstallerPath -KeepDirectory:$KeepSmoke
            }
        }

        $state.status = "pass"
    }
    catch {
        $state.status = "fail"
        $state.error = $_.Exception.Message
        throw
    }
    finally {
        $state.completed_at = [DateTimeOffset]::Now.ToString("o")
        $state.steps = @($script:AcceptanceStepResults)
        Write-AcceptanceResult -Path $SelectedResultsPath -Result $state
    }

    Write-Host "[acceptance] PASS ($SelectedMode)"
    Write-Host "[acceptance] result: $([System.IO.Path]::GetFullPath($SelectedResultsPath))"
}

if ($MyInvocation.InvocationName -ne ".") {
    try {
        Invoke-SystemAcceptanceMain -SelectedMode $Mode -SelectedInstallerPath $InstallerPath -SelectedResultsPath $ResultsPath -KeepSmoke:$KeepSmokeDirectory
    }
    catch {
        Write-Error $_
        exit 1
    }
}
