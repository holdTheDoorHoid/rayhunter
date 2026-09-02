#!/bin/sh
# Can the firmware be persuaded to hand the host probe requests?
#
# Established so far: monitor mode is refused by firmware, but in ordinary AP
# mode the firmware already forwards foreign beacons to the host (181 of 200
# management frames over three minutes). So it can receive frames not
# addressed to us; it simply filters probe requests out, most likely because
# it answers them itself.
#
# WMI_PDEV_PARAM_RX_FILTER (85) and WMI_PDEV_PARAM_SET_PROMISC_MODE_CMDID (96)
# exist in the WMI enum but this driver never sends either. The build carries
# a live knob so they can be tried without a rebuild per experiment.
#
# Each stage watches for probe requests, which is the thing that decides
# whether Flock-class detection is possible on this hardware at all.

NEW=/tmp/wlan_new.ko
STOCK=/usr/lib/modules/3.18.48/extra/wlan.ko
DWELL="${1:-45}"
P=/sys/module/wlan/parameters
LOG=/tmp/rx_filter.log
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
    echo "##### RXFILTER COMPLETE"
}
trap restore EXIT

send_param() {
    echo "$1" > $P/rhmon_param_id 2>&1
    echo "$2" > $P/rhmon_param_val 2>&1
    echo "$3" > $P/rhmon_scope 2>&1
    echo 1 > $P/rhmon_apply 2>&1
    echo "  applied id=$1 val=$2 scope=$3 (write rc=$?)"
}

stage() {
    LABEL="$1"
    MARK="RHSTAGE_$2"
    echo
    echo "======== STAGE $2: $LABEL ========"
    echo "$MARK" > /dev/kmsg
    sleep "$DWELL"
    PR=$(dmesg | sed -n "/$MARK/,\$p" | grep -c "RHMON: PROBE_REQ")
    IN=$(dmesg | sed -n "/$MARK/,\$p" | grep -c "RHMON: entered")
    echo "  mgmt frames delivered (sampled): $IN"
    echo "  PROBE REQUESTS: $PR"
    dmesg | sed -n "/$MARK/,\$p" | grep "RHMON: mgmt census" | tail -1
    dmesg | sed -n "/$MARK/,\$p" | grep "RHMON: PROBE_REQ" | head -4
}

echo "##### rx filter probe $(date), ${DWELL}s per stage"
killall hostapd 2>/dev/null
sleep 1
rmmod wlan 2>/dev/null
sleep 2
insmod "$NEW" 2>&1
echo "insmod rc=$?"
sleep 6

ifconfig wlan0 up 2>&1
iw dev wlan0 interface add wlan1 type __ap 2>/dev/null
ifconfig wlan1 up 2>/dev/null
hostapd -B /tmp/hostapd_wlan0.conf -P /tmp/hostapd_wlan0.pid 2>&1
sleep 4
brctl addif bridge0 wlan0 2>/dev/null
echo "hostapd=$(ps | grep -v grep | grep -c hostapd)"
echo "knob present: $(ls $P | grep -c rhmon)"

stage "baseline, no parameters sent" 0

echo "--- set promiscuous mode = 1 ---"
send_param 96 1 0
stage "PROMISC_MODE=1" 1

echo "--- set rx filter = 0xFFFFFFFF (accept everything) ---"
send_param 85 -1 0
stage "RX_FILTER=all" 2

echo "--- set rx filter = 0xA0 (promiscuous | probe request) ---"
send_param 85 160 0
stage "RX_FILTER=0xA0" 3

echo
echo "===== parameter send results ====="
dmesg | grep "RHMON: sent param" | tail -6

exit 0
