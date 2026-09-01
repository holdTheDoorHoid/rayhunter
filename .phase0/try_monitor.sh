#!/bin/sh
# Phase 0 experiment: load the qcacld-2.0 driver in monitor mode (con_mode=12)
# and report what the kernel and cfg80211 make of it.
MODE="${1:-12}"
KO=/usr/lib/modules/3.18.48/extra/wlan.ko

echo "### stopping wifi services"
killall hostapd 2>/dev/null
sleep 1
ifconfig wlan1 down 2>/dev/null
ifconfig wlan0 down 2>/dev/null
brctl delif bridge0 wlan0 2>/dev/null
sleep 1

echo "### rmmod wlan"
rmmod wlan 2>&1
sleep 2
echo "lsmod: $(lsmod | grep -c '^wlan ')"

echo "### dmesg mark"
echo "PHASE0_MARK_$MODE" > /dev/kmsg

echo "### insmod con_mode=$MODE"
insmod $KO con_mode=$MODE 2>&1
sleep 4

echo "### con_mode readback"
cat /sys/module/wlan/parameters/con_mode 2>&1

echo "### interfaces"
ls /sys/class/net/ 2>&1
echo "### iw dev"
iw dev 2>&1

echo "### supported interface modes"
iw list 2>&1 | sed -n '/Supported interface modes/,/software interface modes/p'

echo "### dmesg tail"
dmesg | sed -n "/PHASE0_MARK_$MODE/,\$p" | head -40
