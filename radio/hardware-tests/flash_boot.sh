#!/bin/sh
# Flash a replacement boot image to the RC400L's mtd8, with verification.
#
# This is the one genuinely dangerous operation in this project. aboot has no
# software route into fastboot or recovery on this device (none of the
# Qualcomm restart cookies appear in it), so a kernel that loads and hangs
# needs a physical key combination to recover. The device does have two
# buttons - power (qpnp_pon) and wps_reset - and aboot reverts to fastboot if
# the boot image fails to load, which is the automatic safety net.
#
# What makes this reasonable rather than reckless: the device reports
# fuse_flag=0, so the secure-boot fuses are not blown and aboot does not
# enforce the image signature.
#
# Reads back and compares before it will let the device reboot. If the
# verify fails it restores the backup and says so, rather than leaving a
# half-written kernel in place.
#
# Usage: flash_boot.sh <image> [mtd-number]

IMG="${1:?usage: flash_boot.sh <image> [mtd]}"
MTD="${2:-8}"
BACKUP=/data/mtdbackup/mtd8_boot.img
LOG=/tmp/flash_boot.log
exec > "$LOG" 2>&1

echo "##### flash boot $(date)"
echo "image: $IMG  -> /dev/mtd$MTD"
ls -la "$IMG"
echo "image md5: $(md5sum "$IMG" | cut -d' ' -f1)"
SIZE=$(wc -c < "$IMG")
echo "image size: $SIZE"

if [ ! -f "$BACKUP" ]; then
    echo "!!! no backup at $BACKUP - refusing to flash"
    echo "##### FLASH ABORTED"
    exit 1
fi
echo "backup present: $(md5sum $BACKUP | cut -d' ' -f1)"

PART_SIZE=$(printf "%d" 0x$(grep "^mtd${MTD}:" /proc/mtd | awk '{print $2}'))
echo "partition size: $PART_SIZE"
if [ "$SIZE" -gt "$PART_SIZE" ]; then
    echo "!!! image larger than partition - refusing"
    echo "##### FLASH ABORTED"
    exit 1
fi

echo
echo "--- erasing /dev/mtd$MTD ---"
flash_erase /dev/mtd$MTD 0 0 2>&1 | tail -3
echo "erase rc=$?"

echo
echo "--- writing ---"
nandwrite -p /dev/mtd$MTD "$IMG" 2>&1 | tail -5
echo "write rc=$?"

echo
echo "--- verifying (read back and compare) ---"
nanddump --omitoob -l "$SIZE" -f /tmp/readback.img /dev/mtd$MTD 2>&1 | tail -2
A=$(md5sum "$IMG" | cut -d' ' -f1)
B=$(md5sum /tmp/readback.img | cut -d' ' -f1)
echo "written : $A"
echo "readback: $B"
if [ "$A" = "$B" ]; then
    echo "VERIFY OK"
else
    echo "!!! VERIFY FAILED - restoring backup"
    flash_erase /dev/mtd$MTD 0 0 2>&1 | tail -2
    nandwrite -p /dev/mtd$MTD "$BACKUP" 2>&1 | tail -3
    echo "backup restored; NOT rebooting"
    echo "##### FLASH FAILED"
    exit 1
fi
rm -f /tmp/readback.img

echo
echo "##### FLASH OK - reboot required to take effect"
echo "##### FLASH COMPLETE"
