# TERSE Uninstall Script for Windows
# Usage: .\uninstall.ps1
#    or: .\uninstall.ps1 -KeepData
#
# This script reverses the install.ps1 actions:
#   1. Deregisters the hook from ~/.claude/settings.json
#   2. Removes ~/.terse/bin/ from user PATH
#   3. Removes the ~/.terse/ directory (binary, config, logs)
#
# Use -KeepData to preserve config and log files in ~/.terse/

#Requires -Version 5.1

[CmdletBinding()]
param(
    [switch]$KeepData,
    [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$TERSE_HOME = Join-Path $env:USERPROFILE ".terse"
$BIN_DIR = Join-Path $TERSE_HOME "bin"
$CLAUDE_SETTINGS = Join-Path $env:USERPROFILE ".claude" "settings.json"

function Write-Step {
    param([string]$Message)
    Write-Host "[terse] " -ForegroundColor Cyan -NoNewline
    Write-Host $Message
}

function Write-Ok {
    param([string]$Message)
    Write-Host "  ✓ " -ForegroundColor Green -NoNewline
    Write-Host $Message
}

function Write-Warn {
    param([string]$Message)
    Write-Host "  ⚠ " -ForegroundColor Yellow -NoNewline
    Write-Host $Message
}

function Write-Err {
    param([string]$Message)
    Write-Host "  ✗ " -ForegroundColor Red -NoNewline
    Write-Host $Message
}

# ---------------------------------------------------------------------------
# Confirmation
# ---------------------------------------------------------------------------

Write-Step "Uninstalling TERSE (Token Efficiency through Refined Stream Engineering)"
Write-Host ""

if (-not $Force) {
    if ($KeepData) {
        Write-Host "  This will remove the terse binary and hook registration." -ForegroundColor White
        Write-Host "  Config and log files in $TERSE_HOME will be preserved." -ForegroundColor White
    } else {
        Write-Host "  This will remove ALL terse files including config and logs." -ForegroundColor White
        Write-Host "  Use -KeepData to preserve config and log files." -ForegroundColor White
    }
    Write-Host ""
    $confirm = Read-Host "  Continue? [y/N]"
    if ($confirm -notin @("y", "Y", "yes", "Yes")) {
        Write-Step "Uninstall cancelled."
        exit 0
    }
    Write-Host ""
}

# ---------------------------------------------------------------------------
# Step 1: Deregister Claude Code hook
# ---------------------------------------------------------------------------

Write-Step "Removing Claude Code hook..."

if (Test-Path $CLAUDE_SETTINGS) {
    try {
        $settings = Get-Content $CLAUDE_SETTINGS -Raw | ConvertFrom-Json

        $modified = $false
        if ($settings.PSObject.Properties.Name.Contains("hooks") -and
            $settings.hooks.PSObject.Properties.Name.Contains("PreToolUse")) {

            $original = $settings.hooks.PreToolUse
            $filtered = @($original | Where-Object {
                $dominated = $false
                # New matcher-based format
                if ($_.PSObject.Properties.Name.Contains("hooks")) {
                    $terseHooks = @($_.hooks | Where-Object { $_.command -like "*terse*hook*" })
                    if ($terseHooks.Count -gt 0) { $dominated = $true }
                }
                # Legacy flat format
                elseif ($_.command -like "*terse*hook*") {
                    $dominated = $true
                }
                -not $dominated
            })

            if ($filtered.Count -lt @($original).Count) {
                $settings.hooks.PreToolUse = $filtered
                $modified = $true
            }

            # Clean up empty structures
            if ($filtered.Count -eq 0) {
                $settings.hooks.PSObject.Properties.Remove("PreToolUse")
            }
            # Remove hooks object if empty
            $remainingHooks = @($settings.hooks.PSObject.Properties)
            if ($remainingHooks.Count -eq 0) {
                $settings.PSObject.Properties.Remove("hooks")
            }
        }

        if ($modified) {
            $settings | ConvertTo-Json -Depth 10 | Set-Content $CLAUDE_SETTINGS -Encoding UTF8
            Write-Ok "Hook removed from $CLAUDE_SETTINGS"
        } else {
            Write-Ok "No terse hook found in Claude settings"
        }
    } catch {
        Write-Warn "Could not modify $CLAUDE_SETTINGS automatically."
        Write-Warn "Manually remove the terse hook entry from the PreToolUse array."
    }
} else {
    Write-Ok "No Claude settings file found (nothing to remove)"
}

# ---------------------------------------------------------------------------
# Step 2: Remove from PATH
# ---------------------------------------------------------------------------

Write-Step "Removing from PATH..."

$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -like "*$BIN_DIR*") {
    # Split, filter, rejoin — handles both leading and trailing semicolons
    $parts = $userPath -split ";" | Where-Object { $_ -ne $BIN_DIR -and $_ -ne "" }
    $newPath = $parts -join ";"
    [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
    Write-Ok "Removed $BIN_DIR from user PATH"
    Write-Warn "Restart your terminal for PATH changes to take effect."
} else {
    Write-Ok "Not found in PATH (nothing to remove)"
}

# Also remove from current session
$env:PATH = ($env:PATH -split ";" | Where-Object { $_ -ne $BIN_DIR -and $_ -ne "" }) -join ";"

# ---------------------------------------------------------------------------
# Step 3: Remove files
# ---------------------------------------------------------------------------

if ($KeepData) {
    Write-Step "Removing binary (keeping config and data)..."

    if (Test-Path $BIN_DIR) {
        Remove-Item -Recurse -Force $BIN_DIR
        Write-Ok "Removed $BIN_DIR"
    } else {
        Write-Ok "Binary directory not found (already removed)"
    }

    Write-Ok "Preserved data in $TERSE_HOME"
    Write-Host "  Kept:" -ForegroundColor DarkGray
    if (Test-Path $TERSE_HOME) {
        Get-ChildItem $TERSE_HOME -File | ForEach-Object {
            Write-Host "    $_" -ForegroundColor DarkGray
        }
    }
} else {
    Write-Step "Removing all terse files..."

    if (Test-Path $TERSE_HOME) {
        Remove-Item -Recurse -Force $TERSE_HOME
        Write-Ok "Removed $TERSE_HOME"
    } else {
        Write-Ok "TERSE home directory not found (already removed)"
    }
}

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

Write-Host ""
Write-Step "Uninstall complete!"
Write-Host ""
if ($KeepData) {
    Write-Host "  Data preserved at: $TERSE_HOME" -ForegroundColor White
    Write-Host "  To fully remove:   Remove-Item -Recurse ~\.terse" -ForegroundColor White
} else {
    Write-Host "  All terse files have been removed." -ForegroundColor White
}
Write-Host ""
