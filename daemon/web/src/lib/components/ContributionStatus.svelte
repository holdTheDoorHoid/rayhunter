<script lang="ts">
    import type { ManifestEntry } from '../manifest.svelte';
    import { telemetry_set_excluded, telemetry_withdraw } from '../utils.svelte';

    /**
     * Whether this recording went to a community dataset, and the two things
     * an owner can do about it: keep it out, or take it back.
     */
    let { entry }: { entry: ManifestEntry } = $props();

    let busy = $state(false);
    let message = $state('');

    const date_formatter = new Intl.DateTimeFormat(undefined, {
        timeStyle: 'short',
        dateStyle: 'short',
    });

    async function withdraw() {
        if (
            !confirm(
                'Ask the service to delete this contribution? It will not be sent again. This cannot be undone from here.'
            )
        ) {
            return;
        }
        busy = true;
        message = '';
        try {
            await telemetry_withdraw(entry.name);
            if (entry.telemetry_submission) {
                entry.telemetry_submission = {
                    ...entry.telemetry_submission,
                    withdrawn_at: new Date().toISOString(),
                };
            }
            message = 'Withdrawn.';
        } catch (e) {
            message = `${e}`;
        } finally {
            busy = false;
        }
    }

    async function toggle_excluded() {
        busy = true;
        message = '';
        try {
            await telemetry_set_excluded(entry.name, !entry.telemetry_excluded);
            entry.telemetry_excluded = !entry.telemetry_excluded;
        } catch (e) {
            message = `${e}`;
        } finally {
            busy = false;
        }
    }

    const linkClass = 'text-blue-700 dark:text-blue-300 hover:underline disabled:opacity-50';
</script>

<div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
    {#if entry.telemetry_submission}
        {#if entry.telemetry_submission.withdrawn_at}
            <span class="text-gray-600 dark:text-gray-400">
                Contribution withdrawn {date_formatter.format(
                    new Date(entry.telemetry_submission.withdrawn_at)
                )}.
            </span>
        {:else}
            <span>
                Contributed ({entry.telemetry_submission.tier}) on {date_formatter.format(
                    new Date(entry.telemetry_submission.submitted_at)
                )}
                <span class="text-gray-600 dark:text-gray-400 font-mono text-xs"
                    >{entry.telemetry_submission.submission_id.slice(0, 8)}</span
                >
            </span>
            <button type="button" class={linkClass} onclick={withdraw} disabled={busy}>
                Withdraw
            </button>
        {/if}
    {:else}
        <span class="text-gray-600 dark:text-gray-400">
            {entry.telemetry_excluded
                ? 'Kept out of the community dataset.'
                : 'Not contributed to a community dataset.'}
        </span>
        <button type="button" class={linkClass} onclick={toggle_excluded} disabled={busy}>
            {entry.telemetry_excluded ? 'Allow contributing' : 'Never contribute this one'}
        </button>
    {/if}
    {#if message}
        <span class="text-gray-700 dark:text-gray-300">{message}</span>
    {/if}
</div>
