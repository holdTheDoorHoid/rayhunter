<script lang="ts">
    import { onMount } from 'svelte';
    import { req_json } from '$lib/utils.svelte';
    import type { StorageCandidate } from '$lib/systemStats';

    /** The config's `removable_store_path`: null for internal storage only. */
    let { value = $bindable() }: { value: string | null } = $props();

    let candidates: StorageCandidate[] = $state([]);
    let failed = $state(false);
    // 'internal', a candidate path, or 'custom'.
    let choice: string = $state('internal');
    let custom_path = $state('');

    function readable(bytes: number | undefined): string {
        if (bytes === undefined) return '';
        const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
        let v = bytes;
        let i = 0;
        while (v >= 1024 && i < units.length - 1) {
            v /= 1024;
            i++;
        }
        return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
    }

    function label(c: StorageCandidate): string {
        const parts: string[] = [];
        if (c.kind === 'internal') parts.push('Internal storage');
        else if (c.kind === 'card') parts.push('Memory card');
        else parts.push('Removable storage');
        parts.push(c.path);
        const details: string[] = [];
        if (c.kind === 'internal' || c.mounted) {
            if (c.available_bytes !== undefined) {
                details.push(`${readable(c.available_bytes)} free`);
            }
            if (c.kind !== 'internal' && c.fstype) details.push(c.fstype.toUpperCase());
        } else {
            details.push('not present now');
        }
        let text = `${parts.join(' — ')}${details.length ? ` (${details.join(', ')})` : ''}`;
        if (value && value !== c.path && belongs_to(c, value)) {
            text += `, recordings in ${value}`;
        }
        return text;
    }

    /** Whether the configured path is the candidate itself or a directory inside it. */
    function belongs_to(c: StorageCandidate, path: string | null): boolean {
        if (!path || c.kind === 'internal') return false;
        return path === c.path || path.startsWith(c.path.replace(/\/+$/, '') + '/');
    }

    function choose(next: string) {
        choice = next;
        if (next === 'internal') value = null;
        else if (next === 'custom') value = custom_path.trim() || null;
        else if (!(value && belongs_to({ path: next, kind: 'card' } as StorageCandidate, value)))
            value = next;
    }

    function custom_changed() {
        if (choice === 'custom') value = custom_path.trim() || null;
    }

    onMount(async () => {
        try {
            candidates = await req_json<StorageCandidate[]>('GET', '/api/storage/candidates');
        } catch {
            failed = true;
        }
        // Reflect what the config already says.
        if (value) {
            const owner = candidates.find((c) => belongs_to(c, value));
            if (owner) {
                choice = owner.path;
            } else {
                choice = 'custom';
                custom_path = value;
            }
        } else {
            choice = 'internal';
        }
    });
</script>

<div class="space-y-2">
    <span class="block text-sm font-medium text-gray-700 dark:text-gray-200">
        Where recordings are stored
    </span>
    <p class="text-xs text-gray-500 dark:text-gray-400">
        Recordings go to internal storage unless you choose a memory card here. If the card is
        missing, they go to internal storage and move back to the card when it returns. You are told
        either way.
    </p>
    {#if failed}
        <p class="text-xs text-red-700 dark:text-red-300">
            Couldn't list the storage on this device; a custom path can still be set.
        </p>
    {/if}
    <div class="space-y-1">
        {#each candidates as c (c.path)}
            <label class="flex items-start gap-2 text-sm text-gray-700 dark:text-gray-200">
                <input
                    type="radio"
                    name="recording_storage"
                    class="mt-1"
                    checked={choice === (c.kind === 'internal' ? 'internal' : c.path)}
                    onchange={() => choose(c.kind === 'internal' ? 'internal' : c.path)}
                />
                <span>{label(c)}</span>
            </label>
        {/each}
        {#if candidates.length === 0}
            <label class="flex items-start gap-2 text-sm text-gray-700 dark:text-gray-200">
                <input
                    type="radio"
                    name="recording_storage"
                    class="mt-1"
                    checked={choice === 'internal'}
                    onchange={() => choose('internal')}
                />
                <span>Internal storage</span>
            </label>
        {/if}
        <label class="flex items-start gap-2 text-sm text-gray-700 dark:text-gray-200">
            <input
                type="radio"
                name="recording_storage"
                class="mt-1"
                checked={choice === 'custom'}
                onchange={() => choose('custom')}
            />
            <span>A path of my own</span>
        </label>
        {#if choice === 'custom'}
            <input
                type="text"
                placeholder="/media/card"
                bind:value={custom_path}
                oninput={custom_changed}
                class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue text-sm"
            />
            <p class="text-xs text-gray-500 dark:text-gray-400">
                Where the card is mounted. Rayhunter mounts the first SD card partition there itself
                when the system has not.
            </p>
        {/if}
    </div>
</div>
