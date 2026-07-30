<#
.SYNOPSIS
    Install the bsl-analyzer LSP/MCP server on Windows.

.DESCRIPTION
    Windows counterpart of install.sh. Downloads the release binary from the release
    server, verifies it against the release manifest, installs it per user and puts the
    install directory on PATH.

    Messages are ASCII so the file needs no BOM: Windows PowerShell 5.1 reads a
    BOM-less file as ANSI and would mangle anything else.

.EXAMPLE
    irm https://dev.runsystems.ru/releases/static/install.ps1 | iex

.EXAMPLE
    & ([scriptblock]::Create((irm https://dev.runsystems.ru/releases/static/install.ps1))) -Version 0.2.66
#>

[CmdletBinding()]
param(
    [string] $Version = $env:BSL_VERSION,
    [string] $InstallDir = $env:BSL_INSTALL_DIR,
    [switch] $NoPathUpdate,
    [switch] $Help
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Invoke-WebRequest spends most of its time redrawing the progress bar otherwise.
$ProgressPreference = 'SilentlyContinue'

# Rewritten to "github" by scripts/github-sync.sh, exactly as INSTALL_SOURCE in install.sh:
# the public mirror cannot reach the internal release server.
$InstallSource = 'gitlab'
$ReleaseUrl = 'https://dev.runsystems.ru/releases'
$Product = 'bsl-analyzer'
$GitHubRepo = 'itrous/bsl-analyzer'
# The launcher, not the app: it keeps the working binary up to date by itself, and
# docs/mcp/SETUP.md makes it the single entry point that belongs on PATH.
$AssetName = 'bsl-analyzer-windows-amd64.exe'
$BinaryName = 'bsl-analyzer.exe'

function Write-Info { param([string] $Message) Write-Host "[INFO] $Message" -ForegroundColor Blue }
function Write-Ok { param([string] $Message) Write-Host "[OK] $Message" -ForegroundColor Green }
function Write-Warn { param([string] $Message) Write-Host "[WARN] $Message" -ForegroundColor Yellow }

function Show-Usage {
    Write-Host @'
Install bsl-analyzer LSP server

Usage: install.ps1 [OPTIONS]

Options:
  -Version <VERSION>    Version to install (default: latest)
  -InstallDir <DIR>     Installation directory
  -NoPathUpdate         Do not add the installation directory to the user PATH
  -Help                 Show this help

Environment variables:
  BSL_INSTALL_DIR       Installation directory
  BSL_VERSION           Version to install
'@
}

function Assert-Supported {
    if ($PSVersionTable.PSVersion.Major -lt 5) {
        throw "PowerShell 5.0 or newer is required, found $($PSVersionTable.PSVersion)"
    }

    # WOW64 reports the 32-bit view in PROCESSOR_ARCHITECTURE, the real one in the W6432 twin.
    $architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    if ($architecture -ne 'AMD64') {
        throw "Unsupported architecture: $architecture. Only windows-amd64 builds are published."
    }
}

function Enable-Tls12 {
    # Windows PowerShell 5.1 on older systems still defaults to TLS 1.0, which the
    # release server rejects. On PowerShell 7 the property is a no-op.
    try {
        [Net.ServicePointManager]::SecurityProtocol = `
            [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch {
        Write-Warn "Cannot raise the TLS version: $($_.Exception.Message)"
    }
}

function Invoke-GitHubApi {
    param([string] $Path)

    # GitHub rejects requests without a User-Agent.
    return Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$GitHubRepo/$Path" `
        -Headers @{ 'User-Agent' = 'bsl-analyzer-installer'; 'Accept' = 'application/vnd.github+json' }
}

function Get-LatestVersion {
    if ($InstallSource -eq 'github') {
        $release = Invoke-GitHubApi -Path 'releases/latest'
        $latest = ([string] $release.tag_name).TrimStart('v').Trim()
    } else {
        $response = Invoke-WebRequest -UseBasicParsing -Uri "$ReleaseUrl/$Product/latest"
        $latest = $response.Content.Trim()
    }

    if (-not $latest) {
        throw 'Failed to determine latest version'
    }
    return $latest
}

function Get-AssetInfo {
    param([string] $ReleaseVersion)

    if ($InstallSource -eq 'github') {
        return Get-GitHubAssetInfo -ReleaseVersion $ReleaseVersion
    }

    $manifestUrl = "$ReleaseUrl/$Product/$ReleaseVersion/manifest.json"
    try {
        $manifest = Invoke-RestMethod -UseBasicParsing -Uri $manifestUrl
    } catch {
        throw "Failed to fetch release manifest from ${manifestUrl}: $($_.Exception.Message)"
    }

    # Asset names carry dots, so go through the property bag rather than member access.
    $entry = $manifest.files.PSObject.Properties[$AssetName]
    if (-not $entry) {
        throw "Release $ReleaseVersion does not contain $AssetName"
    }

    return [pscustomobject]@{
        sha256 = $entry.Value.sha256
        size   = $entry.Value.size
        url    = "$ReleaseUrl/$Product/$ReleaseVersion/$AssetName"
    }
}

function Get-GitHubAssetInfo {
    param([string] $ReleaseVersion)

    $release = Invoke-GitHubApi -Path "releases/tags/v$ReleaseVersion"
    $releaseAsset = $release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1
    if (-not $releaseAsset) {
        throw "Release $ReleaseVersion does not contain $AssetName"
    }

    # GitHub releases carry no manifest; checksums.txt is the counterpart the launcher
    # reads too.
    $checksums = $release.assets | Where-Object { $_.name -eq 'checksums.txt' } | Select-Object -First 1
    if (-not $checksums) {
        throw "Release $ReleaseVersion publishes no checksums.txt, refusing to install unverified"
    }

    $content = (Invoke-WebRequest -UseBasicParsing -Uri $checksums.browser_download_url).Content
    # Served as application/octet-stream, so Content is a byte array, not a string.
    $text = if ($content -is [byte[]]) { [Text.Encoding]::UTF8.GetString($content) } else { [string] $content }

    $sha256 = ''
    foreach ($line in ($text -split "`n")) {
        $parts = $line.Trim() -split '\s+', 2
        if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $AssetName) {
            $sha256 = $parts[0]
            break
        }
    }

    if (-not $sha256) {
        throw "checksums.txt of release $ReleaseVersion has no entry for $AssetName"
    }

    return [pscustomobject]@{
        sha256 = $sha256
        size   = $releaseAsset.size
        url    = $releaseAsset.browser_download_url
    }
}

function Get-InstalledVersion {
    param([string] $Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        return $null
    }

    # --launcher-version answers from the launcher itself; plain --version would make it
    # fetch the whole app binary just to report a number.
    try {
        $output = & $Path --launcher-version 2>$null
    } catch {
        return $null
    }

    # The whole trailing token, so a prerelease suffix survives: matching only three
    # numeric parts would read 0.3.0-beta.1 as 0.3.0 and reinstall it on every run.
    $reported = ($output -join ' ') -split '\s+' | Where-Object { $_ } | Select-Object -Last 1
    if ($reported -match '^\d+\.\d+\.\d+') { return $reported }
    return $null
}

function Install-Binary {
    param([string] $Source, [string] $Target)

    # A running LSP or MCP server holds the target open: Windows refuses to overwrite it
    # but does allow renaming it away, so the new binary can take its place immediately.
    $retired = $null
    if (Test-Path -LiteralPath $Target) {
        $retired = "$Target.old-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
        try {
            Move-Item -LiteralPath $Target -Destination $retired -Force
        } catch {
            throw "Cannot replace $Target - stop running bsl-analyzer processes and retry: $($_.Exception.Message)"
        }
    }

    # The renamed copy is the only working install until the new file lands, so a failed
    # move has to put it back instead of leaving the machine with no binary at all.
    try {
        Move-Item -LiteralPath $Source -Destination $Target -Force
    } catch {
        $failure = $_
        if ($retired) {
            try {
                Move-Item -LiteralPath $retired -Destination $Target -Force
                Write-Warn 'Install failed, restored the previous binary'
            } catch {
                Write-Warn "Install failed and the previous binary could not be restored, it is left as $retired"
            }
        }
        throw $failure
    }

    if ($retired -and (Test-Path -LiteralPath $retired)) {
        try {
            Remove-Item -LiteralPath $retired -Force
        } catch {
            Write-Warn "Previous binary is still in use, left as $retired"
        }
    }
}

function Get-UserPath {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $false)
    if (-not $key) { return @{ Value = ''; Kind = [Microsoft.Win32.RegistryValueKind]::ExpandString } }

    try {
        # Unexpanded, otherwise entries such as %USERPROFILE%\bin would be written back
        # as the literal path they happen to expand to today.
        $value = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $kind = $key.GetValueKind('Path')
        return @{ Value = [string]$value; Kind = $kind }
    } catch {
        return @{ Value = ''; Kind = [Microsoft.Win32.RegistryValueKind]::ExpandString }
    } finally {
        $key.Close()
    }
}

function Publish-EnvironmentChange {
    # Without the broadcast, processes started from the current Explorer session keep the
    # stale environment block until the next logon.
    try {
        if (-not ('BslEnv.Native' -as [type])) {
            Add-Type -Namespace 'BslEnv' -Name 'Native' -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Auto)]
public static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint Msg, System.IntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out System.UIntPtr lpdwResult);
'@
        }

        $HWND_BROADCAST = [IntPtr] 0xffff
        $WM_SETTINGCHANGE = 0x1a
        $SMTO_ABORTIFHUNG = 0x2
        $result = [UIntPtr]::Zero
        [void] [BslEnv.Native]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [IntPtr]::Zero, 'Environment', $SMTO_ABORTIFHUNG, 5000, [ref] $result)
    } catch {
        Write-Warn "Cannot broadcast the environment change: $($_.Exception.Message)"
    }
}

