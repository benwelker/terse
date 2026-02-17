# terse Installation Script for Windows
# Usage: irm https://raw.githubusercontent.com/benwelker/terse/master/install.ps1 | iex
#    or: .\install.ps1
#
# This script:
#   1. Downloads the latest terse.exe release binary
#   2. Places it in ~/.terse/bin/
#   3. Creates a default config at ~/.terse/config.toml
#   4. Checks for Ollama availability
#   5. Registers the hook in ~/.claude/settings.json

#Requires -Version 5.1

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$REPO = "benwelker/terse"
$TERSE_HOME = Join-Path $env:USERPROFILE ".terse"
$BIN_DIR = Join-Path $TERSE_HOME "bin"
$BINARY = Join-Path $BIN_DIR "terse.exe"
$CONFIG_FILE = Join-Path $TERSE_HOME "config.toml"
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
# Step 1: Detect platform & determine download URL
# ---------------------------------------------------------------------------

Write-Step "Installing terse"
Write-Host ""

# Detect architecture
$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($arch -eq "X64") {
    $target = "terse-windows-x86_64"
} else {
    Write-Err "Unsupported architecture: $arch"
    Write-Err "terse currently supports x86_64 on Windows."
    exit 1
}

# Get latest release URL
Write-Step "Finding latest release..."
try {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$REPO/releases/latest" -UseBasicParsing
    $asset = $release.assets | Where-Object { $_.name -like "$target*" } | Select-Object -First 1
    if (-not $asset) {
        throw "No matching asset found for $target"
    }
    $downloadUrl = $asset.browser_download_url
    $version = $release.tag_name
    Write-Ok "Found release $version"
} catch {
    Write-Warn "Could not fetch latest release from GitHub."
    Write-Warn "You can download manually from: https://github.com/$REPO/releases"
    exit 1
}

# ---------------------------------------------------------------------------
# Step 2: Download and install binary
# ---------------------------------------------------------------------------

Write-Step "Downloading $target..."

# Create directories
New-Item -ItemType Directory -Force -Path $BIN_DIR | Out-Null

$tempZip = Join-Path $env:TEMP "terse-download.zip"
try {
    Invoke-WebRequest -Uri $downloadUrl -OutFile $tempZip -UseBasicParsing
    Write-Ok "Downloaded successfully"
} catch {
    Write-Err "Download failed: $_"
    exit 1
}

# Extract
Write-Step "Installing to $BIN_DIR..."
try {
    Expand-Archive -Path $tempZip -DestinationPath $BIN_DIR -Force
    Remove-Item $tempZip -Force
    Write-Ok "Installed terse.exe to $BIN_DIR"
} catch {
    Write-Err "Extraction failed: $_"
    exit 1
}

# Verify binary works
try {
    $versionOut = & $BINARY --help 2>&1 | Select-Object -First 1
    Write-Ok "Binary verified: $versionOut"
} catch {
    Write-Err "Binary verification failed. The download may be corrupt."
    exit 1
}

# ---------------------------------------------------------------------------
# Step 3: Add to PATH (user-level)
# ---------------------------------------------------------------------------

Write-Step "Checking PATH..."

$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$BIN_DIR*") {
    [Environment]::SetEnvironmentVariable("PATH", "$userPath;$BIN_DIR", "User")
    $env:PATH = "$env:PATH;$BIN_DIR"
    Write-Ok "Added $BIN_DIR to user PATH"
    Write-Warn "Restart your terminal for PATH changes to take effect."
} else {
    Write-Ok "Already in PATH"
}

# ---------------------------------------------------------------------------
# Step 4: Create default configuration
# ---------------------------------------------------------------------------

Write-Step "Setting up configuration..."

if (-not (Test-Path $CONFIG_FILE)) {
    try {
        & $BINARY config init 2>&1 | Out-Null
        Write-Ok "Created default config at $CONFIG_FILE"
    } catch {
        Write-Warn "Could not create config file. Run 'terse config init' manually."
    }
} else {
    Write-Ok "Config already exists at $CONFIG_FILE"
}

# ---------------------------------------------------------------------------
# Step 5: Check Ollama
# ---------------------------------------------------------------------------

Write-Step "Checking Ollama (optional, for Smart Path)..."

