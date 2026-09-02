<script lang="ts">
    import { ManifestEntry, type RecordingSidecar } from '$lib/manifest.svelte';

    let { entry }: { entry: ManifestEntry } = $props();

    let visible = $state(false);
    let sidecar: RecordingSidecar | null | undefined = $state(undefined);
    let failed = $state(false);

    function readable_bytes(bytes: number): string {
        const units = ['B', 'KiB', 'MiB', 'GiB'];
        let value = bytes;
        let i = 0;
        while (value >= 1024 && i < units.length - 1) {
            value /= 1024;
            i++;
        }
        return `${value.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
    }

    const date_formatter = new Intl.DateTimeFormat(undefined, {
        timeStyle: 'long',
        dateStyle: 'short',
    });

    async function toggle() {
        visible = !visible;
        if (visible && sidecar === undefined) {
            try {
                const response = await fetch(entry.get_metadata_url());
                if (response.status === 404) {
                    sidecar = null;
                } else if (!response.ok) {
                    failed = true;
                } else {
                    sidecar = (await response.json()) as RecordingSidecar;
                }
            } catch {
                failed = true;
            }
        }
    }

    function device_line(s: RecordingSidecar): string {
        const parts = [s.hardware.device];
        if (s.hardware.model) parts.push(s.hardware.model);
        if (s.hardware.hardware_version) parts.push(s.hardware.hardware_version);
        if (s.hardware.soc) parts.push(`(${s.hardware.soc})`);
        return parts.join(' ');
    }

    function software_line(s: RecordingSidecar): string {
        const parts = [`Rayhunter ${s.software.rayhunter_version}`];
        if (s.software.system_os || s.software.arch) {
            parts.push(`on ${s.software.system_os} ${s.software.arch}`.trim());
        }
        if (s.hardware.firmware_build) parts.push(`firmware ${s.hardware.firmware_build}`);
        return parts.join(', ');
    }

    function clock_line(s: RecordingSidecar): string {
        if (!s.clock.system_time_at_start) return 'not recorded';
        const raw = date_formatter.format(new Date(s.clock.system_time_at_start));
        const offset = s.clock.offset_seconds_at_start;
        let text =
            offset === 0
                ? `${raw}, uncorrected`
                : `${raw}, corrected by ${offset > 0 ? '+' : ''}${offset} s`;
        if (s.clock.offset_changed_during_recording) {
            text += ' (the correction changed while recording)';
        }
        return text;
    }

    function home_network_line(s: RecordingSidecar): string {
        if (s.home_plmn.length > 0) return s.home_plmn.join(', ');
        if (s.redacted_fields?.includes('home_plmn')) return 'removed from this copy';
        return 'unknown (SIM not read)';
    }

    function storage_line(s: RecordingSidecar): string {
        if (s.resources.disk_available_bytes === undefined) return 'not recorded';
        let text = `${readable_bytes(s.resources.disk_available_bytes)} free`;
        if (s.resources.disk_total_bytes !== undefined) {
            text += ` of ${readable_bytes(s.resources.disk_total_bytes)}`;
        }
        if (s.resources.storage_path) text += ` at ${s.resources.storage_path}`;
        return text;
    }

    function memory_line(s: RecordingSidecar): string {
        if (s.resources.memory_available_kb === undefined) return 'not recorded';
        let text = `${readable_bytes(s.resources.memory_available_kb * 1024)} available`;
        if (s.resources.memory_total_kb !== undefined) {
            text += ` of ${readable_bytes(s.resources.memory_total_kb * 1024)}`;
        }
        return text;
    }

    function wifi_line(s: RecordingSidecar): string {
        if (!s.wifi) {
            return s.redacted_fields?.includes('wifi') ? 'removed from this copy' : 'not recorded';
        }
        if (!s.wifi.client_enabled) return 'off';
        return s.wifi.connected_network
            ? `${s.wifi.client_state}, on ${s.wifi.connected_network}`
            : s.wifi.client_state;
    }
</script>

<div class="flex flex-col gap-1 text-sm">
    <button
        class="text-left text-blue-700 dark:text-blue-300 hover:underline w-fit"
        onclick={toggle}
        aria-expanded={visible}
    >
        {visible ? 'Hide device details' : 'Device details'}
    </button>
    {#if visible}
        {#if failed}
            <span class="text-red-700 dark:text-red-300">Couldn't load the device details.</span>
        {:else if sidecar === undefined}
            <span class="text-gray-600 dark:text-gray-400">Loading…</span>
        {:else if sidecar === null}
            <span class="text-gray-600 dark:text-gray-400">
                No device details were saved with this recording. It was made before Rayhunter
                started saving them.
            </span>
        {:else}
            <dl class="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-0.5">
                <dt class="font-semibold">Device</dt>
                <dd>{device_line(sidecar)}</dd>
                <dt class="font-semibold">Software</dt>
                <dd>{software_line(sidecar)}</dd>
                <dt class="font-semibold">Home network</dt>
                <dd>{home_network_line(sidecar)}</dd>
                <dt class="font-semibold">Device clock</dt>
                <dd>{clock_line(sidecar)}</dd>
                <dt class="font-semibold">Storage</dt>
                <dd>{storage_line(sidecar)}</dd>
                <dt class="font-semibold">Memory</dt>
                <dd>{memory_line(sidecar)}</dd>
                <dt class="font-semibold">WiFi client</dt>
                <dd>{wifi_line(sidecar)}</dd>
            </dl>
            <span class="text-gray-600 dark:text-gray-400">
                Saved beside the recording as {sidecar.recording}-meta.json and included in the zip.
                The shareable zip leaves out the home network and WiFi details.
            </span>
        {/if}
    {/if}
</div>
