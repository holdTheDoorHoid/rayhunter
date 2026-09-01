#!/bin/sh
# Rayhunter Phase 0 radio capability probe (Orbic RC400L / QCA9377 / qcacld-2.0).
#
# Invoked via AT+SYSCMD so it inherits the full capability bounding set
# (CAP_SYS_MODULE / CAP_NET_ADMIN); adb-spawned processes have only
# CAP_SETUID|CAP_SETGID and cannot load modules or configure interfaces.
# Restores AP mode on every exit path.

LOG=/tmp/phase0_probe.log
exec > "$LOG" 2>&1

KO=/usr/lib/modules/3.18.48/extra/wlan.ko

restore_ap() {
    echo "##### RESTORE: returning to AP mode"
    iw dev scan0 del 2>/dev/null
    iw dev mon0 del 2>/dev/null
    if [ "$(cat /sys/module/wlan/parameters/con_mode 2>/dev/null)" != "1" ]; then
        rmmod wlan 2>&1
        sleep 2
        insmod $KO 2>&1
        sleep 4
        ifconfig wlan0 up 2>&1
        iw dev wlan0 interface add wlan1 type __ap 2>/dev/null
        ifconfig wlan1 up 2>/dev/null
    fi
    killall hostapd 2>/dev/null
    sleep 1
    hostapd -B /tmp/hostapd_wlan0.conf -P /tmp/hostapd_wlan0.pid 2>&1
    sleep 2
    brctl addif bridge0 wlan0 2>/dev/null
    echo "restore: con_mode=$(cat /sys/module/wlan/parameters/con_mode 2>&1) hostapd=$(ps | grep -v grep | grep -c hostapd)"
    iw dev | grep -E "Interface|type"
    echo "##### PROBE COMPLETE"
}
trap restore_ap EXIT

echo "##### Phase 0 probe start: $(date)"
echo "caps: $(grep CapEff /proc/self/status)"

echo
echo "##### SECTION 1: baseline"
uptime
iw dev
echo "--- supported modes ---"
iw list | sed -n '/Supported interface modes/,/software interface modes/p'
head -3 /proc/meminfo
echo "--- hostapd running: $(ps | grep -v grep | grep -c hostapd)"

echo
echo "##### SECTION 2: managed scan vif alongside AP (Phase 1 mechanism)"
iw dev wlan0 interface add scan0 type managed 2>&1
echo "add scan0 rc=$?"
ifconfig scan0 up 2>&1
echo "scan0 up rc=$?"
iw dev | grep -E "Interface|type"
echo "--- hostapd still up: $(ps | grep -v grep | grep -c hostapd)"

echo
echo "##### SECTION 3: scanning on scan0 (AP stays live?)"
for i in 1 2 3; do
    A=$(cut -d' ' -f1 /proc/uptime)
    iw dev scan0 scan > /tmp/scan_$i.txt 2>/tmp/scanerr_$i.txt
    RC=$?
    B=$(cut -d' ' -f1 /proc/uptime)
    echo "scan $i rc=$RC start=$A end=$B bss=$(grep -c '^BSS' /tmp/scan_$i.txt) hostapd=$(ps | grep -v grep | grep -c hostapd)"
    head -2 /tmp/scanerr_$i.txt
done
echo "--- distinct frequencies observed ---"
grep -h "freq:" /tmp/scan_1.txt /tmp/scan_2.txt /tmp/scan_3.txt 2>/dev/null | sort -u | head -30
echo "--- signal strengths present? ---"
grep -h "signal:" /tmp/scan_1.txt | head -5
echo "--- information elements present? (vendor/IE) ---"
grep -hE "Vendor specific|SSID:|HT capabilities|WPS|country" /tmp/scan_1.txt | head -12
echo "--- sample of one BSS record ---"
sed -n '1,25p' /tmp/scan_1.txt

echo
echo "##### SECTION 4: monitor vif while driver is in AP con_mode"
iw dev wlan0 interface add mon0 type monitor 2>&1
echo "add mon0 rc=$?"
iw dev | grep -E "Interface|type"

echo
echo "##### SECTION 5: cleanup vifs"
iw dev mon0 del 2>/dev/null; echo "del mon0 rc=$?"
iw dev scan0 del 2>/dev/null; echo "del scan0 rc=$?"

echo
echo "##### SECTION 6: reload driver with con_mode=12 (monitor)"
killall hostapd 2>/dev/null
sleep 1
rmmod wlan 2>&1
echo "rmmod rc=$? loaded=$(lsmod | grep -c '^wlan ')"
sleep 2
insmod $KO con_mode=12 2>&1
echo "insmod con_mode=12 rc=$?"
sleep 6
echo "con_mode readback: $(cat /sys/module/wlan/parameters/con_mode 2>&1)"
echo "--- net ifaces ---"
ls /sys/class/net/
echo "--- iw dev ---"
iw dev
echo "--- supported modes now ---"
iw list | sed -n '/Supported interface modes/,/software interface modes/p'
echo "--- dmesg ---"
dmesg | tail -40

echo
echo "##### SECTION 7: monitor capture test"
MON=""
for c in mon0 wlan0 mon1 wifi0; do
    if [ -d /sys/class/net/$c ]; then MON=$c; break; fi
done
echo "candidate monitor iface: $MON"
if [ -n "$MON" ]; then
    ifconfig $MON up 2>&1; echo "up rc=$?"
    iw dev $MON info 2>&1
    for ch in 1 6 11 36 149; do
        iw dev $MON set channel $ch 2>&1
        echo "set channel $ch rc=$?"
    done
    iw dev $MON set channel 6 2>/dev/null
    echo "--- rx counters over 12s on channel 6 ---"
    R1=$(cat /sys/class/net/$MON/statistics/rx_packets 2>/dev/null)
    B1=$(cat /sys/class/net/$MON/statistics/rx_bytes 2>/dev/null)
    sleep 12
    R2=$(cat /sys/class/net/$MON/statistics/rx_packets 2>/dev/null)
    B2=$(cat /sys/class/net/$MON/statistics/rx_bytes 2>/dev/null)
    echo "rx_packets: $R1 -> $R2    rx_bytes: $B1 -> $B2"
    head -3 /proc/meminfo
    cat /proc/loadavg
fi

echo
echo "##### SECTION 8: done"
exit 0
