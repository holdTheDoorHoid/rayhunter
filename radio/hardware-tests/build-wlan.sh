#!/bin/bash
# Build qcacld-2.0 (wlan.ko) for the Orbic RC400L's QCA9377.
#
# Pairing, established by matching the shipped artefacts:
#   kernel  msm-3.18 @ caf_migration/LE.UM.1.3.r1.8   -> 3.18.48 (device's uname)
#   driver  qcacld-2.0 @ caf_migration/LE.UM.1.3.r1.1 -> 4.0.11.205G (device's driver)
#
# The device kernel has CONFIG_MODVERSIONS=n and CONFIG_MODULE_SIG=n, so the
# only load-time gate is the vermagic string. .scmversion is present in the
# kernel tree to keep UTS_RELEASE at a bare "3.18.48".
#
# Host workarounds: the CAF build calls a python2 gcc wrapper (bypassed with
# CC=), and the 2018-vintage toolchain wants libmpfr.so.4 (shimmed via
# LD_LIBRARY_PATH).

set -u

ROOT=/home/hoid/Desktop/qcadriver
KDIR=$ROOT/msm-3.18
WLAN=$ROOT/qcacld-2.0
TC=$ROOT/toolchain/armv7-eabihf--glibc--stable-2018.02-1/bin
COMPAT=$ROOT/toolchain/compat/usr/lib/x86_64-linux-gnu

export PATH="$TC:$PATH"
export LD_LIBRARY_PATH="$COMPAT:${LD_LIBRARY_PATH:-}"
export ARCH=arm
export CROSS_COMPILE=arm-linux-

# gcc 6.4 is newer than this tree expects; demote the resulting noise rather
# than letting -Werror stop a build that is otherwise fine.
EXTRA_CFLAGS_QUIET="-Wno-error -Wno-misleading-indentation -Wno-shift-negative-value \
-Wno-unused-const-variable -Wno-implicit-fallthrough -Wno-format-truncation \
-Wno-format-overflow -Wno-maybe-uninitialized -Wno-unused-variable \
-Wno-unused-function -Wno-sizeof-pointer-memaccess -Wno-bool-operation \
-Wno-int-in-bool-context -Wno-memset-elt-size -Wno-stringop-overflow \
-Wno-array-bounds -Wno-address-of-packed-member -Wno-nonnull-compare \
-Wno-sizeof-array-argument -Wno-frame-address"

# NOTE: use KCFLAGS, which *appends* to KBUILD_CFLAGS. Overriding
# KBUILD_CFLAGS_MODULE instead drops the kernel's own -DMODULE, which silently
# compiles every MODULE_INFO() away — the module then builds fine but carries
# no vermagic and refuses to load.
make -C "$KDIR" M="$WLAN" modules \
    CC=arm-linux-gcc \
    HOSTCFLAGS="-fcommon -w" \
    KCFLAGS="$EXTRA_CFLAGS_QUIET" \
    WLAN_ROOT="$WLAN" \
    MODNAME=wlan \
    WLAN_OPEN_SOURCE=1 \
    CONFIG_QCA_CLD_WLAN=m \
    CONFIG_QCA_WIFI_ISOC=0 \
    CONFIG_QCA_WIFI_2_0=1 \
    CONFIG_CLD_HL_SDIO_CORE=y \
    KBUILD_EXTRA_SYMBOLS="" \
    "$@" 2>&1
