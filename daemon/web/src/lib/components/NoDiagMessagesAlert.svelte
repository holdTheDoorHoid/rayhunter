<script lang="ts">
    import type { ManifestEntry } from '$lib/manifest.svelte';
    import { get_daemon_time } from '$lib/utils.svelte';
    import { diag_silence_minutes } from '$lib/cellInfo';

    let { entry }: { entry: ManifestEntry | undefined } = $props();
    let daemon_adjusted_time = $state<Date | undefined>(undefined);

    // Judged against the device's own clock, not the browser's: the recording
    // timestamps come from the device, and the two clocks need not agree.
    let silent_for = $derived(
        entry && daemon_adjusted_time
            ? diag_silence_minutes(
                  entry.last_message_time ?? entry.start_time,
                  daemon_adjusted_time
              )
            : null
    );

    async function update_daemon_time() {
        try {
            const response = await get_daemon_time();
            daemon_adjusted_time = new Date(response.adjusted_time);
        } catch (err) {
            console.error('Failed to check diagnostic message activity:', err);
        }
    }

    $effect(() => {
        update_daemon_time();
        const interval = setInterval(update_daemon_time, 30_000);
        return () => clearInterval(interval);
    });
</script>

{#if silent_for !== null}
    <div
        class="mt-2 rounded-md bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-950 dark:text-amber-200"
    >
        <span class="font-medium">Nothing from the modem for {silent_for} minutes</span>
        <span class="block text-xs opacity-90">
            A recording is running, but the modem has not sent Rayhunter a single diagnostic message
            in that time, so it may not be capturing anything. This clears on its own when messages
            resume. If it does not, stop and start the recording; if that does not help either,
            power the device off and on.
        </span>
    </div>
{/if}
