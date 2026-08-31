<script lang="ts">
    import { annotate_recording } from '../utils.svelte';
    import type { ManifestEntry } from '../manifest.svelte';
    import Explainer from './Explainer.svelte';

    /**
     * A name and notes for one recording.
     *
     * Recordings are named by the second they started, which says nothing
     * about why anyone made them. A folder of timestamps is unreadable a week
     * later, which is what EFForg/rayhunter#501 asked to fix.
     */
    let { entry }: { entry: ManifestEntry } = $props();

    /** Matches MAX_DISPLAY_NAME in the daemon. */
    const MAX_NAME = 29;
    /** Matches MAX_NOTES in the daemon. */
    const MAX_NOTES = 2000;

    let editing = $state(false);
    let name = $state('');
    let notes = $state('');
    let saving = $state(false);
    let error = $state('');

    function open() {
        name = entry.display_name ?? '';
        notes = entry.notes ?? '';
        error = '';
        editing = true;
    }

    /**
     * What the name will become once the daemon has cleaned it.
     *
     * Shown rather than silently applied, because a name coming back different
     * from what was typed is confusing if you never saw why. The rule is the
     * same one the daemon enforces: the name ends up in a downloaded filename,
     * so anything that is not a letter, digit, dash or underscore is replaced.
     */
    let cleaned = $derived(
        name
            .trim()
            .slice(0, MAX_NAME)
            .replace(/[^A-Za-z0-9_-]/g, '_')
            .replace(/^_+|_+$/g, '')
    );
    let willChange = $derived(cleaned !== name.trim() && name.trim().length > 0);

    async function save() {
        saving = true;
        error = '';
        try {
            await annotate_recording(entry.name, name.trim(), notes.trim());
            // Reflect it straight away rather than waiting for the next poll.
            entry.display_name = cleaned.length > 0 ? cleaned : null;
            entry.notes = notes.trim().length > 0 ? notes.trim() : null;
            editing = false;
        } catch (e) {
            error = `${e}`;
        } finally {
            saving = false;
        }
    }
</script>

{#if editing}
    <div class="mt-2 space-y-2 rounded-md border border-gray-300 p-2 dark:border-gray-600">
        <div>
            <label
                for="name-{entry.name}"
                class="block text-xs text-gray-600 dark:text-gray-300 mb-1"
            >
                Name
            </label>
            <input
                id="name-{entry.name}"
                type="text"
                bind:value={name}
                maxlength={MAX_NAME}
                placeholder="Leave empty to keep the timestamp"
                class="w-full rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
            />
            <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">
                {MAX_NAME - name.length} characters left.
                {#if willChange}
                    Will be saved as <span class="font-mono">{cleaned || '(empty)'}</span>, since a
                    name has to work as a filename.
                {/if}
            </p>
        </div>

        <div>
            <label
                for="notes-{entry.name}"
                class="block text-xs text-gray-600 dark:text-gray-300 mb-1"
            >
                Notes
            </label>
            <textarea
                id="notes-{entry.name}"
                bind:value={notes}
                maxlength={MAX_NOTES}
                rows="3"
                placeholder="Where you were, what you were doing, anything worth remembering"
                class="w-full rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
            ></textarea>
        </div>

        {#if error}
            <p class="text-xs text-red-600 dark:text-red-400">{error}</p>
        {/if}

        <div class="flex gap-2">
            <button
                type="button"
                onclick={save}
                disabled={saving}
                class="rounded-md border border-gray-300 px-2 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
            >
                {saving ? 'Saving…' : 'Save'}
            </button>
            <button
                type="button"
                onclick={() => (editing = false)}
                disabled={saving}
                class="rounded-md px-2 py-1 text-sm underline disabled:opacity-40"
            >
                Cancel
            </button>
        </div>

        <Explainer summary="Where a name and notes are kept, and where they show up.">
            <p>
                Both are stored alongside the recording rather than inside it. A recording is
                evidence, so naming one never rewrites it, and the capture you download is byte for
                byte what the device wrote.
            </p>
            <p>
                The name is also used for the downloaded zip, with the timestamp kept on the end so
                two recordings with the same name cannot collide.
            </p>
        </Explainer>
    </div>
{:else}
    <div class="mt-1 flex flex-wrap items-baseline gap-2">
        {#if entry.notes}
            <p class="w-full text-xs whitespace-pre-wrap text-gray-600 dark:text-gray-300">
                {entry.notes}
            </p>
        {/if}
        <button type="button" onclick={open} class="text-xs text-rayhunter-blue underline">
            {entry.display_name || entry.notes ? 'Edit name and notes' : 'Add a name and notes'}
        </button>
    </div>
{/if}
