#!/bin/sh
# Does the firmware support OCB (802.11p, "outside the context of a BSS")?
#
# Why this is worth a try: the qcacld monitor-mode commits are things like
# "Add 5m/10m support for monitor mode" - 5/10 MHz channels are DSRC/802.11p.
# So this driver's monitor mode belongs to Qualcomm's vehicle-comms product
# line, and OCB is that line's normal operating mode. OCB receives frames
# without associating to anything, which is the closest thing to raw capture
# the driver offers short of monitor mode.
#
# Enabled with gDot11PMode=1 (standalone) in the ini, which makes
# hdd_wlan_startup open a WLAN_HDD_OCB adapter named wlanocb0 instead of the
# usual station interface.
#
# Restores the ini and AP mode on every exit path.

NEW=/tmp/wlan_new.ko
STOCK=/usr/lib/modules/3.18.48/extra/wlan.ko
INI=/lib/firmware/wlan/qca_cld/WCNSS_qcom_cfg.ini
LOG=/tmp/ocb_probe.log
exec > "$LOG" 2>&1

restore() {
    echo "##### RESTORE"
    [ -f "$INI.rhbak" ] && cp "$INI.rhbak" "$INI" && rm -f "$INI.rhbak" && echo "  ini restored"
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
    sleep 3
    brctl addif bridge0 wlan0 2>/dev/null
    echo "  hostapd=$(ps | grep -v grep | grep -c hostapd) dot11p_left=$(grep -c gDot11PMode $INI)"
    echo "##### OCB COMPLETE"
}
trap restore EXIT

echo "##### OCB probe $(date)"
killall hostapd 2>/dev/null
sleep 1
rmmod wlan 2>/dev/null
sleep 2

cp "$INI" "$INI.rhbak"
# The ini parser stops at the END marker and says so in a trailing comment,
# so a setting appended to the file is silently ignored. Insert before END.
sed -i 's/^END$/gDot11PMode=1\nEND/' "$INI"
echo "ini now has: $(grep gDot11PMode $INI)"
echo "position check (must be above END):"
grep -n -E "^gDot11PMode|^END" "$INI" | head -3

echo
echo "--- load driver with OCB standalone ---"
echo "RHOCB_START" > /dev/kmsg
insmod "$NEW" 2>&1
echo "insmod rc=$?"
sleep 8

echo "--- interfaces ---"
ls /sys/class/net/ | tr '\n' ' '; echo
for IF in wlanocb0 wlan0; do
    [ -d /sys/class/net/$IF ] || continue
    echo "$IF ARPHRD=$(cat /sys/class/net/$IF/type)"
    iw dev $IF info 2>&1 | sed 's/^/  /' | head -6
done

OCB=""
[ -d /sys/class/net/wlanocb0 ] && OCB=wlanocb0
if [ -n "$OCB" ]; then
    echo
    echo "--- OCB interface exists, bringing it up ---"
    ifconfig $OCB up 2>&1
    echo "up rc=$?"
    R1=$(cat /sys/class/net/$OCB/statistics/rx_packets)
    sleep 20
    R2=$(cat /sys/class/net/$OCB/statistics/rx_packets)
    echo "rx_packets: $R1 -> $R2"
else
    echo
    echo "--- no OCB interface was created ---"
fi

echo
echo "--- driver log ---"
dmesg | sed -n '/RHOCB_START/,$p' | grep -viE "set_addr_win" | grep -iE "ocb|dot11p|11p|RHMON|Target Ready|driver loaded|:E :" | head -25

exit 0
