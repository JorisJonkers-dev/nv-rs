#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Install the porting factory's systemd user units from this clone. Nothing is enabled unless --enable is given.
#
#   scripts/fork/install-factory-service.sh [--enable] [--dry-run] [--uninstall]
#
# Copies the units of scripts/fork/systemd/ to ~/.config/systemd/user, writes ~/.config/nv-factory/env (the clone's
# path, kept when it exists) and reloads the user manager. With --enable it also enables and starts the service and the
# two timers. --uninstall stops and disables them and removes the files. --dry-run prints what would be done.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../.." && pwd)
units=(nv-factory.slice nv-factory.service nv-factory-watchdog.service nv-factory-watchdog.timer
       nv-factory-summary.service nv-factory-summary.timer)
active=(nv-factory.service nv-factory-watchdog.timer nv-factory-summary.timer)
target="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
envfile="${XDG_CONFIG_HOME:-$HOME/.config}/nv-factory/env"
enable=0 dry=0 remove=0
for arg in "$@"; do
  case "$arg" in
    --enable) enable=1 ;;
    --dry-run) dry=1 ;;
    --uninstall) remove=1 ;;
    *) echo "usage: $0 [--enable] [--dry-run] [--uninstall]" >&2; exit 2 ;;
  esac
done

do_it() {
  if [ "$dry" = 1 ]; then echo "would: $*"; else "$@"; fi
}

if [ "$remove" = 1 ]; then
  for unit in "${active[@]}"; do do_it systemctl --user disable --now "$unit" || true; done
  for unit in "${units[@]}"; do do_it rm -f "$target/$unit"; done
  do_it systemctl --user daemon-reload
  exit 0
fi

do_it mkdir -p "$target" "$(dirname "$envfile")"
for unit in "${units[@]}"; do do_it install -m 0644 "$here/systemd/$unit" "$target/$unit"; done
if [ ! -e "$envfile" ]; then
  if [ "$dry" = 1 ]; then echo "would write $envfile"; else printf 'NV_FACTORY_REPO=%s\n' "$repo" > "$envfile"; fi
fi
do_it systemctl --user daemon-reload
if [ "$enable" = 1 ]; then
  for unit in "${active[@]}"; do do_it systemctl --user enable --now "$unit"; done
else
  echo "installed, not enabled; to start: systemctl --user enable --now ${active[*]}"
fi
