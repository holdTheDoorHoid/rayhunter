#!/bin/sh
# Swap the WLAN firmware blob and retry monitor mode.
#
# The device carries two different ROME firmware builds:
#   /lib/firmware/qwlan30.bin     619364 bytes, Aug 2020  (the one in use)
#   /firmware/image/qwlan30.bin   613948 bytes, Apr 2020  (unused alternative)
# with matching otp30.bin and bdwlan30.bin that also differ.
#
# Monitor mode fails because the firmware never answers VDEV_START for a
# monitor-type vdev. If the other build answers, monitor mode becomes
# reachable without replacing anything else.
#
# Fully reversible: these are ordinary files, backed up here and restored on
# every exit path, including on error.
#
# Usage: firmware_swap.sh [fw-only|fw+otp|restore]

MODE="${1:-fw-only}"
NEW=/tmp/wlan_new.ko
STOCK=/usr/lib/modules/3.18.48/extra/wlan.ko
LF=/lib/firmware
IMG=/firmware/image
LOG=/tmp/fw_swap.log
exec > "$LOG" 2>&1

restore_all() {
    echo "##### RESTORE firmware and stock driver"
    rmmod wlan 2>/dev/null
    sleep 2
    for f in qwlan30.bin otp30.bin bdwlan30.bin; do
        if [ -f "$LF/$f.rhbak" ]; then
            cp "$LF/$f.rhbak" "$LF/$f" && rm -f "$LF/$f.rhbak"
            echo "  restored $f"
        fi
    done
    echo "  md5 after restore: $(md5sum $LF/qwlan30.bin | cut -d' ' -f1)"
    insmod $STOCK 2>&1
    sleep 5
    ifconfig wlan0 up 2>/dev/null
    iw dev wlan0 interface add wlan1 type __ap 2>/dev/null
    ifconfig wlan1 up 2>/dev/null
    killall hostapd 2>/dev/null
    sleep 1
    hostapd -B /tmp/hostapd_wlan0.conf -P /tmp/hostapd_wlan0.pid 2>&1
    sleep 3
    brctl addif bridge0 wlan0 2>/dev/null
    echo "  hostapd=$(ps | grep -v grep | grep -c hostapd) ifaces=$(ls /sys/class/net | grep -c wlan)"
    echo "##### FWSWAP COMPLETE"
}
trap restore_all EXIT

echo "##### firmware swap test $(date) mode=$MODE"
echo "before: lib=$(md5sum $LF/qwlan30.bin | cut -d' ' -f1) img=$(md5sum $IMG/qwlan30.bin | cut -d' ' -f1)"

killall hostapd 2>/dev/null
sleep 1
rmmod wlan 2>/dev/null
sleep 2

echo
echo "--- backing up and swapping firmware ---"
cp "$LF/qwlan30.bin" "$LF/qwlan30.bin.rhbak" && cp "$IMG/qwlan30.bin" "$LF/qwlan30.bin"
echo "  qwlan30.bin swapped, now $(md5sum $LF/qwlan30.bin | cut -d' ' -f1)"
if [ "$MODE" = "fw+otp" ]; then
    cp "$LF/otp30.bin" "$LF/otp30.bin.rhbak" && cp "$IMG/otp30.bin" "$LF/otp30.bin"
    cp "$LF/bdwlan30.bin" "$LF/bdwlan30.bin.rhbak" && cp "$IMG/bdwlan30.bin" "$LF/bdwlan30.bin"
    echo "  otp30.bin and bdwlan30.bin also swapped"
fi

echo
echo "===== PASS A: does the alternative firmware boot at all (normal mode)? ====="
echo "RHFW_NORMAL" > /dev/kmsg
insmod "$NEW" 2>&1
echo "insmod rc=$?"
sleep 8
dmesg | sed -n '/RHFW_NORMAL/,$p' | grep -iE "Host SW|Target Ready|HTT version|driver loaded|failed|BMI|timeout" | head -8
ifconfig wlan0 up 2>&1
sleep 2
iw dev wlan0 interface add rhscan0 type managed 2>/dev/null
ifconfig rhscan0 up 2>/dev/null
sleep 1
iw dev rhscan0 scan > /tmp/fwswap_scan.txt 2>&1
echo "scan rc=$? networks=$(grep -c '^BSS' /tmp/fwswap_scan.txt)"
iw dev rhscan0 del 2>/dev/null
rmmod wlan 2>/dev/null
sleep 3

echo
echo "===== PASS B: monitor mode on the alternative firmware ====="
echo "RHFW_MONITOR" > /dev/kmsg
insmod "$NEW" con_mode=4 2>&1
echo "insmod rc=$?"
sleep 8
echo "con_mode=$(cat /sys/module/wlan/parameters/con_mode) ARPHRD=$(cat /sys/class/net/wlan0/type 2>/dev/null)"
ifconfig wlan0 up 2>&1
sleep 2
iwpriv wlan0 setMonChan 6 0 2>&1
echo "setMonChan rc=$?"
R1=$(cat /sys/class/net/wlan0/statistics/rx_packets 2>/dev/null)
sleep 15
R2=$(cat /sys/class/net/wlan0/statistics/rx_packets 2>/dev/null)
echo "rx_packets: $R1 -> $R2"

echo
echo "--- monitor traces ---"
dmesg | sed -n '/RHFW_MONITOR/,$p' | grep "RHMON" | head -12
echo "--- VDEV_START_RESP count (the decisive line) ---"
dmesg | sed -n '/RHFW_MONITOR/,$p' | grep -c "VDEV_START_RESP"
echo "--- firmware banner under monitor ---"
dmesg | sed -n '/RHFW_MONITOR/,$p' | grep -iE "Host SW|Target Ready|driver loaded" | head -4

exit 0