$ollamaAvailable = $false
if (Get-Command ollama -ErrorAction SilentlyContinue) {
    $ollamaVersion = & ollama --version 2>&1
    Write-Ok "Ollama found: $ollamaVersion"
    $ollamaAvailable = $true

    $models = & ollama list 2>&1
    if ($models -match "llama3") {
        Write-Ok "Model available (llama3 family detected)"
    } else {
        Write-Warn "No llama3 model found. For Smart Path, run:"
        Write-Host "    ollama pull llama3.2:1b" -ForegroundColor DarkYellow
    }
} else {
    Write-Warn "Ollama not found. Smart Path will be disabled."
    Write-Warn "Install Ollama from https://ollama.com for LLM-powered optimization."
    Write-Warn "terse works fine without it — rule-based optimizers are always available."
}

# ---------------------------------------------------------------------------
# Step 6: Register Claude Code hook
# ---------------------------------------------------------------------------

Write-Step "Registering Claude Code hook..."

$claudeDir = Split-Path $CLAUDE_SETTINGS
if (-not (Test-Path $claudeDir)) {
    New-Item -ItemType Directory -Force -Path $claudeDir | Out-Null
}

# Quote the binary path to handle spaces in user profile paths (e.g. "John Smith")
$hookCommand = "`"$BINARY`" hook"

try {
    if (Test-Path $CLAUDE_SETTINGS) {
        $settings = Get-Content $CLAUDE_SETTINGS -Raw | ConvertFrom-Json
    } else {
        $settings = [PSCustomObject]@{}
    }

    # Ensure hooks structure exists
    $settingsProps = @($settings.PSObject.Properties | ForEach-Object { $_.Name })
    if (-not ($settingsProps -contains "hooks")) {
        $settings | Add-Member -NotePropertyName "hooks" -NotePropertyValue ([PSCustomObject]@{})
    } elseif ($null -eq $settings.hooks) {
        $settings.hooks = [PSCustomObject]@{}
    }
    if ($settings.hooks -is [hashtable]) {
        $settings.hooks = [PSCustomObject]$settings.hooks
    }

    $hookProps = @($settings.hooks.PSObject.Properties | ForEach-Object { $_.Name })
    if (-not ($hookProps -contains "PreToolUse")) {
        $settings.hooks | Add-Member -NotePropertyName "PreToolUse" -NotePropertyValue @()
    } elseif ($null -eq $settings.hooks.PreToolUse) {
        $settings.hooks.PreToolUse = @()
    } else {
        $settings.hooks.PreToolUse = @($settings.hooks.PreToolUse)
    }

    # Check if hook already registered (handle both old and new formats)
    $existing = @($settings.hooks.PreToolUse | Where-Object {
        $entryProps = @($_.PSObject.Properties | ForEach-Object { $_.Name })
        # New matcher-based format
        if ($entryProps -contains "hooks") {
            $_.hooks | Where-Object { $_.command -like "*terse*hook*" }
        }
        # Legacy flat format
        elseif ($_.command -like "*terse*hook*") {
            $true
        }
    })

    if ($existing.Count -gt 0) {
        Write-Ok "Hook already registered in Claude settings"
    } else {
        $hook = [PSCustomObject]@{
            matcher = "Bash"
            hooks   = @(
                [PSCustomObject]@{
                    type    = "command"
                    command = $hookCommand
                }
            )
        }
        $settings.hooks.PreToolUse = @($settings.hooks.PreToolUse) + @($hook)
        $json = $settings | ConvertTo-Json -Depth 10
        [System.IO.File]::WriteAllText($CLAUDE_SETTINGS, $json)
        Write-Ok "Hook registered in $CLAUDE_SETTINGS"
    }
} catch {
    Write-Warn "Could not register hook automatically."
    Write-Warn "Add this to $CLAUDE_SETTINGS manually:"
    Write-Host @"
    {
      "hooks": {
        "PreToolUse": [
          {
            "matcher": "Bash",
            "hooks": [
              { "type": "command", "command": "$hookCommand" }
            ]
          }
        ]
      }
    }
"@ -ForegroundColor DarkYellow
}

# ---------------------------------------------------------------------------
# Step 7: Create Copilot hook template & offer per-repo install
# ---------------------------------------------------------------------------

# Write-Step "Setting up GitHub Copilot hook..."

# $COPILOT_TEMPLATE = Join-Path $TERSE_HOME "copilot-hooks.json"
# $copilotBashCmd = "$BINARY copilot-hook"
# $copilotPsCmd = "& `"$BINARY`" copilot-hook"

