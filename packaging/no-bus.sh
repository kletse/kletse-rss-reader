#!/usr/bin/env bash
# Runs a command with no way to a real keyring, and no way to a real config. For working on Onward: cargo test, the QML
# snapshots, tmux runs, anything that might start the app. See "Keyrings" in AGENTS.md.
#
#   packaging/no-bus.sh cargo test --locked
#   ONWARD_DEMO=1 packaging/no-bus.sh target/debug/onward
#
# XDG_CONFIG_HOME and XDG_STATE_HOME you set yourself are kept, so a run can have a config of its own; if they aren't
# set, each run gets empty folders. Either way they must not be the real ones.
set -u

export DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus
export DBUS_SYSTEM_BUS_ADDRESS=unix:path=/nonexistent/bus
unset GNOME_KEYRING_CONTROL

throwaway=$(mktemp -d "${TMPDIR:-/tmp}/onward-no-bus.XXXXXX")
trap 'rm -rf "$throwaway"' EXIT
mkdir -m 700 "$throwaway/config" "$throwaway/state" "$throwaway/run"
for own in "${XDG_CONFIG_HOME:-}" "${XDG_STATE_HOME:-}"; do
    if [ "$own" = "$HOME/.config" ] || [ "$own" = "$HOME/.local/state" ]; then
        echo "no-bus.sh: $own is the real one; use a throwaway folder" >&2
        exit 2
    fi
done
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$throwaway/config}"
export XDG_STATE_HOME="${XDG_STATE_HOME:-$throwaway/state}"
export XDG_RUNTIME_DIR="$throwaway/run"

"$@"
