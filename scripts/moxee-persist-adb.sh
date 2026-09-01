#!/usr/bin/env bash
# Make ADB persist on a Moxee, with the target checked first.
#
# Why this wrapper exists: a Moxee and a home router very often both live at
# 192.168.1.1, on different interfaces. Which one the system picks can change,
# and picking the wrong one means sending the device's admin password to
# somebody else's router. So this fingerprints the target before sending
# anything, and refuses if it is not a Moxee running Rayhunter.
#
# The password is read straight into the installer. It is never passed on the
# command line, so it does not reach the process list or the shell history.

set -euo pipefail

ADMIN_IP="${1:-192.168.1.1}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

say "Checking what is actually at ${ADMIN_IP}..."

# Rayhunter's API answers on 8080. A home router does not.
api_code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 6 \
    "http://${ADMIN_IP}:8080/api/config" || true)"
if [ "$api_code" != "200" ]; then
    die "no Rayhunter API at ${ADMIN_IP}:8080 (got '${api_code:-nothing}').
       That address is probably not the Moxee. Check which interface it is on:
         ip route get ${ADMIN_IP}
       and that the Moxee is plugged in and enumerated:
         lsusb | grep 05c6"
fi

device="$(curl -s --max-time 6 "http://${ADMIN_IP}:8080/api/config" \
    | tr ',' '\n' | grep -m1 '"device"' | cut -d'"' -f4 || true)"
say "  Rayhunter is answering there, configured as: ${device:-unknown}"

route_dev="$(ip route get "$ADMIN_IP" 2>/dev/null | head -1 | sed -n 's/.* dev \([^ ]*\).*/\1/p')"
say "  traffic to ${ADMIN_IP} goes out via: ${route_dev:-unknown}"
say ""
say "This will set /usrdata/mode.cfg to 9 so the Moxee boots with ADB enabled."
say "It is reversible: run this again with --revert, or set mode.cfg back to 3."
say ""

read -r -s -p "Moxee admin password (not shown, not stored): " ADMIN_PASSWORD
echo
[ -n "$ADMIN_PASSWORD" ] || die "no password entered"

cd "$(dirname "$0")/.."
# Password goes in on stdin-free argv only for this one process, and the
# variable is unset immediately after.
ADMIN_PASSWORD="$ADMIN_PASSWORD" cargo run -q -p installer -- \
    util moxee-persist-adb \
    --admin-ip "$ADMIN_IP" \
    --admin-password "$ADMIN_PASSWORD" \
    "${@:2}"
unset ADMIN_PASSWORD

say ""
say "Now power cycle the Moxee, off USB power, then check:"
say "  lsusb | grep 05c6      # expect f622 rather than f626"
say "  adb devices"
