<script lang="ts">
    import {
        DISPLAY_COLOR_DEFAULTS,
        default_recording_color,
        type Config,
        type DisplayColorKey,
    } from '../utils.svelte';

    let { config = $bindable() }: { config: Config } = $props();

    type LinePattern = 'solid' | 'dashed' | 'dotted';

    interface StateRow {
        key: DisplayColorKey;
        label: string;
        description: string;
        pattern: LinePattern;
    }

    const rows: StateRow[] = [
        {
            key: 'paused',
            label: 'Paused',
            description: 'Recording is stopped',
            pattern: 'solid',
        },
        {
            key: 'recording',
            label: 'Recording',
            description: 'Recording normally, no warnings',
            pattern: 'solid',
        },
        {
            key: 'warning_low',
            label: 'Low warning',
            description: 'A low-severity heuristic triggered',
            pattern: 'dotted',
        },
        {
            key: 'warning_medium',
            label: 'Medium warning',
            description: 'A medium-severity heuristic triggered',
            pattern: 'dashed',
        },
        {
            key: 'warning_high',
            label: 'High warning',
            description: 'A high-severity heuristic triggered',
            pattern: 'solid',
        },
    ];

    /** The built-in color for a state, honouring colorblind mode for `recording`. */
    function fallback_color(key: DisplayColorKey): string {
        return key === 'recording'
            ? default_recording_color(config.colorblind_mode)
            : DISPLAY_COLOR_DEFAULTS[key];
    }

    /** The color actually shown for a state: the override if set, else the built-in. */
    function effective_color(key: DisplayColorKey): string {
        return config.display_colors[key] ?? fallback_color(key);
    }

    function set_color(key: DisplayColorKey, value: string) {
        config.display_colors[key] = value;
    }

    function reset_color(key: DisplayColorKey) {
        config.display_colors[key] = null;
    }

    function reset_all() {
        for (const row of rows) {
            config.display_colors[row.key] = null;
        }
    }

    let any_customized = $derived(rows.some((row) => config.display_colors[row.key] !== null));

    /**
     * Reproduce the device's line pattern in CSS. The device draws a dashed
     * line as 4 pixels on / 4 off and a dotted line as 1 on / 3 off, scaled up
     * here so the pattern is visible on a normal screen.
     */
    function line_background(key: DisplayColorKey, pattern: LinePattern): string {
        const color = effective_color(key);
        switch (pattern) {
            case 'solid':
                return color;
            case 'dashed':
                return `repeating-linear-gradient(90deg, ${color} 0 8px, transparent 8px 16px)`;
            case 'dotted':
                return `repeating-linear-gradient(90deg, ${color} 0 2px, transparent 2px 8px)`;
        }
    }
</script>

<div class="border-t border-gray-200 pt-4 mt-6 space-y-3">
    <h3 class="text-lg font-semibold text-gray-800 mb-4">Device Display Colors</h3>

    <p class="text-xs text-gray-500">
        Choose the colors used for the status line on the device's own screen. States you leave
        unchanged keep Rayhunter's built-in color, including the green-to-blue switch from
        Colorblind Mode. These colors do not affect this page, and have no effect on devices with a
        one-bit display (such as the TP-Link M7350), which show status icons instead of a colored
        line.
    </p>

    <div class="space-y-3">
        {#each rows as row (row.key)}
            {@const customized = config.display_colors[row.key] !== null}
            <div class="flex items-center gap-3">
                <input
                    id="display_color_{row.key}"
                    type="color"
                    value={effective_color(row.key)}
                    oninput={(e) => set_color(row.key, e.currentTarget.value)}
                    aria-describedby="display_color_{row.key}_description"
                    class="h-8 w-12 shrink-0 cursor-pointer rounded-sm border border-gray-300 bg-white p-0.5"
                />
                <div class="min-w-0 flex-1">
                    <label
                        for="display_color_{row.key}"
                        class="block text-sm font-medium text-gray-700"
                    >
                        {row.label}
                    </label>
                    <p id="display_color_{row.key}_description" class="text-xs text-gray-500">
                        {row.description}{customized ? '' : ' — using built-in color'}
                    </p>
                </div>

                <!-- Preview of the line as the device will draw it, on the
                     device's black background. -->
                <div
                    class="hidden h-6 w-28 shrink-0 items-center rounded-sm bg-black px-1 sm:flex"
                    aria-hidden="true"
                >
                    <div
                        class="h-1.5 w-full"
                        style:background={line_background(row.key, row.pattern)}
                    ></div>
                </div>

                <button
                    type="button"
                    onclick={() => reset_color(row.key)}
                    disabled={!customized}
                    class="shrink-0 text-xs text-rayhunter-blue underline disabled:cursor-default disabled:text-gray-300 disabled:no-underline"
                >
                    Reset
                </button>
            </div>
        {/each}
    </div>

    <button
        type="button"
        onclick={reset_all}
        disabled={!any_customized}
        class="text-xs text-rayhunter-blue underline disabled:cursor-default disabled:text-gray-300 disabled:no-underline"
    >
        Reset all colors to defaults
    </button>

    <p class="text-xs text-gray-500">
        Warning severity is also shown by line pattern — dotted for low, dashed for medium, solid
        for high — which stays readable no matter which colors you pick.
    </p>
</div>
