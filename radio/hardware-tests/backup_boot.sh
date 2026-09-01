#!/bin/sh
# Back up the boot and recovery partitions before any kernel work.
#
# The RC400L is NAND with MTD partitions; mtd8 is "boot" (7.75 MB) and mtd12
# is "recovery". Both are dumped to /data so they can be pulled over USB and
# written back with nandwrite if a replacement kernel fails to boot.
#
# nanddump includes OOB data by default, which is NOT what nandwrite wants
# back; --omitoob keeps the dump to page data so it can be written straight
# back with `nandwrite -p`.

OUT=/data/mtdbackup
LOG=/tmp/backup_boot.log
exec > "$LOG" 2>&1

mkdir -p "$OUT"

echo "##### mtd backup $(date)"
grep -E "mtd8|mtd12|mtd11|mtd7" /proc/mtd

for spec in "8:boot" "12:recovery" "11:misc" "7:aboot"; do
    N="${spec%%:*}"
    NAME="${spec##*:}"
    echo
    echo "--- dumping mtd$N ($NAME) ---"
    nanddump --omitoob -f "$OUT/mtd${N}_${NAME}.img" "/dev/mtd${N}" 2>&1 | tail -3
    ls -la "$OUT/mtd${N}_${NAME}.img"
    md5sum "$OUT/mtd${N}_${NAME}.img"
done

echo
echo "--- free space ---"
df -h /data | tail -2
echo "##### BACKUP COMPLETE"
