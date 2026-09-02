#!/bin/sh
# Alternative routes to frame-level visibility on QCA9377 / qcacld-2.0,
# after con_mode 0-14 all failed to yield a monitor interface.
LOG=/tmp/monitor_alt.log
exec > "$LOG" 2>&1

cleanup() {
    iw dev scan0 del 2>/dev/null
    echo "##### ALT COMPLETE"
}
trap cleanup EXIT

echo "##### A: iwpriv setMonChan on the live AP iface"
iwpriv wlan0 setMonChan 6 2>&1
echo "rc=$?"
echo "--- iwpriv full command list ---"
iwpriv wlan0 2>&1 | head -40

echo
echo "##### B: change an added vif to monitor type"
iw dev wlan0 interface add scan0 type managed 2>&1
ifconfig scan0 down 2>/dev/null
iw dev scan0 set type monitor 2>&1
echo "set type monitor rc=$?"
iw dev scan0 info 2>&1 | head -6
iw dev scan0 set monitor control 2>&1
echo "set monitor control rc=$?"

echo
echo "##### C: pktlog interfaces"
ls -la /proc/ath_pktlog/ 2>&1 | head -10
find /sys/kernel/debug -iname "*pktlog*" 2>/dev/null | head -10
ls /sys/kernel/debug/ 2>&1 | head -20
mount | grep -i debug

echo
echo "##### D: mount debugfs and retry"
mkdir -p /tmp/dbg 2>/dev/null
mount -t debugfs none /tmp/dbg 2>&1
echo "mount rc=$?"
ls /tmp/dbg 2>&1 | head -20
find /tmp/dbg -iname "*pktlog*" -o -iname "*ieee80211*" 2>/dev/null | head -10

echo
echo "##### E: cfg80211 wiphy capabilities detail"
iw phy phy0 info 2>&1 | grep -iE "monitor|Supported interface modes" -A 8 | head -20

echo
echo "##### F: does the firmware expose a sniffer via iwpriv getConfig?"
iwpriv wlan0 getConfig 2>&1 | grep -iE "monitor|sniff|promis" | head -10

exit 0
