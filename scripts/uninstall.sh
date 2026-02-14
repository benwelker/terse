#!/usr/bin/env bash
# TERSE Uninstall Script for macOS and Linux
# Usage: curl -fsSL https://raw.githubusercontent.com/benwelker/terse/master/uninstall.sh | bash
#    or: ./uninstall.sh
#    or: ./uninstall.sh --keep-data
#
# This script reverses the install.sh actions:
#   1. Deregisters the hook from ~/.claude/settings.json
#   2. Removes ~/.terse/bin/ PATH entry from shell profile
#   3. Removes the ~/.terse/ directory (binary, config, logs)
#
# Use --keep-data to preserve config and log files in ~/.terse/

set -euo pipefail

TERSE_HOME="$HOME/.terse"
BIN_DIR="$TERSE_HOME/bin"
CLAUDE_SETTINGS="$HOME/.claude/settings.json"

KEEP_DATA=false
FORCE=false

# ---------------------------------------------------------------------------
# Parse arguments
# ---------------------------------------------------------------------------

for arg in "$@"; do
    case "$arg" in
        --keep-data) KEEP_DATA=true ;;
        --force|-f)  FORCE=true ;;
        --help|-h)
            echo "Usage: $0 [--keep-data] [--force]"
            echo ""
            echo "  --keep-data  Remove binary and hook but preserve config/logs in ~/.terse/"
            echo "  --force      Skip confirmation prompt"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg"
            echo "Run '$0 --help' for usage."
            exit 1
            ;;
    esac
done

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

step()  { printf '\033[36m[terse]\033[0m %s\n' "$1"; }
ok()    { printf '  \033[32m✓\033[0m %s\n' "$1"; }
warn()  { printf '  \033[33m⚠\033[0m %s\n' "$1"; }
err()   { printf '  \033[31m✗\033[0m %s\n' "$1"; }

# ---------------------------------------------------------------------------
# Confirmation
# ---------------------------------------------------------------------------

step "Uninstalling TERSE (Token Efficiency through Refined Stream Engineering)"
echo ""

if [ "$KEEP_DATA" = true ]; then
    echo "  This will remove the terse binary and hook registration."
    echo "  Config and log files in $TERSE_HOME will be preserved."
else
    echo "  This will remove ALL terse files including config and logs."
    echo "  Use --keep-data to preserve config and log files."
fi
echo ""

# Only prompt when running interactively (not via curl | bash).
# Piping to bash is itself explicit consent.
if [ "$FORCE" = false ] && [ -t 0 ]; then
    printf "  Continue? [y/N] "
    read -r confirm
    case "$confirm" in
        y|Y|yes|Yes) ;;
        *)
            step "Uninstall cancelled."
            exit 0
            ;;
    esac
    echo ""
fi

# ---------------------------------------------------------------------------
# Step 1: Deregister Claude Code hook
# ---------------------------------------------------------------------------

step "Removing Claude Code hook..."

if [ -f "$CLAUDE_SETTINGS" ]; then
    if grep -q "terse.*hook" "$CLAUDE_SETTINGS" 2>/dev/null; then
        if command -v python3 &>/dev/null; then
            python3 -c "
import json, sys

with open('$CLAUDE_SETTINGS', 'r') as f:
    settings = json.load(f)

modified = False
hooks = settings.get('hooks', {})
pre = hooks.get('PreToolUse', [])

def has_terse(entry):
    # New matcher-based format: {'matcher': {}, 'hooks': [{'command': '...'}]}
    if 'hooks' in entry and isinstance(entry['hooks'], list):
        return any('terse' in h.get('command', '') and 'hook' in h.get('command', '')
                   for h in entry['hooks'])
    # Legacy flat format: {'type': 'command', 'command': '...'}
    cmd = entry.get('command', '')
    return 'terse' in cmd and 'hook' in cmd

filtered = [h for h in pre if not has_terse(h)]

if len(filtered) < len(pre):
    modified = True
    if filtered:
        hooks['PreToolUse'] = filtered
    else:
        hooks.pop('PreToolUse', None)
    # Remove empty hooks object
    if not hooks:
        settings.pop('hooks', None)
    else:
        settings['hooks'] = hooks

if modified:
    with open('$CLAUDE_SETTINGS', 'w') as f:
        json.dump(settings, f, indent=2)
    print('removed')
else:
    print('none')
" | {
                read -r result
                if [ "$result" = "removed" ]; then
                    ok "Hook removed from $CLAUDE_SETTINGS"
                else
                    ok "No terse hook found in Claude settings"
                fi
            }
        else
            warn "Could not modify $CLAUDE_SETTINGS automatically (python3 not found)."
            warn "Manually remove the terse hook entry from the PreToolUse array."
        fi
    else
        ok "No terse hook found in Claude settings"
    fi