# # Always create/update the template at ~/.terse/copilot-hooks.json
# $copilotHooksJson = @"
# {
#   "version": 1,
#   "hooks": {
#     "preToolUse": [
#       {
#         "type": "command",
#         "bash": "$($BINARY -replace '\\', '/') copilot-hook",
#         "powershell": "& \`"$BINARY\`" copilot-hook",
#         "timeoutSec": 30
#       }
#     ]
#   }
# }
# "@
# [System.IO.File]::WriteAllText($COPILOT_TEMPLATE, $copilotHooksJson)
# Write-Ok "Created Copilot hook template at $COPILOT_TEMPLATE"

# # Check if we're in a git repo and offer to install hooks there
# $gitRoot = $null
# try {
#     $gitRoot = & git rev-parse --show-toplevel 2>$null
# } catch { }

# if ($gitRoot) {
#     $hooksDir = Join-Path $gitRoot ".github" "hooks"
#     $hooksFile = Join-Path $hooksDir "terse.json"

#     if (Test-Path $hooksFile) {
#         if (Select-String -Path $hooksFile -Pattern "terse" -Quiet) {
#             Write-Ok "Copilot hook already registered in $hooksFile"
#         } else {
#             # File exists but no terse entry — merge in our hook
#             try {
#                 $existingJson = Get-Content $hooksFile -Raw | ConvertFrom-Json
#                 # Ensure preToolUse array exists
#                 $existingProps = @($existingJson.PSObject.Properties | ForEach-Object { $_.Name })
#                 $hooksProps = if ($existingProps -contains "hooks") {
#                     @($existingJson.hooks.PSObject.Properties | ForEach-Object { $_.Name })
#                 } else { @() }

#                 if (-not ($existingProps -contains "hooks")) {
#                     $existingJson | Add-Member -NotePropertyName "hooks" -NotePropertyValue ([PSCustomObject]@{})
#                 }
#                 if (-not ($hooksProps -contains "preToolUse")) {
#                     $existingJson.hooks | Add-Member -NotePropertyName "preToolUse" -NotePropertyValue @()
#                 }

#                 $newEntry = [PSCustomObject]@{
#                     type       = "command"
#                     bash       = "$($BINARY -replace '\\', '/') copilot-hook"
#                     powershell = "& `"$BINARY`" copilot-hook"
#                     timeoutSec = 30
#                 }
#                 $existingJson.hooks.preToolUse = @($existingJson.hooks.preToolUse) + @($newEntry)
#                 $mergedJson = $existingJson | ConvertTo-Json -Depth 10
#                 [System.IO.File]::WriteAllText($hooksFile, $mergedJson)
#                 Write-Ok "Added terse hook to $hooksFile"
#             } catch {
#                 Write-Warn "Could not update $hooksFile automatically."
#                 Write-Warn "Copy $COPILOT_TEMPLATE to $hooksDir manually."
#             }
#         }
#     } else {
#         # Create new hooks file
#         New-Item -ItemType Directory -Force -Path $hooksDir | Out-Null
#         Copy-Item $COPILOT_TEMPLATE $hooksFile
#         Write-Ok "Created Copilot hook at $hooksFile"
#     }
#     Write-Warn "Commit .github/hooks/terse.json to your repo's default branch for Copilot coding agent."
# } else {
#     Write-Warn "Not in a git repo — skipping per-repo Copilot hook install."
#     Write-Warn "To add Copilot hooks to a repo, copy the template:"
#     Write-Host "    copy `"$COPILOT_TEMPLATE`" <repo>\.github\hooks\terse.json" -ForegroundColor DarkYellow
# }

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

Write-Host ""
Write-Step "Installation complete!"
Write-Host ""
Write-Host "  Binary:  $BINARY" -ForegroundColor White
Write-Host "  Config:  $CONFIG_FILE" -ForegroundColor White
Write-Host "  Claude:  $CLAUDE_SETTINGS" -ForegroundColor White
Write-Host "  Copilot: $COPILOT_TEMPLATE (template)" -ForegroundColor White
Write-Host ""
Write-Host "  Quick start:" -ForegroundColor Cyan
Write-Host "    terse health       — verify installation"
Write-Host "    terse stats        — view token savings"
Write-Host "    terse test ""git status"" — preview optimization"
Write-Host ""
