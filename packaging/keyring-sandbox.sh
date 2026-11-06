#!/usr/bin/env bash
# Runs a command beside a keyring that isn't yours. The only place the real store may be exercised: see "Keyrings" in
# AGENTS.md. Everything else goes through no-bus.sh.
#
#   packaging/keyring-sandbox.sh <state> <command…>
#
# <state> is what's on the private bus:
#   unlocked   a keyring with a password, unlocked
#   locked     the same keyring, locked: nothing here can answer its prompt
#   none       a daemon with no keyring at all
#   omarchy    the file Omarchy's installer writes: a keyring with no password
#   hung       no keyring: a stub owns the name and never answers
#   nobus-name a private bus with nobody owning the name
#
# ONWARD_SANDBOX_ROOT reuses a folder, so a keyring outlives a run. Otherwise each run starts empty and leaves nothing.
#
# What keeps this from reaching the real thing:
#   - bwrap puts a tmpfs over /run and your home, so the real bus, /run/user/1000/keyring and ~/.local/share/keyrings
#     aren't there. Only this worktree is brought back, read-only, for its binaries.
#   - The bus has no service directories, so it can't start anything. A session bus that could would start a daemon by
#     itself, and that one looks for a running one.
#   - XDG_RUNTIME_DIR is /run/k. A long one breaks the daemon's socket, which is how this went wrong the first time.
#   - The owner of org.freedesktop.secrets must be the PID started here, or it stops before the command runs.
set -eu

state=${1:?usage: keyring-sandbox.sh <unlocked|locked|none|omarchy|hung|nobus-name> <command…>}
shift
worktree=$(cd "$(dirname "$0")/.." && pwd)

if [ -z "${ONWARD_SANDBOX_INSIDE:-}" ]; then
    root=${ONWARD_SANDBOX_ROOT:-}
    made=
    if [ -z "$root" ]; then
        root=$(mktemp -d /tmp/oks.XXXXXX)
        made=$root
    fi
    mkdir -p "$root"
    root=$(cd "$root" && pwd)
    set +e
    env -u GNOME_KEYRING_CONTROL -u DBUS_SYSTEM_BUS_ADDRESS -u DISPLAY -u WAYLAND_DISPLAY \
        DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus \
        ONWARD_SANDBOX_INSIDE=1 ONWARD_SANDBOX_ROOT="$root" \
        bwrap --dev-bind / / --proc /proc --tmpfs /run --tmpfs "$HOME" \
        --ro-bind "$worktree" "$worktree" --bind "$root" "$root" \
        --unshare-pid --die-with-parent --chdir "$root" \
        "$worktree/packaging/keyring-sandbox.sh" "$state" "$@"
    status=$?
    [ -n "$made" ] && rm -rf "$made"
    exit "$status"
fi

root=$ONWARD_SANDBOX_ROOT
mkdir -p "$root/home" "$root/config" "$root/state" "$root/data" /run/k
chmod 700 /run/k
export HOME="$root/home" XDG_CONFIG_HOME="$root/config" XDG_STATE_HOME="$root/state" XDG_DATA_HOME="$root/data"
export XDG_RUNTIME_DIR=/run/k TMUX_TMPDIR=/run/k
unset GNOME_KEYRING_CONTROL

if [ -z "${ONWARD_SANDBOX_BUS:-}" ]; then
    cat > "$root/bus.conf" <<CONF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <keep_umask/>
  <listen>unix:path=/run/k/bus</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF
    export ONWARD_SANDBOX_BUS=1
    exec dbus-run-session --config-file="$root/bus.conf" -- "$0" "$state" "$@"
fi

case "$DBUS_SESSION_BUS_ADDRESS" in
    unix:path=/run/k/bus*) ;;
    *) echo "sandbox: not on the private bus ($DBUS_SESSION_BUS_ADDRESS)" >&2; exit 3 ;;
esac

keyrings="$XDG_DATA_HOME/keyrings"
daemon() { exec /usr/bin/gnome-keyring-daemon --foreground --components=secrets "$@"; }

# Asks the private bus who has the name; the address was checked above.
owner_pid() {
    local name
    name=$(/usr/bin/gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
        --method org.freedesktop.DBus.GetNameOwner org.freedesktop.secrets 2>/dev/null) || return 1
    name=${name#*\'}; name=${name%%\'*}
    /usr/bin/gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
        --method org.freedesktop.DBus.GetConnectionUnixProcessID "$name" | sed -E 's/.* ([0-9]+).*/\1/'
}

wait_for_owner() {
    for _ in $(seq 100); do
        owner=$(owner_pid || true)
        [ -n "$owner" ] && break
        sleep 0.1
    done
    if [ "${owner:-}" != "$1" ]; then
        echo "sandbox: org.freedesktop.secrets is owned by '${owner:-nobody}', not $1. Stopping." >&2
        kill "$1" 2>/dev/null || true
        exit 3
    fi
    echo "sandbox: $state: org.freedesktop.secrets is owned by $1, started here"
}

# Makes the password keyring, and stops the daemon that made it.
make_keyring() {
    [ -e "$keyrings/login.keyring" ] && return
    (daemon --unlock < <(printf "sandbox-password") > /dev/null 2>&1) &
    wait_for_owner $!
    /usr/bin/gdbus call --session --dest org.freedesktop.secrets --object-path /org/freedesktop/secrets/aliases/default \
        --method org.freedesktop.DBus.Properties.Get org.freedesktop.Secret.Collection Locked > /dev/null
    kill %% 2>/dev/null || true
    wait 2>/dev/null || true
    for _ in $(seq 50); do [ -z "$(owner_pid || true)" ] && break; sleep 0.1; done
}

case "$state" in
    unlocked)
        (daemon --unlock < <(printf "sandbox-password") > "$root/daemon.log" 2>&1) &
        wait_for_owner $!
        ;;
    locked)
        make_keyring
        (daemon < /dev/null > "$root/daemon.log" 2>&1) &
        wait_for_owner $!
        ;;
    none)
        rm -rf "$keyrings"
        (daemon < /dev/null > "$root/daemon.log" 2>&1) &
        wait_for_owner $!
        ;;
    omarchy)
        # As install/user/default-keyring.sh writes them, read in spike S8.
        if [ ! -e "$keyrings/default" ]; then
            mkdir -p "$keyrings"
            chmod 700 "$keyrings"
            printf 'Default_keyring' > "$keyrings/default"
            printf '[keyring]\ndisplay-name=Default keyring\nctime=1700000000\nmtime=0\nlock-on-idle=false\nlock-after=false\n' \
                > "$keyrings/Default_keyring.keyring"
            chmod 600 "$keyrings/default" "$keyrings/Default_keyring.keyring"
        fi
        (daemon < /dev/null > "$root/daemon.log" 2>&1) &
        wait_for_owner $!
        ;;
    hung)
        python3 -I "$(dirname "$0")/keyring-stub.py" /run/k/bus &
        wait_for_owner $!
        ;;
    nobus-name)
        echo "sandbox: nobus-name: nobody owns org.freedesktop.secrets"
        ;;
    *)
        echo "sandbox: no state called $state" >&2
        exit 2
        ;;
esac

set +e
"$@"
status=$?
kill %% 2>/dev/null
exit "$status"
