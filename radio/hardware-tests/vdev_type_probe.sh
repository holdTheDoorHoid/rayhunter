#!/bin/sh
# Does the QCA9377 firmware answer VDEV_START for a MONITOR-type vdev?
#
# Everything host-side in monitor mode succeeds (vdev attach as type 4,
# self_peer, hdd_mon_open, WLANTL registration, sme_create_mon_session,
# wma_vdev_start on channel 6) and yet the firmware sends no VDEV_START_RESP
# and no rx indications at all.
#
# This runs the same monitor path twice, changing only the vdev type sent to
# firmware, using the rhmon_vdev_type module parameter:
#   pass 1: type 4 (WMI_VDEV_TYPE_MONITOR) - the real case
#   pass 2: type 2 (WMI_VDEV_TYPE_STA)     - control
# If the control gets a VDEV_START_RESP and monitor does not, the firmware is
# rejecting the monitor vdev type and no host-side change can fix it.

NEW=/tmp/wlan_new.ko
STOCK=/usr/lib/modules/3.18.48/extra/wlan.ko
LOG=/tmp/vdev_probe.log
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
    echo "##### VDEV PROBE COMPLETE"
}
trap restore EXIT

run_pass() {
    TYPE="$1"
    LABEL="$2"
    echo
    echo "============ PASS: vdev type $TYPE ($LABEL) ============"
    killall hostapd 2>/dev/null
    sleep 1
    rmmod wlan 2>/dev/null
    sleep 3
    echo "RHPROBE_TYPE_$TYPE" > /dev/kmsg
    insmod "$NEW" con_mode=4 rhmon_vdev_type="$TYPE" 2>&1
    echo "insmod rc=$?"
    sleep 6
    ifconfig wlan0 up 2>&1
    sleep 2
    iwpriv wlan0 setMonChan 6 0 2>&1
    echo "setMonChan rc=$?"
    R1=$(cat /sys/class/net/wlan0/statistics/rx_packets 2>/dev/null)
    sleep 12
    R2=$(cat /sys/class/net/wlan0/statistics/rx_packets 2>/dev/null)
    echo "rx_packets: $R1 -> $R2"
    echo "--- traces for this pass ---"
    dmesg | sed -n "/RHPROBE_TYPE_$TYPE/,\$p" | grep -E "RHMON" | head -12
    echo "--- VDEV_START_RESP seen? ---"
    dmesg | sed -n "/RHPROBE_TYPE_$TYPE/,\$p" | grep -c "VDEV_START_RESP"
}

echo "##### vdev type probe $(date)"
run_pass 4 "WMI_VDEV_TYPE_MONITOR"
run_pass 2 "WMI_VDEV_TYPE_STA control"

exit 0
