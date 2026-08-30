<script lang="ts">
    import {
        set_display_gif,
        delete_display_gif,
        MAX_GIF_BYTES,
        DEVICE_SCREEN_PX,
        type Config,
        type DisplayColorKey,
    } from '../utils.svelte';

    let { config = $bindable() }: { config: Config } = $props();

    interface StateRow {
        key: DisplayColorKey;
        label: string;
        description: string;
    }

    const rows: StateRow[] = [
        { key: 'paused', label: 'Paused', description: 'Recording is stopped' },
        { key: 'recording', label: 'Recording', description: 'Recording normally, no warnings' },
        { key: 'warning_low', label: 'Low warning', description: 'A low-severity heuristic fired' },
        {
            key: 'warning_medium',
            label: 'Medium warning',
            description: 'A medium-severity heuristic fired',
        },
        {
            key: 'warning_high',
            label: 'High warning',
            description: 'A high-severity heuristic fired',
        },
    ];

    let busy = $state<Record<string, boolean>>({});
    let errors = $state<Record<string, string>>({});
    let notices = $state<Record<string, string>>({});
    // Bumped after an upload so the preview refetches. The URL is stable per
    // state, so without this the browser would keep showing the replaced GIF.
    let versions = $state<Record<string, number>>({});
    // States whose stored GIF could not be fetched, so a broken image is
    // replaced by an honest label rather than a browser placeholder.
    let unfetchable = $state<Record<string, boolean>>({});

    function preview_url(key: DisplayColorKey): string {
        const v = versions[key] ?? 0;
        return `/api/display-gif/${key}?v=${v}`;
    }

    function human_size(bytes: number): string {
        return bytes < 1024 * 1024
            ? `${Math.round(bytes / 1024)} KB`
            : `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    }

    /**
     * Check a GIF locally before sending it, so obvious problems are reported
     * instantly instead of after a slow upload over the device's WiFi.
     */
    function inspect(file: File): Promise<{ error?: string; notice?: string }> {
        return new Promise((resolve) => {
            if (file.size > MAX_GIF_BYTES) {
                resolve({
                    error: `That GIF is ${human_size(file.size)}. The limit is ${human_size(MAX_GIF_BYTES)} — the device has very little memory.`,
                });
                return;
            }
            const img = new Image();
            const url = URL.createObjectURL(file);
            img.onload = () => {
                URL.revokeObjectURL(url);
                const { naturalWidth: w, naturalHeight: h } = img;
                if (w === DEVICE_SCREEN_PX && h === DEVICE_SCREEN_PX) {
                    resolve({});
                } else if (w !== h) {
                    resolve({
                        notice: `This GIF is ${w}×${h}. The device screen is square, so it will be scaled to fit and may look stretched or letterboxed. ${DEVICE_SCREEN_PX}×${DEVICE_SCREEN_PX} works best.`,
                    });
                } else {
                    resolve({
                        notice: `This GIF is ${w}×${h} and will be scaled to ${DEVICE_SCREEN_PX}×${DEVICE_SCREEN_PX}. Fine detail may be lost.`,
                    });
                }
            };
            img.onerror = () => {
                URL.revokeObjectURL(url);
                resolve({ error: 'That file could not be read as an image.' });
            };
            img.src = url;
        });
    }

    async function upload(key: DisplayColorKey, event: Event) {
        const input = event.currentTarget as HTMLInputElement;
        const file = input.files?.[0];
        if (!file) return;

        errors[key] = '';
        notices[key] = '';

        if (!file.type.includes('gif') && !file.name.toLowerCase().endsWith('.gif')) {
            errors[key] = 'Only GIF files can be used as animations.';
            input.value = '';
            return;
        }

        const { error, notice } = await inspect(file);
        if (error) {
            errors[key] = error;
            input.value = '';
            return;
        }
        if (notice) notices[key] = notice;

        busy[key] = true;
        try {
            await set_display_gif(key, file);
            config.display_gifs[key] = `${key}.gif`;
            unfetchable[key] = false;
            versions[key] = (versions[key] ?? 0) + 1;
        } catch (e) {
            errors[key] = `Upload failed: ${e}`;
        } finally {
            busy[key] = false;
            input.value = '';
        }
    }

    async function remove(key: DisplayColorKey) {
        busy[key] = true;
        errors[key] = '';
        notices[key] = '';
        try {
            await delete_display_gif(key);
            config.display_gifs[key] = null;
            unfetchable[key] = false;
        } catch (e) {
            errors[key] = `Could not remove: ${e}`;
        } finally {
            busy[key] = false;
        }
    }
</script>

<div class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-3">
    <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4">Device Display GIFs</h3>

    <p class="text-xs text-gray-500 dark:text-gray-400">
        Play your own animation on the device for each state. The screen is {DEVICE_SCREEN_PX}×{DEVICE_SCREEN_PX}
        pixels, so square GIFs of that size look best — anything larger is scaled down on the device.
        Any state without a GIF falls back to showing its colored status line instead, so the device is
        never blank. Uploaded GIFs are stored right away, but only take effect once you save this form.
    </p>

    <p class="text-xs text-gray-500 dark:text-gray-400">
        Warnings interrupt a playing animation immediately, so a long GIF will never delay an alert.
    </p>

    <div class="space-y-3">
        {#each rows as row (row.key)}
            {@const uploaded = config.display_gifs[row.key] !== null}
            <div class="flex items-start gap-3">
                <div
                    class="flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded-sm bg-black"
                >
                    {#if uploaded && !unfetchable[row.key]}
                        <img
                            src={preview_url(row.key)}
                            alt="Animation currently set for {row.label}"
                            class="max-h-full max-w-full object-contain"
                            onerror={() => (unfetchable[row.key] = true)}
                        />
                    {:else if uploaded}
                        <span class="text-[10px] text-gray-400 dark:text-gray-500">stored</span>
                    {:else}
                        <span class="text-[10px] text-gray-600 dark:text-gray-300">none</span>
                    {/if}
                </div>

                <div class="min-w-0 flex-1">
                    <div class="block text-sm font-medium text-gray-700 dark:text-gray-200">
                        {row.label}
                    </div>
                    <p class="text-xs text-gray-500 dark:text-gray-400">{row.description}</p>

                    <div class="mt-1 flex flex-wrap items-center gap-3">
                        <label
                            class="cursor-pointer text-xs text-rayhunter-blue underline {busy[
                                row.key
                            ]
                                ? 'pointer-events-none text-gray-300 dark:text-gray-600'
                                : ''}"
                        >
                            {busy[row.key] ? 'Uploading…' : uploaded ? 'Replace GIF' : 'Choose GIF'}
                            <input
                                type="file"
                                accept="image/gif,.gif"
                                class="hidden"
                                disabled={busy[row.key]}
                                onchange={(e) => upload(row.key, e)}
                            />
                        </label>
                        {#if uploaded}
                            <button
                                type="button"
                                onclick={() => remove(row.key)}
                                disabled={busy[row.key]}
                                class="text-xs text-rayhunter-blue underline disabled:text-gray-300 dark:disabled:text-gray-600 disabled:no-underline"
                            >
                                Remove
                            </button>
                        {/if}
                    </div>

                    {#if errors[row.key]}
                        <p class="mt-1 text-xs text-red-600 dark:text-red-400">{errors[row.key]}</p>
                    {/if}
                    {#if notices[row.key]}
                        <p class="mt-1 text-xs text-amber-600 dark:text-amber-400">
                            {notices[row.key]}
                        </p>
                    {/if}
                </div>
            </div>
        {/each}
    </div>
</div>
