#!/bin/sh
# What management frames does the firmware actually hand the host?
#
# Monitor mode is blocked in firmware, but an access point must see probe
# requests in order to answer them - including the wildcard probes that
# current Flock cameras emit. If the firmware delivers those to the host in
# ordinary AP mode, surveillance detection gets the frames it needs without
# monitor mode at all.
#
# Runs the instrumented driver in normal AP mode and simply watches, for long
# enough that a quiet street still produces traffic.

NEW=/tmp/wlan_new.ko
STOCK=/usr/lib/modules/3.18.48/extra/wlan.ko
WATCH="${1:-180}"
LOG=/tmp/mgmt_census.log
exec > "$LOG" 2>&1

restore() {
    echo "##### RESTORE STOCK"
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
    echo "restored: hostapd=$(ps | grep -v grep | grep -c hostapd)"
    echo "##### CENSUS COMPLETE"
}
trap restore EXIT

echo "##### mgmt frame census $(date), watching ${WATCH}s"
killall hostapd 2>/dev/null
sleep 1
rmmod wlan 2>/dev/null
sleep 2
echo "RHCENSUS_START" > /dev/kmsg
insmod "$NEW" 2>&1
echo "insmod rc=$?"
sleep 6

echo "--- bring up AP normally ---"
ifconfig wlan0 up 2>&1
iw dev wlan0 interface add wlan1 type __ap 2>/dev/null
ifconfig wlan1 up 2>/dev/null
hostapd -B /tmp/hostapd_wlan0.conf -P /tmp/hostapd_wlan0.pid 2>&1
sleep 4
brctl addif bridge0 wlan0 2>/dev/null
echo "hostapd=$(ps | grep -v grep | grep -c hostapd)"
echo "AP channel: $(iw dev wlan0 info 2>/dev/null | grep -i channel)"

echo
echo "--- watching for ${WATCH}s ---"
sleep "$WATCH"

echo
echo "===== mgmt frames delivered by firmware ====="
dmesg | sed -n '/RHCENSUS_START/,$p' | grep "RHMON: entered" | tail -6
echo "----- probe requests -----"
dmesg | sed -n '/RHCENSUS_START/,$p' | grep "RHMON: PROBE_REQ" | head -10
echo "----- subtype census -----"
dmesg | sed -n '/RHCENSUS_START/,$p' | grep "RHMON: mgmt census" | tail -4
echo "----- totals -----"
echo "entered:   $(dmesg | sed -n '/RHCENSUS_START/,$p' | grep -c 'RHMON: entered')"
echo "probereqs: $(dmesg | sed -n '/RHCENSUS_START/,$p' | grep -c 'RHMON: PROBE_REQ')"

exit 0
