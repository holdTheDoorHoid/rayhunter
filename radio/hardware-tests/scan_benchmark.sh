#!/bin/sh
# Sustained-scanning cost on the Orbic RC400L.
#
# Answers the questions in requirement 14: what continuous Wi-Fi scanning
# costs in CPU, memory and storage, and — the one that decides whether the
# feature is acceptable at all — whether it degrades cellular capture.
#
# Cellular health is measured as the growth rate of the active QMDL file
# before, during and after scanning. A drop while scanning would mean diag
# messages are being missed.
#
# Run via AT+SYSCMD (needs CAP_NET_ADMIN to create the scan interface).

LOG=/tmp/scan_benchmark.log
exec > "$LOG" 2>&1

IFACE=rhscan0
BASE=wlan0
PHASE_SECS="${1:-420}"     # per phase: baseline, scanning, recovery

qmdl_bytes() {
    # Newest QMDL in the active recording directory.
    f=$(ls -t /data/rayhunter/qmdl/*.qmdl.gz 2>/dev/null | head -1)
    [ -n "$f" ] && wc -c < "$f" || echo 0
}

mem_free() { grep MemAvailable /proc/meminfo | tr -dc '0-9'; }
loadavg()  { cut -d' ' -f1 /proc/loadavg; }

rayhunter_cpu() {
    # Ticks of user+system CPU consumed by the cellular daemon so far.
    p=$(ps | grep -v grep | grep rayhunter-daemon | head -1 | awk '{print $1}')
    [ -n "$p" ] || { echo 0; return; }
    awk '{print $14 + $15}' /proc/"$p"/stat 2>/dev/null || echo 0
}

sample() {
    echo "$1 t=$(cut -d' ' -f1 /proc/uptime) qmdl=$(qmdl_bytes) memavail=$(mem_free) load=$(loadavg) rh_cpu_ticks=$(rayhunter_cpu) hostapd=$(ps | grep -v grep | grep -c hostapd)"
}

cleanup() {
    iw dev $IFACE del 2>/dev/null
    echo "##### BENCHMARK COMPLETE"
}
trap cleanup EXIT

echo "##### scan benchmark, ${PHASE_SECS}s per phase, started $(date)"
echo "clock ticks per second: $(getconf CLK_TCK 2>/dev/null || echo 100)"

echo
echo "### PHASE 1: baseline (no scanning)"
sample baseline_start
sleep "$PHASE_SECS"
sample baseline_end

echo
echo "### PHASE 2: continuous scanning"
iw dev $BASE interface add $IFACE type managed 2>&1
ip link set $IFACE up 2>&1
sample scanning_start
SCANS=0
FAILED=0
END=$(( $(cut -d' ' -f1 /proc/uptime | cut -d. -f1) + PHASE_SECS ))
while [ "$(cut -d' ' -f1 /proc/uptime | cut -d. -f1)" -lt "$END" ]; do
    if iw dev $IFACE scan > /tmp/bench_scan.txt 2>/dev/null; then
        SCANS=$((SCANS + 1))
    else
        FAILED=$((FAILED + 1))
    fi
    # Pace the loop the way a daemon would rather than scanning flat out.
    sleep 5
done
sample scanning_end
echo "scans_completed=$SCANS scans_failed=$FAILED"
echo "last_scan_bss=$(grep -c '^BSS' /tmp/bench_scan.txt 2>/dev/null)"
iw dev $IFACE del 2>/dev/null

echo
echo "### PHASE 3: recovery (no scanning)"
sample recovery_start
sleep "$PHASE_SECS"
sample recovery_end

echo
echo "### daemon health"
ps | grep -v grep | grep -c rayhunter-daemon
echo "recent rayhunter log:"
tail -12 /data/rayhunter/rayhunter.log 2>/dev/null
echo "storage used by recordings:"
du -sk /data/rayhunter/qmdl 2>/dev/null

exit 0