else
    ok "No Claude settings file found (nothing to remove)"
fi

# ---------------------------------------------------------------------------
# Step 2: Remove from shell profile PATH
# ---------------------------------------------------------------------------

step "Removing from shell profile..."

SHELL_NAME="$(basename "${SHELL:-bash}" 2>/dev/null || echo "bash")"
case "$SHELL_NAME" in
    zsh)  PROFILE="$HOME/.zshrc" ;;
    fish) PROFILE="$HOME/.config/fish/config.fish" ;;
    *)    PROFILE="$HOME/.bashrc" ;;
esac

removed_from_profile=false

if [ -f "$PROFILE" ] && grep -q "$BIN_DIR" "$PROFILE" 2>/dev/null; then
    # Create a backup before modifying
    cp "$PROFILE" "$PROFILE.terse-backup"

    # Remove the TERSE comment line and the export/set line
    if [ "$(uname -s)" = "Darwin" ]; then
        # macOS sed requires '' for in-place with no backup
        sed -i '' "/# TERSE - Token Efficiency through Refined Stream Engineering/d" "$PROFILE"
        sed -i '' "\|$BIN_DIR|d" "$PROFILE"
    else
        sed -i "/# TERSE - Token Efficiency through Refined Stream Engineering/d" "$PROFILE"
        sed -i "\|$BIN_DIR|d" "$PROFILE"
    fi

    # Remove trailing blank lines left behind
    # (only remove if the last line is blank)
    while [ -s "$PROFILE" ] && [ -z "$(tail -c 1 "$PROFILE" | tr -d '\n')" ] && \
          [ "$(tail -1 "$PROFILE")" = "" ]; do
        if [ "$(uname -s)" = "Darwin" ]; then
            sed -i '' '${/^$/d;}' "$PROFILE"
        else
            sed -i '${/^$/d;}' "$PROFILE"
        fi
    done

    ok "Removed PATH entry from $PROFILE"
    ok "Backup saved to $PROFILE.terse-backup"
    removed_from_profile=true
else
    ok "No PATH entry found in $PROFILE"
fi

# Also check other common profiles
for alt_profile in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile" "$HOME/.bash_profile"; do
    # Skip if it's the same as the primary profile or doesn't exist
    [ "$alt_profile" = "$PROFILE" ] && continue
    [ ! -f "$alt_profile" ] && continue

    if grep -q "$BIN_DIR" "$alt_profile" 2>/dev/null; then
        cp "$alt_profile" "$alt_profile.terse-backup"
        if [ "$(uname -s)" = "Darwin" ]; then
            sed -i '' "/# TERSE - Token Efficiency through Refined Stream Engineering/d" "$alt_profile"
            sed -i '' "\|$BIN_DIR|d" "$alt_profile"
        else
            sed -i "/# TERSE - Token Efficiency through Refined Stream Engineering/d" "$alt_profile"
            sed -i "\|$BIN_DIR|d" "$alt_profile"
        fi
        ok "Also removed PATH entry from $alt_profile"
    fi
done

if [ "$removed_from_profile" = true ]; then
    warn "Restart your terminal or run: source $PROFILE"
fi

# ---------------------------------------------------------------------------
# Step 3: Remove files
# ---------------------------------------------------------------------------

if [ "$KEEP_DATA" = true ]; then
    step "Removing binary (keeping config and data)..."

    if [ -d "$BIN_DIR" ]; then
        rm -rf "$BIN_DIR"
        ok "Removed $BIN_DIR"
    else
        ok "Binary directory not found (already removed)"
    fi

    ok "Preserved data in $TERSE_HOME"
    if [ -d "$TERSE_HOME" ]; then
        echo "  Kept:"
        find "$TERSE_HOME" -maxdepth 1 -type f -exec basename {} \; | while read -r f; do
            echo "    $f"
        done
    fi
else
    step "Removing all terse files..."

    if [ -d "$TERSE_HOME" ]; then
        rm -rf "$TERSE_HOME"
        ok "Removed $TERSE_HOME"
    else
        ok "TERSE home directory not found (already removed)"
    fi
fi

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

echo ""
step "Uninstall complete!"
echo ""
if [ "$KEEP_DATA" = true ]; then
    echo "  Data preserved at: $TERSE_HOME"
    echo "  To fully remove:   rm -rf ~/.terse"
else
    echo "  All terse files have been removed."
fi
echo ""
