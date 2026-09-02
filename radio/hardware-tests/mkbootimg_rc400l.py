#!/usr/bin/env python3
"""Repack an Orbic RC400L boot image around a replacement kernel.

The RC400L's mtd8 "boot" is a standard Android boot image: a 4096-byte header,
then the zImage, then (unused here) ramdisk and second-stage areas, then a
Qualcomm device-tree blob. This device boots with `noinitrd` and its rootfs on
UBI, so there is no ramdisk to preserve.

Everything except the kernel is copied from the original image: load addresses,
page size, command line, and the device-tree blob verbatim. Getting any of
those wrong produces an image that loads and then hangs, which on this device
is the expensive failure mode — aboot has no software route into fastboot, so
recovery means a physical key combination.

Usage:
  mkbootimg_rc400l.py --orig mtd8_boot.img --kernel zImage --out newboot.img
  mkbootimg_rc400l.py --orig mtd8_boot.img --info
"""

import argparse
import hashlib
import struct
import sys

HEADER_FMT = "<8s10I16s512s32s1024s"
MAGIC = b"ANDROID!"


def pages(n, page):
    return (n + page - 1) // page


def parse(data):
    if data[:8] != MAGIC:
        raise SystemExit("not an Android boot image (bad magic)")
    (magic, ksize, kaddr, rsize, raddr, ssize, saddr, tags, page, dtsize,
     osver, name, cmdline, ident, extra) = struct.unpack_from(HEADER_FMT, data, 0)
    off_k = page
    off_r = off_k + pages(ksize, page) * page
    off_s = off_r + pages(rsize, page) * page
    off_d = off_s + pages(ssize, page) * page
    return {
        "ksize": ksize, "kaddr": kaddr,
        "rsize": rsize, "raddr": raddr,
        "ssize": ssize, "saddr": saddr,
        "tags": tags, "page": page, "dtsize": dtsize, "osver": osver,
        "name": name, "cmdline": cmdline, "extra": extra,
        "kernel": data[off_k:off_k + ksize],
        "ramdisk": data[off_r:off_r + rsize],
        "second": data[off_s:off_s + ssize],
        "dt": data[off_d:off_d + dtsize],
        "used": off_d + pages(dtsize, page) * page,
    }


def build(orig, kernel):
    """Rebuild with a new kernel, preserving every other field."""
    page = orig["page"]
    ksize = len(kernel)

    # The id field is a SHA1 over each section and its length, in order. Not
    # every bootloader checks it, but a stale hash is free to get wrong and
    # cheap to get right.
    sha = hashlib.sha1()
    for blob in (kernel, orig["ramdisk"], orig["second"]):
        sha.update(blob)
        sha.update(struct.pack("<I", len(blob)))
    if orig["dtsize"]:
        sha.update(orig["dt"])
        sha.update(struct.pack("<I", orig["dtsize"]))
    ident = sha.digest().ljust(32, b"\0")

    header = struct.pack(
        HEADER_FMT, MAGIC,
        ksize, orig["kaddr"],
        orig["rsize"], orig["raddr"],
        orig["ssize"], orig["saddr"],
        orig["tags"], page, orig["dtsize"], orig["osver"],
        orig["name"], orig["cmdline"], ident, orig["extra"],
    )

    out = bytearray()
    out += header.ljust(page, b"\0")
    out += kernel.ljust(pages(ksize, page) * page, b"\0")
    out += orig["ramdisk"].ljust(pages(orig["rsize"], page) * page, b"\0")
    out += orig["second"].ljust(pages(orig["ssize"], page) * page, b"\0")
    out += orig["dt"].ljust(pages(orig["dtsize"], page) * page, b"\0")
    return bytes(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--orig", required=True, help="original boot partition dump")
    ap.add_argument("--kernel", help="replacement zImage")
    ap.add_argument("--out", help="output image")
    ap.add_argument("--info", action="store_true", help="describe the original and exit")
    ap.add_argument("--partition-size", type=int, default=0x7C0000,
                    help="mtd8 size; the image must fit (default 8126464)")
    args = ap.parse_args()

    orig = parse(open(args.orig, "rb").read())

    if args.info or not args.kernel:
        print(f"kernel   : {orig['ksize']} bytes @ 0x{orig['kaddr']:08x}")
        print(f"ramdisk  : {orig['rsize']} bytes @ 0x{orig['raddr']:08x}")
        print(f"second   : {orig['ssize']} bytes @ 0x{orig['saddr']:08x}")
        print(f"tags     : 0x{orig['tags']:08x}")
        print(f"page     : {orig['page']}")
        print(f"dt       : {orig['dtsize']} bytes")
        print(f"cmdline  : {orig['cmdline'].rstrip(chr(0).encode()).decode()}")
        print(f"used     : {orig['used']} of {args.partition_size} bytes")
        return

    kernel = open(args.kernel, "rb").read()
    if kernel[:2] == b"\x1f\x8b":
        raise SystemExit("that looks like a gzipped Image, not a zImage")

    img = build(orig, kernel)
    if len(img) > args.partition_size:
        raise SystemExit(
            f"image is {len(img)} bytes, larger than the {args.partition_size}-byte partition")

    open(args.out, "wb").write(img)
    grew = len(kernel) - orig["ksize"]
    print(f"wrote {args.out}: {len(img)} bytes "
          f"({len(img) * 100 // args.partition_size}% of partition)")
    print(f"kernel {orig['ksize']} -> {len(kernel)} bytes ({grew:+d})")
    print(f"device tree preserved: {orig['dtsize']} bytes")
    print(f"cmdline preserved    : {orig['cmdline'].rstrip(chr(0).encode()).decode()[:60]}...")


if __name__ == "__main__":
    sys.exit(main())
