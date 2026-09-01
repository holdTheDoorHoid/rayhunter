#!/bin/sh
# Load a locally-built wlan.ko and prove the device still works normally.
#
# This is the baseline validation: before changing anything about monitor mode,
# an unmodified rebuild from matched sources must behave exactly like the
# vendor driver. If it does not, no later result can be trusted.
#
# Deliberately loads from /tmp and never touches the on-disk vendor module, so
# a reboot restores the stock driver with no intervention.
#
# Usage: test_custom_driver.sh [path-to-new-ko] [con_mode]

NEW="${1:-/tmp/wlan_new.ko}"
MODE="${2:-}"
STOCK=/usr/lib/modules/3.18.48/extra/wlan.ko
LOG=/tmp/custom_driver.log
exec > "$LOG" 2>&1

restore_stock() {
    echo "##### RESTORE STOCK DRIVER"
    rmmod wlan 2>/dev/null
    sleep 2
    insmod $STOCK 2>&1
    sleep 5
    ifconfig wlan0 up 2>/dev/null
    iw dev wlan0 interface add wlan1 type __ap 2>/dev/null
    ifconfig wlan1 up 2>/dev/null
    killall hostapd 2>/dev/null
    sleep 1
    hostapd -B /tmp/hostapd_wlan0.conf -P /tmp/hostapd_wlan0.pid 2>&1
    sleep 2
    brctl addif bridge0 wlan0 2>/dev/null
    echo "stock restored: hostapd=$(ps | grep -v grep | grep -c hostapd) ifaces=$(ls /sys/class/net | grep -c wlan)"
    echo "##### CUSTOM DRIVER TEST COMPLETE"
}
trap restore_stock EXIT

echo "##### custom driver test $(date)"
echo "new module: $NEW"
ls -la "$NEW"
echo "md5: $(md5sum "$NEW" | cut -d' ' -f1)"

echo
echo "--- unload stock ---"
killall hostapd 2>/dev/null
sleep 1
rmmod wlan 2>&1
echo "rmmod rc=$? loaded=$(lsmod | grep -c '^wlan ')"
sleep 2

echo
echo "--- insmod custom ${MODE:+(con_mode=$MODE)} ---"
echo "PHASE0_CUSTOM" > /dev/kmsg
if [ -n "$MODE" ]; then
    insmod "$NEW" con_mode="$MODE" 2>&1
else
    insmod "$NEW" 2>&1
fi
RC=$?
echo "insmod rc=$RC"
if [ $RC -ne 0 ]; then
    echo "!!! custom module refused to load"
    dmesg | sed -n '/PHASE0_CUSTOM/,$p' | tail -20
    exit 1
fi
sleep 6

echo
echo "--- loaded? ---"
lsmod | grep '^wlan '
echo "con_mode=$(cat /sys/module/wlan/parameters/con_mode 2>&1)"
echo "ifaces: $(ls /sys/class/net | tr '\n' ' ')"
[ -d /sys/class/net/wlan0 ] && echo "wlan0 ARPHRD=$(cat /sys/class/net/wlan0/type)"

echo
echo "--- driver banner from dmesg ---"
dmesg | sed -n '/PHASE0_CUSTOM/,$p' | grep -iE "wlan: loading|Host SW|driver loaded|Target Ready|HTT version" | head -8

if [ -z "$MODE" ]; then
    echo
    echo "--- normal operation: bring up AP and scan ---"
    ifconfig wlan0 up 2>&1
    iw dev wlan0 interface add wlan1 type __ap 2>/dev/null
    ifconfig wlan1 up 2>/dev/null
    hostapd -B /tmp/hostapd_wlan0.conf -P /tmp/hostapd_wlan0.pid 2>&1
    sleep 4
    echo "hostapd running: $(ps | grep -v grep | grep -c hostapd)"
    iw dev wlan0 interface add rhscan0 type managed 2>/dev/null
    ifconfig rhscan0 up 2>/dev/null
    sleep 1
    iw dev rhscan0 scan > /tmp/custom_scan.txt 2>&1
    echo "scan rc=$? networks=$(grep -c '^BSS' /tmp/custom_scan.txt)"
    iw dev rhscan0 del 2>/dev/null
else
    echo
    echo "--- monitor path ---"
    ifconfig wlan0 up 2>&1
    echo "up rc=$?"
    sleep 2
    iwpriv wlan0 setMonChan 6 0 2>&1
    echo "setMonChan rc=$?"
    R1=$(cat /sys/class/net/wlan0/statistics/rx_packets)
    sleep 12
    R2=$(cat /sys/class/net/wlan0/statistics/rx_packets)
    echo "rx_packets: $R1 -> $R2"
fi

echo
echo "--- errors in dmesg ---"
dmesg | sed -n '/PHASE0_CUSTOM/,$p' | grep -iE ":E :|BUG|Unable to handle|Oops|panic" | head -15

exit 0
