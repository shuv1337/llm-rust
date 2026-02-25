#!/usr/bin/env bash
#
# parity-diff.sh - Compare --help output between upstream Python llm and Rust llm-cli
#
# Usage:
#   ./scripts/parity-diff.sh                    # Compare all commands
#   ./scripts/parity-diff.sh prompt             # Compare specific command
#   ./scripts/parity-diff.sh --list             # List available commands
#
# Requirements:
#   - Python `llm` installed and in PATH (pip install llm)
#   - Rust binary built (cargo build --release)
#
# Environment:
#   UPSTREAM_BIN   - Path to upstream llm binary (default: llm)
#   RUST_BIN       - Path to Rust binary (default: ./target/release/llm-cli)
#   DIFF_TOOL      - Diff tool to use (default: diff -u)
#

set -euo pipefail

# Configuration
UPSTREAM_BIN="${UPSTREAM_BIN:-llm}"
RUST_BIN="${RUST_BIN:-./target/release/llm-cli}"
DIFF_TOOL="${DIFF_TOOL:-diff -u}"
TEMP_DIR=$(mktemp -d)

# Cleanup on exit
trap 'rm -rf "$TEMP_DIR"' EXIT

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Core commands present in both implementations
CORE_COMMANDS=(
    ""              # top-level help
    "prompt"
    "keys"
    "keys list"
    "keys get"
    "keys set"
    "keys path"
    "logs"
    "logs list"
    "logs path"
    "logs status"
    "logs on"
    "logs off"
    "logs backup"
    "models"
    "models list"
    "models default"
    "plugins"
)

# Commands only in upstream (expected to be missing in Rust)
UPSTREAM_ONLY=(
    "aliases"
    "chat"
    "collections"
    "embed"
    "embed-models"
    "embed-multi"
    "install"
    "openai"
    "schemas"
    "similar"
    "templates"
    "tools"
    "uninstall"
)

# Commands only in Rust (intentional extensions)
RUST_ONLY=(
    "cmd"
    "version"
    "keys resolve"
)

usage() {
    cat << 'USAGE'
parity-diff.sh - Compare --help output between upstream and Rust llm

Usage:
    ./scripts/parity-diff.sh                    Compare all core commands
    ./scripts/parity-diff.sh <command>          Compare specific command
    ./scripts/parity-diff.sh --list             List command categories
    ./scripts/parity-diff.sh --summary          Show parity summary only
    ./scripts/parity-diff.sh --help             Show this help

Examples:
    ./scripts/parity-diff.sh prompt             Compare prompt --help
    ./scripts/parity-diff.sh "keys list"        Compare keys list --help
    ./scripts/parity-diff.sh --summary          Quick parity overview

Environment:
    UPSTREAM_BIN    Path to upstream llm (default: llm)
    RUST_BIN        Path to Rust binary (default: ./target/release/llm-cli)
USAGE
}

list_commands() {
    echo -e "${BLUE}Core commands (should match):${NC}"
    for cmd in "${CORE_COMMANDS[@]}"; do
        echo "  ${cmd:-<top-level>}"
    done
    echo
    echo -e "${YELLOW}Upstream only (expected missing):${NC}"
    for cmd in "${UPSTREAM_ONLY[@]}"; do
        echo "  $cmd"
    done
    echo
    echo -e "${GREEN}Rust only (intentional extensions):${NC}"
    for cmd in "${RUST_ONLY[@]}"; do
        echo "  $cmd"
    done
}

check_binary() {
    local name="$1"
    local path="$2"
    if ! command -v "$path" &> /dev/null && [[ ! -x "$path" ]]; then
        echo -e "${RED}Error: $name not found at '$path'${NC}" >&2
        return 1
    fi
}

normalize_help() {
    # Normalize help output for comparison:
    # - Remove version numbers
    # - Normalize whitespace
    # - Remove path-specific content
    sed -E \
        -e 's/[0-9]+\.[0-9]+\.[0-9]+/X.Y.Z/g' \
        -e 's|/[^ ]+/\.config/io\.datasette\.llm|~/.config/io.datasette.llm|g' \
        -e 's/^Usage: [a-z-]+/Usage: llm/g' \
        -e 's/[[:space:]]+$//' \
        | grep -v '^$' || true
}

