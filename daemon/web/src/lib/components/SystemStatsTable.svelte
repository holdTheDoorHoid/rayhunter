<script lang="ts">
    import {
        type SystemStats,
        cpu_state,
        format_uptime,
        hours_until_full,
        format_duration_hours,
    } from '$lib/systemStats';
    import Explainer from './Explainer.svelte';
    import { gps_mode_label, GpsMode, type GpsData } from '$lib/utils.svelte';
    let {
        stats,
        gps_data = null,
        gps_mode = GpsMode.Disabled,
    }: {
        stats: SystemStats;
        gps_data?: GpsData | null;
        gps_mode?: GpsMode;
    } = $props();

    // Recording growth, sampled across polls. A single reading says nothing
    // about rate, so this keeps the oldest sample it has and measures against
    // it rather than guessing from one number.
    let firstSample: { at: number; free: number } | null = $state(null);
    let bytesPerSecond: number | null = $state(null);

    $effect(() => {
        const free = stats.disk_stats.available_bytes;
        if (free === undefined) return;
        const at = Date.now();
        if (!firstSample) {
            firstSample = { at, free };
            return;
        }
        const seconds = (at - firstSample.at) / 1000;
        // Below a minute the numbers are noise. A device that gained space,
        // because a recording rotated or was deleted, restarts the measurement
        // rather than reporting a negative rate.
        if (seconds < 60) return;
        const consumed = firstSample.free - free;
        if (consumed < 0) {
            firstSample = { at, free };
            bytesPerSecond = null;
            return;
        }
        bytesPerSecond = consumed / seconds;
    });

    let hoursLeft = $derived(
        bytesPerSecond !== null
            ? hours_until_full(bytesPerSecond, stats.disk_stats.available_bytes)
            : null
    );
    let cpuLabel = $derived(stats.health ? cpu_state(stats.health) : null);
    let cpuClass = $derived(
        cpuLabel === 'overloaded'
            ? 'text-red-600 dark:text-red-400'
            : cpuLabel === 'stretched'
              ? 'text-amber-600 dark:text-amber-400'
              : ''
    );

    let battery_level = $derived(stats.battery_status ? stats.battery_status.level : 0);
    let bar_color = $derived.by(() => {
        if (stats.battery_status === undefined) {
            return '';
        }
        if (battery_level <= 10) {
            return 'fill-red-500';
        }
        if (battery_level <= 25) {
            return 'fill-yellow-300';
        }
        return 'fill-green-500';
    });
    let title_text = $derived.by(() => {
        if (stats.battery_status === undefined) {
            return 'Rayhunter does not yet support displaying the battery level for this device.';
        }

        let text = `Battery is ${stats.battery_status.level}% full`;

        if (stats.battery_status.is_plugged_in) {
            text += ' and plugged in';
        }
        return text;
    });
</script>

<div
    class="flex-1 drop-shadow-sm p-4 flex flex-col gap-2 border rounded-md bg-gray-100 dark:bg-gray-800 border-gray-100 dark:border-gray-800"
