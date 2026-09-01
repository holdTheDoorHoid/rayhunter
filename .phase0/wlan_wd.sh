#!/bin/sh
# Phase 0 safety watchdog: restore the vendor Wi-Fi configuration if a
# monitor-mode experiment leaves the driver in a non-serving state.
# Cancelled by creating /tmp/wd_cancel.
sleep "${1:-900}"
if [ ! -f /tmp/wd_cancel ]; then
    echo "wlan watchdog: restoring AP mode" > /dev/kmsg
    /etc/init.d/wlan stop
    sleep 2
    /etc/init.d/wlan start ap,ap
fi
