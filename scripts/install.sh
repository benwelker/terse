#!/usr/bin/env bash
# terse Installation Script for macOS and Linux
# Usage: curl -fsSL https://raw.githubusercontent.com/benwelker/terse/master/install.sh | bash
#    or: ./install.sh
#
# This script:
#   1. Downloads the latest terse release binary for your platform
#   2. Places it in ~/.terse/bin/
#   3. Creates a default config at ~/.terse/config.toml
#   4. Checks for Ollama availability
#   5. Registers the hook in ~/.claude/settings.json

set -euo pipefail

REPO="benwelker/terse"
TERSE_HOME="$HOME/.terse"
BIN_DIR="$TERSE_HOME/bin"
BINARY="$BIN_DIR/terse"
CONFIG_FILE="$TERSE_HOME/config.toml"
CLAUDE_SETTINGS="$HOME/.claude/settings.json"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

step()  { printf '\033[36m[terse]\033[0m %s\n' "$1"; }
ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
warn()  { printf '  \033[33m⚠\033[0m %s\n' "$1"; }
err()   { printf '  \033[31m✗\033[0m %s\n' "$1"; }

# ---------------------------------------------------------------------------
# Step 1: Detect platform & determine download target
# ---------------------------------------------------------------------------

step "Installing terse"
echo ""

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        case "$ARCH" in
            x86_64)  TARGET="terse-linux-x86_64"  ;;
            aarch64) TARGET="terse-linux-aarch64"  ;;
            arm64)   TARGET="terse-linux-aarch64"  ;;
            *)
                err "Unsupported architecture: $ARCH"
                exit 1
                ;;
        esac
        ;;
    Darwin)
        case "$ARCH" in
            x86_64) TARGET="terse-macos-x86_64"  ;;
            arm64)  TARGET="terse-macos-aarch64"  ;;
            *)
                err "Unsupported architecture: $ARCH"
                exit 1
                ;;
        esac
        ;;
    *)
        err "Unsupported OS: $OS"
        err "Use install.ps1 for Windows."
        exit 1
        ;;
esac

# Determine download tool
if command -v curl &>/dev/null; then
    FETCH="curl -fsSL"
    FETCH_FILE="curl -fsSL -o"
elif command -v wget &>/dev/null; then
    FETCH="wget -qO-"
    FETCH_FILE="wget -qO"
else
    err "Neither curl nor wget found. Please install one."
    exit 1
fi

# ---------------------------------------------------------------------------
# Step 2: Get latest release URL & download
# ---------------------------------------------------------------------------

step "Finding latest release..."

RELEASE_JSON=$($FETCH "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null || true)

if [ -z "$RELEASE_JSON" ]; then
    warn "Could not fetch latest release from GitHub."
    warn "You can download manually from: https://github.com/$REPO/releases"
    exit 1
fi

# Parse download URL (portable grep-based, no jq dependency)
DOWNLOAD_URL=$(echo "$RELEASE_JSON" \
    | grep -o '"browser_download_url": *"[^"]*'"$TARGET"'[^"]*"' \
    | head -1 \
    | grep -o 'https://[^"]*')

VERSION=$(echo "$RELEASE_JSON" \
    | grep -o '"tag_name": *"[^"]*"' \
    | head -1 \
    | grep -o 'v[^"]*')

if [ -z "$DOWNLOAD_URL" ]; then
    err "No release asset found for $TARGET"
    warn "Available at: https://github.com/$REPO/releases"
    exit 1
fi

ok "Found release $VERSION"

# ---------------------------------------------------------------------------
# Step 3: Download and install binary
# ---------------------------------------------------------------------------

step "Downloading $TARGET..."

mkdir -p "$BIN_DIR"
TEMP_TAR="$(mktemp)"

$FETCH_FILE "$TEMP_TAR" "$DOWNLOAD_URL"
ok "Downloaded successfully"

step "Installing to $BIN_DIR..."

tar xzf "$TEMP_TAR" -C "$BIN_DIR"
rm -f "$TEMP_TAR"
chmod +x "$BINARY"

ok "Installed terse to $BIN_DIR"

# Verify binary works
if "$BINARY" --help &>/dev/null; then
    ok "Binary verified"
else
    err "Binary verification failed. The download may be corrupt."
    exit 1
fi

# ---------------------------------------------------------------------------
# Step 4: Add to PATH
# ---------------------------------------------------------------------------

step "Checking PATH..."

if echo "$PATH" | tr ':' '\n' | grep -qx "$BIN_DIR"; then
    ok "Already in PATH"
else
    # Detect shell and add to profile
    SHELL_NAME="$(basename "$SHELL" 2>/dev/null || echo "bash")"
    case "$SHELL_NAME" in
        zsh)  PROFILE="$HOME/.zshrc" ;;
        fish) PROFILE="$HOME/.config/fish/config.fish" ;;
        *)    PROFILE="$HOME/.bashrc" ;;
    esac

    if [ "$SHELL_NAME" = "fish" ]; then
        EXPORT_LINE="set -gx PATH \$PATH $BIN_DIR"
    else
        EXPORT_LINE="export PATH=\"\$PATH:$BIN_DIR\""
    fi

    if [ -f "$PROFILE" ] && grep -q "$BIN_DIR" "$PROFILE" 2>/dev/null; then
        ok "PATH entry already in $PROFILE"
    else
        echo "" >> "$PROFILE"
        echo "# terse" >> "$PROFILE"
        echo "$EXPORT_LINE" >> "$PROFILE"
        ok "Added $BIN_DIR to $PROFILE"
        warn "Restart your terminal or run: source $PROFILE"
    fi

    # Make available in current session
    export PATH="$PATH:$BIN_DIR"