get_help() {
    local bin="$1"
    local cmd="$2"
    
    if [[ -z "$cmd" ]]; then
        "$bin" --help 2>&1 || true
    else
        # shellcheck disable=SC2086
        "$bin" $cmd --help 2>&1 || true
    fi
}

compare_command() {
    local cmd="$1"
    local label="${cmd:-<top-level>}"
    
    local upstream_file="$TEMP_DIR/upstream_${cmd// /_}.txt"
    local rust_file="$TEMP_DIR/rust_${cmd// /_}.txt"
    
    # Get help output
    get_help "$UPSTREAM_BIN" "$cmd" | normalize_help > "$upstream_file"
    get_help "$RUST_BIN" "$cmd" | normalize_help > "$rust_file"
    
    # Compare
    if $DIFF_TOOL "$upstream_file" "$rust_file" > /dev/null 2>&1; then
        echo -e "${GREEN}✓${NC} $label"
        return 0
    else
        echo -e "${RED}✗${NC} $label"
        if [[ "${SHOW_DIFF:-}" == "1" ]]; then
            echo "--- upstream ---"
            cat "$upstream_file"
            echo "--- rust ---"
            cat "$rust_file"
            echo "--- diff ---"
            $DIFF_TOOL "$upstream_file" "$rust_file" || true
            echo
        fi
        return 1
    fi
}

show_summary() {
    local pass=0
    local fail=0
    local skip=0
    
    echo -e "${BLUE}=== Parity Summary ===${NC}"
    echo
    
    # Check binaries exist
    if ! check_binary "Upstream llm" "$UPSTREAM_BIN" 2>/dev/null; then
        echo -e "${RED}Upstream binary not available${NC}"
        skip=$((skip + ${#CORE_COMMANDS[@]}))
    elif ! check_binary "Rust llm-cli" "$RUST_BIN" 2>/dev/null; then
        echo -e "${RED}Rust binary not available (run: cargo build --release)${NC}"
        skip=$((skip + ${#CORE_COMMANDS[@]}))
    else
        echo -e "${BLUE}Core commands:${NC}"
        for cmd in "${CORE_COMMANDS[@]}"; do
            if compare_command "$cmd"; then
                pass=$((pass + 1))
            else
                fail=$((fail + 1))
            fi
        done
    fi
    
    echo
    echo -e "${BLUE}Results:${NC}"
    echo -e "  ${GREEN}Pass:${NC} $pass"
    echo -e "  ${RED}Fail:${NC} $fail"
    echo -e "  ${YELLOW}Skip:${NC} $skip"
    echo
    echo -e "${YELLOW}Upstream-only commands:${NC} ${#UPSTREAM_ONLY[@]} (expected missing)"
    echo -e "${GREEN}Rust-only commands:${NC} ${#RUST_ONLY[@]} (intentional extensions)"
    
    if [[ $fail -gt 0 ]]; then
        return 1
    fi
    return 0
}

main() {
    case "${1:-}" in
        --help|-h)
            usage
            exit 0
            ;;
        --list|-l)
            list_commands
            exit 0
            ;;
        --summary|-s)
            show_summary
            exit $?
            ;;
        "")
            # Full comparison with diffs
            export SHOW_DIFF=1
            check_binary "Upstream llm" "$UPSTREAM_BIN"
            check_binary "Rust llm-cli" "$RUST_BIN"
            
            echo -e "${BLUE}=== Help Output Comparison ===${NC}"
            echo "Upstream: $UPSTREAM_BIN"
            echo "Rust:     $RUST_BIN"
            echo
            
            for cmd in "${CORE_COMMANDS[@]}"; do
                compare_command "$cmd" || true
            done
            ;;
        *)
            # Specific command comparison
            export SHOW_DIFF=1
            check_binary "Upstream llm" "$UPSTREAM_BIN"
            check_binary "Rust llm-cli" "$RUST_BIN"
            
            compare_command "$1"
            ;;
    esac
}

main "$@"