>
    <p class="text-xl mb-2">System Information</p>
    <table class="text-sm w-full">
        <tbody>
            <tr class="border-b border-gray-200 dark:border-gray-700">
                <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium"
                    >Rayhunter Version</td
                >
                <td class="py-1">{stats.runtime_metadata.rayhunter_version}</td>
            </tr>
            <tr class="border-b border-gray-200 dark:border-gray-700">
                <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium">Storage</td>
                <td class="py-1">
                    {stats.disk_stats.used_percent} used ({stats.disk_stats.used_size} used / {stats
                        .disk_stats.available_size} available)
                </td>
            </tr>
            {#if hoursLeft !== null}
                <tr class="border-b border-gray-200 dark:border-gray-700">
                    <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium"
                        >Recording room</td
                    >
                    <td class="py-1">
                        about {format_duration_hours(hoursLeft)} left at the current rate
                    </td>
                </tr>
            {/if}
            {#if stats.health}
                <tr class="border-b border-gray-200 dark:border-gray-700">
                    <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium">Processor</td
                    >
                    <td class="py-1 {cpuClass}">
                        {#if stats.health.cpu_busy_percent !== undefined}
                            {cpuLabel}
                            <span class="text-gray-500 dark:text-gray-400">
                                ({stats.health.cpu_busy_percent.toFixed(0)}% in use{#if stats.health.rayhunter_cpu_percent !== undefined},
                                    {stats.health.rayhunter_cpu_percent.toFixed(0)}% Rayhunter{/if})
                            </span>
                        {:else}
                            measuring
                        {/if}
                    </td>
                </tr>
                <tr class="border-b border-gray-200 dark:border-gray-700">
                    <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium">Uptime</td>
                    <td class="py-1">{format_uptime(stats.health.uptime_secs)}</td>
                </tr>
                {#if stats.health.cpu_temp_c !== undefined || stats.health.radio_temp_c !== undefined}
                    <tr class="border-b border-gray-200 dark:border-gray-700">
                        <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium"
                            >Temperature</td
                        >
                        <td class="py-1">
                            {#if stats.health.cpu_temp_c !== undefined}
                                {stats.health.cpu_temp_c.toFixed(0)}&deg;C processor{/if}{#if stats.health.cpu_temp_c !== undefined && stats.health.radio_temp_c !== undefined},
                            {/if}{#if stats.health.radio_temp_c !== undefined}
                                {stats.health.radio_temp_c.toFixed(0)}&deg;C radio{/if}
                        </td>
                    </tr>
                {/if}
            {/if}
            <tr class="border-b border-gray-200 dark:border-gray-700">
                <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium">Memory (RAM)</td>
                <td class="py-1">
                    Free: {stats.memory_stats.free}, Used: {stats.memory_stats.used}
                </td>
            </tr>
            <tr
                class={gps_mode !== GpsMode.Disabled
                    ? 'border-b border-gray-200 dark:border-gray-700'
                    : ''}
            >
                <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium">Battery</td>
                <td class="py-1">
                    <svg
                        width="80"
                        height="30"
                        viewBox="0 0 80 30"
                        role="img"
                        xmlns="http://www.w3.org/2000/svg"
                        class="battery-icon"
                    >
                        <title>{title_text}</title>
                        <rect
                            class="fill-none stroke-neutral-800 stroke-2"
                            width="70"
                            height="30"
                            rx="3"
                            ry="3"
                        />
                        <rect
                            class="fill-neutral-800"
                            x="70"
                            y="7"
                            width="8"
                            height="16"
                            rx="2"
                            ry="2"
                        />
                        <rect
                            class={bar_color}
                            x="2"
                            y="2"
                            height="26"
                            rx="2"
                            ry="2"
                            style="width: {battery_level * 0.66}px;"
                        />
                        {#if stats.battery_status && stats.battery_status.is_plugged_in}
                            <path
                                class="fill-yellow-300 stroke-neutral-800 stroke-1"
                                d="M38 3 L28 17 L34 17 L30 27 L40 13 L34 13 Z"
                            />
                        {/if}
                        {#if !stats.battery_status}
                            <text
                                class="fill-neutral-500 text-[20px] font-bold [text-anchor:middle] [dominant-baseline:central]"
                                x="35"
                                y="15">?</text
                            >
                        {/if}
                    </svg>
                </td>
            </tr>
            {#if gps_mode !== GpsMode.Disabled}
                <tr class="border-b border-gray-200 dark:border-gray-700">
                    <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium">GPS Mode</td>
                    <td class="py-1">{gps_mode_label(gps_mode)}</td>
                </tr>
                {#if gps_data}
                    <tr class="border-b border-gray-200 dark:border-gray-700">
                        <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium"
                            >Latitude</td
                        >
                        <td class="py-1 font-mono">{gps_data.latitude.toFixed(6)}</td>
                    </tr>
                    <tr>
                        <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium"
                            >Longitude</td
                        >
                        <td class="py-1 font-mono">{gps_data.longitude.toFixed(6)}</td>
                    </tr>
                {:else}
                    <tr>
                        <td class="py-1 pr-4 text-gray-500 dark:text-gray-400 font-medium"
                            >GPS Data</td
                        >
                        <td class="py-1 text-gray-400 dark:text-gray-500">Awaiting GPS data...</td>
                    </tr>
                {/if}
            {/if}
        </tbody>
    </table>

    {#if stats.health}
        <div class="mt-2">
            <Explainer summary="What load, uptime and temperature tell you about this device.">
                <p>
                    <strong>Load</strong> is how much work is queued, measured against the
                    {stats.health.cpu_count === 1
                        ? 'single processor core this device has'
                        : `${stats.health.cpu_count} cores this device has`}. Below one per core the
                    device is keeping up. Above it, work is waiting, and a device that falls far
                    enough behind can drop radio messages it never gets back. That matters here more
                    than on an ordinary computer: a missed message looks exactly like a quiet night.
                </p>
                <p>
                    <strong>Uptime</strong> is how long the device has been running since it last booted.
                    Worth glancing at if you left it somewhere: an uptime shorter than you expect means
                    it restarted while you were away, and everything it would have seen during that gap
                    was not recorded.
                </p>
                <p>
                    <strong>Temperature</strong> covers the processor and, separately, the radio power
                    amplifiers. The radio figure tracks how hard the device is transmitting rather than
                    how busy it is thinking. Sustained high readings lead to throttling, which shows up
                    as the load climbing for no obvious reason.
                </p>
                <p>
                    <strong>Recording room</strong> is measured from how fast this recording is actually
                    growing, not from an assumed rate, so it appears a minute or so after the page opens
                    and reflects what the device is really capturing.
                </p>
            </Explainer>
        </div>
    {/if}
</div>