function Add-ToUserPath {
    param([string] $Directory)

    $current = Get-UserPath
    $entries = $current.Value -split ';' | Where-Object { $_ }
    # The stored value is deliberately unexpanded, so an entry written as
    # %LOCALAPPDATA%\... names the same directory as the absolute path and must not be
    # appended a second time. Paths themselves compare case-insensitively on Windows.
    $normalized = [Environment]::ExpandEnvironmentVariables($Directory).TrimEnd('\')
    if ($entries | Where-Object { [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') -ieq $normalized }) {
        return $false
    }

    $updated = if ($current.Value) { "$($current.Value.TrimEnd(';'));$Directory" } else { $Directory }

    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    if (-not $key) {
        throw 'Cannot open HKCU\Environment to update PATH'
    }
    try {
        # SetValue keeps the original value kind: rewriting an ExpandString PATH as a
        # plain String would freeze every %VAR% already in it.
        $kind = if ($current.Kind -eq [Microsoft.Win32.RegistryValueKind]::String) {
            [Microsoft.Win32.RegistryValueKind]::String
        } else {
            [Microsoft.Win32.RegistryValueKind]::ExpandString
        }
        $key.SetValue('Path', $updated, $kind)
    } finally {
        $key.Close()
    }

    Publish-EnvironmentChange
    $env:Path = "$env:Path;$Directory"
    return $true
}

function Update-Path {
    param([string] $Directory)

    if ($NoPathUpdate) {
        Write-Info "PATH left untouched, run the binary as $(Join-Path $Directory $BinaryName)"
        return
    }

    if (Add-ToUserPath -Directory $Directory) {
        Write-Ok "Added $Directory to the user PATH, restart open terminals to pick it up"
    }
}

function Invoke-Install {
    if ($Help) {
        Show-Usage
        return
    }

    Write-Host ''
    Write-Host '  bsl-analyzer installer'
    Write-Host ''

    Assert-Supported
    Enable-Tls12

    if (-not $InstallDir) {
        $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\bsl-analyzer'
    }

    # PATH is stored once and read later from other working directories, so a relative
    # -InstallDir has to be resolved before it is written anywhere.
    $InstallDir = [IO.Path]::GetFullPath([IO.Path]::Combine((Get-Location).ProviderPath, $InstallDir))

    Write-Info 'Platform: windows-amd64'
    Write-Info "Install dir: $InstallDir"

    if (-not $Version) {
        Write-Info 'Fetching latest version...'
        $Version = Get-LatestVersion
    }
    Write-Info "Version: $Version"

    $target = Join-Path $InstallDir $BinaryName
    $installed = Get-InstalledVersion -Path $target
    # -ceq: SemVer prerelease identifiers and release tags are case-sensitive, while the
    # ordinary PowerShell operators are not, so 0.3.0-RC.1 would pass for 0.3.0-rc.1.
    if ($installed -ceq $Version) {
        Write-Ok "bsl-analyzer $Version is already installed"
        # A rerun still has to finish what an interrupted or -NoPathUpdate run left undone.
        Update-Path -Directory $InstallDir
        return
    }
    if ($installed) {
        Write-Info "Upgrading from $installed to $Version"
    }

    $info = Get-AssetInfo -ReleaseVersion $Version

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

    # An upgrade performed while the server was running had to leave the old binary
    # behind; sweep those once nothing holds them any more, or they pile up per upgrade.
    Get-ChildItem -Path $InstallDir -Filter "$BinaryName.old-*" -File -ErrorAction SilentlyContinue |
        ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue }

    # Staged inside the install directory so the final move stays on one volume.
    $staged = "$target.new-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
    try {
        Write-Info "Downloading $AssetName ($([math]::Round($info.size / 1MB, 1)) MiB)..."
        Invoke-WebRequest -UseBasicParsing -Uri $info.url -OutFile $staged

        # Unconditional: both sources are contracted to yield a checksum, so an empty one
        # is a defect to surface rather than a reason to install something unverified.
        Write-Info 'Verifying checksum...'
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $staged).Hash
        if ($actual -ine $info.sha256) {
            throw "Checksum mismatch! Expected: $($info.sha256) Got: $actual"
        }
        Write-Ok 'Checksum verified'

        # Clears the mark-of-the-web SmartScreen would otherwise prompt on.
        Unblock-File -LiteralPath $staged

        Install-Binary -Source $staged -Target $target
    } finally {
        if (Test-Path -LiteralPath $staged) {
            Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
        }
    }

    Write-Ok "Installed bsl-analyzer $Version to $target"

    Update-Path -Directory $InstallDir

    $confirmed = Get-InstalledVersion -Path $target
    if ($confirmed) {
        Write-Ok "bsl-analyzer-launcher $confirmed"
    }

    Write-Info 'The analyzer itself is fetched on first run; force it now with: bsl-analyzer --launcher-update'
    Write-Host ''
}

Invoke-Install
