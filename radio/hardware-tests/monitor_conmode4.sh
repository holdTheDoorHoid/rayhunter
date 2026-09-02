#!/bin/sh
# Test whether con_mode=4 (VOS_MONITOR_MODE) plus the setMonChan private ioctl
# yields a working monitor interface on the RC400L.
#
# The earlier sweep only inspected `iw dev` (which reports the cfg80211 iftype)
# and concluded monitor was unavailable. That was the wrong measurement: with
# con_mode=4 the driver allocates the netdev via mon_mode_ether_setup, which
# sets ARPHRD_IEEE80211_RADIOTAP (803) regardless of what cfg80211 reports, and
# the channel is set with `iwpriv setMonChan` rather than `iw set channel`.
#
# Run via AT+SYSCMD; restores AP mode on every exit path.

LOG=/tmp/monitor_conmode4.log
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
    echo "##### MON4 COMPLETE"
}
trap restore EXIT

echo "##### con_mode=4 monitor test $(date)"
killall hostapd 2>/dev/null
sleep 1
rmmod wlan 2>&1
sleep 2
echo "PHASE0_MON4" > /dev/kmsg
insmod $KO con_mode=4 2>&1
echo "insmod rc=$?"
sleep 6

echo "con_mode readback: $(cat /sys/module/wlan/parameters/con_mode)"
echo "--- interfaces ---"
ls /sys/class/net/ | tr '\n' ' '; echo

for IF in wlan0 wlan1 mon0; do
    [ -d /sys/class/net/$IF ] || continue
    echo "--- $IF ---"
    # ARPHRD type: 1 = ether, 803 = IEEE80211_RADIOTAP, 802 = IEEE80211_PRISM
    echo "  ARPHRD type = $(cat /sys/class/net/$IF/type)"
    iw dev $IF info 2>&1 | sed 's/^/  /'
done

echo
echo "--- iwpriv command list for wlan0 (is setMonChan registered?) ---"
iwpriv wlan0 2>&1 | grep -iE "setMonChan|Available" | head -5

echo
echo "--- bring wlan0 up ---"
ifconfig wlan0 up 2>&1
echo "up rc=$?"
sleep 2
echo "  ARPHRD after up = $(cat /sys/class/net/wlan0/type)"

echo
echo "--- setMonChan 6 ---"
iwpriv wlan0 setMonChan 6 0 2>&1
echo "setMonChan rc=$?"
sleep 3
dmesg | sed -n '/PHASE0_MON4/,$p' | grep -iE "monitor|mon mode|RoamChannel|chan" | head -10

echo
echo "--- rx counters, 15s on channel 6 ---"
R1=$(cat /sys/class/net/wlan0/statistics/rx_packets)
B1=$(cat /sys/class/net/wlan0/statistics/rx_bytes)
sleep 15
R2=$(cat /sys/class/net/wlan0/statistics/rx_packets)
B2=$(cat /sys/class/net/wlan0/statistics/rx_bytes)
echo "rx_packets: $R1 -> $R2"
echo "rx_bytes:   $B1 -> $B2"

echo
echo "--- try other channels ---"
for CH in 1 11; do
    iwpriv wlan0 setMonChan $CH 0 2>&1
    echo "setMonChan $CH rc=$?"
    P1=$(cat /sys/class/net/wlan0/statistics/rx_packets)
    sleep 6
    P2=$(cat /sys/class/net/wlan0/statistics/rx_packets)
    echo "  ch$CH rx_packets: $P1 -> $P2"
done

echo
echo "--- dmesg tail ---"
dmesg | sed -n '/PHASE0_MON4/,$p' | tail -25

exit 0
