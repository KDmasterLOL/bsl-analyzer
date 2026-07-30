<#
.SYNOPSIS
    Parse every PowerShell script in scripts/ and fail on syntax errors.

.DESCRIPTION
    The release installer runs on machines the test suite never touches, so a syntax
    error in it would only surface at a user's prompt. Parsing is the part of that risk
    a Linux CI can actually check.
#>

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSCommandPath
$failed = $false

foreach ($script in Get-ChildItem -Path $root -Filter '*.ps1' -File | Sort-Object Name) {
    $errors = $null
    [void] [System.Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref] $null, [ref] $errors)

    if ($errors) {
        $failed = $true
        Write-Host "[FAIL] $($script.Name)"
        $errors | ForEach-Object { Write-Host "       $($_.ToString())" }
    } else {
        Write-Host "[OK] $($script.Name)"
    }
}

if ($failed) { exit 1 }
