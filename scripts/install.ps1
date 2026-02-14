# TERSE Installation Script for Windows
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

Write-Step "Installing TERSE (Token Efficiency through Refined Stream Engineering)"
Write-Host ""

# Detect architecture
$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($arch -eq "X64") {
    $target = "terse-windows-x86_64"
} else {
    Write-Err "Unsupported architecture: $arch"
    Write-Err "TERSE currently supports x86_64 on Windows."
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
    Write-Warn "TERSE works fine without it — rule-based optimizers are always available."
}

# ---------------------------------------------------------------------------
# Step 6: Register Claude Code hook
# ---------------------------------------------------------------------------

Write-Step "Registering Claude Code hook..."

$claudeDir = Split-Path $CLAUDE_SETTINGS
if (-not (Test-Path $claudeDir)) {
    New-Item -ItemType Directory -Force -Path $claudeDir | Out-Null
}

$hookCommand = "$BINARY hook"

try {
    if (Test-Path $CLAUDE_SETTINGS) {
        $settings = Get-Content $CLAUDE_SETTINGS -Raw | ConvertFrom-Json
    } else {
        $settings = [PSCustomObject]@{}
    }

    # Ensure hooks structure exists
    if (-not $settings.PSObject.Properties.Name.Contains("hooks")) {
        $settings | Add-Member -NotePropertyName "hooks" -NotePropertyValue ([PSCustomObject]@{})
    }
    if (-not $settings.hooks.PSObject.Properties.Name.Contains("PreToolUse")) {
        $settings.hooks | Add-Member -NotePropertyName "PreToolUse" -NotePropertyValue @()
    }

    # Check if hook already registered
    $existing = $settings.hooks.PreToolUse | Where-Object {
        $_.command -like "*terse*hook*"
    }

    if ($existing) {
        Write-Ok "Hook already registered in Claude settings"
    } else {
        $hook = [PSCustomObject]@{
            type    = "command"
            command = $hookCommand
        }
        $settings.hooks.PreToolUse += $hook
        $settings | ConvertTo-Json -Depth 10 | Set-Content $CLAUDE_SETTINGS -Encoding UTF8
        Write-Ok "Hook registered in $CLAUDE_SETTINGS"
    }
} catch {
    Write-Warn "Could not register hook automatically."
    Write-Warn "Add this to $CLAUDE_SETTINGS manually:"
    Write-Host @"
    {
      "hooks": {
        "PreToolUse": [
          { "type": "command", "command": "$hookCommand" }
        ]
      }
    }
"@ -ForegroundColor DarkYellow
}

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

Write-Host ""
Write-Step "Installation complete!"
Write-Host ""
Write-Host "  Binary:  $BINARY" -ForegroundColor White
Write-Host "  Config:  $CONFIG_FILE" -ForegroundColor White
Write-Host "  Hook:    $CLAUDE_SETTINGS" -ForegroundColor White
Write-Host ""
Write-Host "  Quick start:" -ForegroundColor Cyan
Write-Host "    terse health       — verify installation"
Write-Host "    terse stats        — view token savings"
Write-Host "    terse test ""git status"" — preview optimization"
Write-Host ""
