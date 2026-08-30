<script lang="ts">
    import { trigger_demo_warning } from '../utils.svelte';

    let busy = $state(false);
    let message = $state('');
    let failed = $state(false);

    async function fire() {
        busy = true;
        message = '';
        failed = false;
        try {
            await trigger_demo_warning();
            message = 'Demo warning injected. It will appear in the history within a few seconds.';
        } catch (e) {
            failed = true;
            message = `${e}`;
        } finally {
            busy = false;
        }
    }
</script>

<!-- Deliberately styled as a demo tool rather than as part of the interface.
     Someone glancing at a screenshot should be able to tell this device is
     being demonstrated rather than reporting a real detection. -->
<div
    class="rounded-md border border-dashed border-amber-500 bg-amber-50 p-3 dark:border-amber-600 dark:bg-amber-950"
>
    <div class="flex flex-wrap items-center justify-between gap-2">
        <div>
            <div class="text-sm font-medium text-amber-800 dark:text-amber-300">
                Demonstration mode
            </div>
            <p class="text-xs text-amber-700 dark:text-amber-400">
                Fakes a surveillance detection for showing people how Rayhunter reacts.
            </p>
        </div>
        <button
            type="button"
            onclick={fire}
            disabled={busy}
            class="rounded-md bg-amber-600 px-3 py-2 text-sm font-medium text-white hover:bg-amber-700 disabled:opacity-60"
        >
            {busy ? 'Injecting…' : 'Simulate a detection'}
        </button>
    </div>

    {#if message}
        <p
            class="mt-2 text-xs {failed
                ? 'text-red-600 dark:text-red-400'
                : 'text-amber-700 dark:text-amber-400'}"
        >
            {message}
        </p>
    {/if}

    <p class="mt-2 text-xs text-amber-700 dark:text-amber-400">
        This writes a clearly labelled fake message into the current recording, which then goes
        through the real detectors. Do not treat a recording containing demo data as evidence, or
        send it to EFF.
    </p>
</div>
