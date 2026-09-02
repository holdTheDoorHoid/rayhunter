#!/bin/sh
# Re-run the con_mode=4 monitor test with the driver's own tracing turned up,
# so we can see how far the monitor path actually gets.
#
# The stock ini sets every vosTraceEnable* to 0, which leaves only E-level
# messages in dmesg. The interesting lines (notably "Set monitor mode Channel"
# and the SME/WMA session and vdev work) are at INFO, so they are invisible by
# default.
#
# Backs up and restores the ini, and restores AP mode, on every exit path.

LOG=/tmp/monitor_verbose.log
exec > "$LOG" 2>&1
KO=/usr/lib/modules/3.18.48/extra/wlan.ko
INI=/lib/firmware/wlan/qca_cld/WCNSS_qcom_cfg.ini

restore() {
    echo "##### RESTORE"
    [ -f "$INI.bak" ] && cp "$INI.bak" "$INI" && rm -f "$INI.bak"
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
    echo "restored: con_mode=$(cat /sys/module/wlan/parameters/con_mode) hostapd=$(ps | grep -v grep | grep -c hostapd) ini=$(grep -c vosTraceEnableHDD=0 $INI)"
    echo "##### VERBOSE COMPLETE"
}
trap restore EXIT

echo "##### verbose monitor test $(date)"
killall hostapd 2>/dev/null
sleep 1
rmmod wlan 2>/dev/null
sleep 2

echo "--- raise trace levels in ini ---"
cp "$INI" "$INI.bak"
# 4 = VOS_TRACE_LEVEL_INFO, which is what hddLog(LOG1, ...) uses.
sed -i 's/^vosTraceEnableHDD=.*/vosTraceEnableHDD=4/; s/^vosTraceEnableWDA=.*/vosTraceEnableWDA=4/; s/^vosTraceEnableSME=.*/vosTraceEnableSME=4/; s/^vosTraceEnablePE=.*/vosTraceEnablePE=4/; s/^vosTraceEnableWMA=.*/vosTraceEnableWMA=4/; s/^vosTraceEnableVOSS=.*/vosTraceEnableVOSS=4/' "$INI"
grep -E "^vosTraceEnable" "$INI"

echo
echo "--- load con_mode=4 ---"
echo "PHASE0_VERBOSE" > /dev/kmsg
insmod $KO con_mode=4 2>&1
echo "insmod rc=$?"
sleep 6

echo "con_mode=$(cat /sys/module/wlan/parameters/con_mode) ARPHRD=$(cat /sys/class/net/wlan0/type 2>/dev/null)"

echo
echo "--- ifconfig wlan0 up (this triggers hdd_mon_open) ---"
ifconfig wlan0 up 2>&1
echo "rc=$?"
sleep 3

echo
echo "--- setMonChan 6 ---"
iwpriv wlan0 setMonChan 6 0 2>&1
echo "rc=$?"
sleep 4

echo
echo "--- rx over 12s ---"
R1=$(cat /sys/class/net/wlan0/statistics/rx_packets)
sleep 12
R2=$(cat /sys/class/net/wlan0/statistics/rx_packets)
echo "rx_packets: $R1 -> $R2"

echo
echo "===== DRIVER LOG ====="
dmesg | sed -n '/PHASE0_VERBOSE/,$p' | grep -viE "set_addr_win" | grep -iE "wlan|hdd|sme|wma|pe |lim|txrx|htt|mon|vdev|peer|chan" | head -120

exit 0
