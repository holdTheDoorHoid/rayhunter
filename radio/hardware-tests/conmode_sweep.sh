#!/bin/sh
# Sweep qcacld-2.0 con_mode values looking for one that yields a monitor
# interface. Known: 1 = SoftAP (device default), 5 = FTM (vendor init script).
LOG=/tmp/conmode_sweep.log
exec > "$LOG" 2>&1
KO=/usr/lib/modules/3.18.48/extra/wlan.ko

restore() {
    echo "##### RESTORE"
    rmmod wlan 2>/dev/null
    sleep 2
    insmod $KO 2>&1
    sleep 5
    ifconfig wlan0 up 2>/dev/null
    iw dev wlan0 interface add wlan1 type __ap 2>/dev/null
    ifconfig wlan1 up 2>/dev/null
    killall hostapd 2>/dev/null
    sleep 1
    hostapd -B /tmp/hostapd_wlan0.conf -P /tmp/hostapd_wlan0.pid 2>&1
    sleep 2
    brctl addif bridge0 wlan0 2>/dev/null
    echo "restored: con_mode=$(cat /sys/module/wlan/parameters/con_mode) hostapd=$(ps | grep -v grep | grep -c hostapd)"
    echo "##### SWEEP COMPLETE"
}
trap restore EXIT

killall hostapd 2>/dev/null
sleep 1

for M in 0 2 3 4 6 7 8 9 10 11 13 14; do
    echo "==================== con_mode=$M"
    rmmod wlan 2>/dev/null
    sleep 2
    insmod $KO con_mode=$M 2>&1
    echo "insmod rc=$?"
    sleep 5
    echo "readback=$(cat /sys/module/wlan/parameters/con_mode 2>&1)"
    echo "--- iw dev types ---"
    iw dev 2>&1 | grep -E "Interface|type"
    echo "--- monitor in supported modes? ---"
    iw list 2>&1 | sed -n '/Supported interface modes/,/software interface modes/p' | grep -ci monitor
    echo "--- netdevs ---"
    ls /sys/class/net/ | grep -E "wlan|mon" | tr '\n' ' '
    echo
    echo "--- dmesg (mode hints) ---"
    dmesg | tail -12 | grep -iE "monitor|mode|con_mode|invalid|fail" | head -6
done

echo "##### sweep loop done"
exit 0