fi

# ---------------------------------------------------------------------------
# Step 5: Create default configuration
# ---------------------------------------------------------------------------

step "Setting up configuration..."

if [ ! -f "$CONFIG_FILE" ]; then
    if "$BINARY" config init 2>/dev/null; then
        ok "Created default config at $CONFIG_FILE"
    else
        warn "Could not create config. Run 'terse config init' manually."
    fi
else
    ok "Config already exists at $CONFIG_FILE"
fi

# ---------------------------------------------------------------------------
# Step 6: Check Ollama
# ---------------------------------------------------------------------------

step "Checking Ollama (optional, for Smart Path)..."

if command -v ollama &>/dev/null; then
    OLLAMA_VER=$(ollama --version 2>&1 || echo "unknown")
    ok "Ollama found: $OLLAMA_VER"

    MODELS=$(ollama list 2>&1 || echo "")
    if echo "$MODELS" | grep -q "llama3"; then
        ok "Model available (llama3 family detected)"
    else
        warn "No llama3 model found. For Smart Path, run:"
        echo "    ollama pull llama3.2:1b"
    fi
else
    warn "Ollama not found. Smart Path will be disabled."
    warn "Install Ollama from https://ollama.com for LLM-powered optimization."
    warn "terse works fine without it — rule-based optimizers are always available."
fi

# ---------------------------------------------------------------------------
# Step 7: Register Claude Code hook
# ---------------------------------------------------------------------------

step "Registering Claude Code hook..."

CLAUDE_DIR="$(dirname "$CLAUDE_SETTINGS")"
mkdir -p "$CLAUDE_DIR"

HOOK_CMD="$BINARY hook"

if [ -f "$CLAUDE_SETTINGS" ]; then
    # Check if hook already registered
    if grep -q "terse.*hook" "$CLAUDE_SETTINGS" 2>/dev/null; then
        ok "Hook already registered in Claude settings"
    else
        # Try to add hook using Python (commonly available) or inform user
        if command -v python3 &>/dev/null; then
            python3 -c "
import json, sys
try:
    with open('$CLAUDE_SETTINGS', 'r') as f:
        settings = json.load(f)
except:
    settings = {}
hooks = settings.setdefault('hooks', {})
pre = hooks.setdefault('PreToolUse', [])
# Matcher-based format: 'Bash' matches the BashTool
entry = {
    'matcher': 'Bash',
    'hooks': [{'type': 'command', 'command': '$HOOK_CMD'}]
}
pre.append(entry)
with open('$CLAUDE_SETTINGS', 'w') as f:
    json.dump(settings, f, indent=2)
print('ok')
" && ok "Hook registered in $CLAUDE_SETTINGS" || {
                warn "Could not register hook automatically."
            }
        else
            warn "Could not register hook automatically (python3 not found)."
        fi
    fi
else
    # Create new settings file
    if command -v python3 &>/dev/null; then
        python3 -c "
import json
entry = {
    'matcher': 'Bash',
    'hooks': [{'type': 'command', 'command': '$HOOK_CMD'}]
}
settings = {'hooks': {'PreToolUse': [entry]}}
with open('$CLAUDE_SETTINGS', 'w') as f:
    json.dump(settings, f, indent=2)
print('ok')
" && ok "Hook registered in $CLAUDE_SETTINGS" || {
            warn "Could not create Claude settings."
        }
    else
        warn "Could not register hook (python3 not found)."
    fi
fi

# If hook registration failed, show manual instructions
if [ ! -f "$CLAUDE_SETTINGS" ] || ! grep -q "terse.*hook" "$CLAUDE_SETTINGS" 2>/dev/null; then
    warn "Add this to $CLAUDE_SETTINGS manually:"
    cat <<HOOKEOF
    {
      "hooks": {
        "PreToolUse": [
          {
            "matcher": "Bash",
            "hooks": [
              { "type": "command", "command": "$HOOK_CMD" }
            ]
          }
        ]
      }
    }
HOOKEOF
fi

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

echo ""
step "Installation complete!"
echo ""
echo "  Binary:  $BINARY"
echo "  Config:  $CONFIG_FILE"
echo "  Hook:    $CLAUDE_SETTINGS"
echo ""
echo "  Quick start:"
echo "    terse health            — verify installation"
echo "    terse stats             — view token savings"
echo "    terse test \"git status\" — preview optimization"
echo ""
