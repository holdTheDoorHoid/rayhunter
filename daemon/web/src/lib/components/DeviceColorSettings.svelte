<script lang="ts">
    import {
        DISPLAY_COLOR_DEFAULTS,
        default_recording_color,
        DEVICE_SCREEN_PX,
        type Config,
        type DisplayColorKey,
    } from '../utils.svelte';
    import { is_too_dark, similar_threat_pairs } from '../colorAdvice';

    let { config = $bindable() }: { config: Config } = $props();

    type LinePattern = 'solid' | 'dashed' | 'dotted';

    interface StateRow {
        key: DisplayColorKey;
        label: string;
        description: string;
        pattern: LinePattern;
    }

    const rows: StateRow[] = [
        { key: 'paused', label: 'Paused', description: 'Recording is stopped', pattern: 'solid' },
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

    const INVISIBLE = 0;
    const DEMO = 2;
    const EFF_LOGO = 3;
    const HIGH_VISIBILITY = 4;
    const CUSTOM_GIF = 5;
    const DEFAULT_BAR_HEIGHT = 2;

    /**
     * The levels that fill the screen, mirroring `covers_the_screen` in
     * daemon/src/display/generic_framebuffer.rs. These are the ones where a
     * button press can step aside, because they are the ones hiding the
     * device's own screens in the first place.
     */
    const COVERS_SCREEN = [DEMO, EFF_LOGO, HIGH_VISIBILITY, CUSTOM_GIF];
    let covers_screen = $derived(COVERS_SCREEN.includes(config.ui_level));

    /**
     * Whether the chosen height is what gets drawn when a button is pressed.
     *
     * In the full screen levels the height is otherwise unused, so it looked
     * safe to lock the slider. It is not: stepping aside draws the status line
     * at this height, and on a device whose top rows are not visible a two
     * pixel line is no line at all. The Moxee needs about six.
     */
    let pause_uses_height = $derived(covers_screen && config.pause_display_on_keypress);

    /** Only truly fixed when nothing will ever draw it at the chosen height. */
    let height_locked = $derived(config.ui_level === HIGH_VISIBILITY && !pause_uses_height);
    let bar_height = $derived(
        height_locked ? DEVICE_SCREEN_PX : (config.status_bar_height ?? DEFAULT_BAR_HEIGHT)
    );

    function fallback_color(key: DisplayColorKey): string {
        return key === 'recording'
            ? default_recording_color(config.colorblind_mode)
            : DISPLAY_COLOR_DEFAULTS[key];
    }

    function effective_color(key: DisplayColorKey): string {
        return config.display_colors[key] ?? fallback_color(key);
    }

    function reset_color(key: DisplayColorKey) {
        config.display_colors[key] = null;
    }

    function reset_all() {
        for (const row of rows) config.display_colors[row.key] = null;
    }

    let any_customized = $derived(rows.some((row) => config.display_colors[row.key] !== null));

    /**
     * Turning on colorblind mode does nothing for recording if a custom color
     * is set, because the explicit choice wins. Worth saying out loud: someone
     * enabling it for accessibility would otherwise assume it had taken effect.
     */
    let colorblind_overridden = $derived(
        config.colorblind_mode && config.display_colors.recording !== null
    );

    let dim_states = $derived(
        rows.filter((row) => is_too_dark(effective_color(row.key))).map((row) => row.label)
    );

    let clashing = $derived(
        similar_threat_pairs(
            rows.map((row) => ({
                key: row.key,
                label: row.label,
                hex: effective_color(row.key),
            }))
        )
    );

    /**
     * Reproduce the device's line pattern in CSS. The device draws dashes as
     * 4 pixels on / 4 off and dots as 1 on / 3 off, scaled up here to stay
     * visible on a normal screen.
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

    // The preview is a scale model of the screen, so the bar occupies roughly
    // the same fraction of it that it will on the device. A true-to-scale 2px
    // line would be sub-pixel here and read as an empty box, so the bar never
    // drops below two pixels — enough to show its color without pretending a
    // hairline is prominent.
    const PREVIEW_PX = 44;
    let preview_bar_px = $derived(
        Math.max(2, Math.round((bar_height / DEVICE_SCREEN_PX) * PREVIEW_PX))
    );
</script>

<div class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-4">
    <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100">Status Line</h3>

    <p class="text-xs text-gray-500 dark:text-gray-400">
        The colored line Rayhunter draws on the device's own screen to show what it is doing. It
        appears in every mode except Invisible.
        {#if config.ui_level === CUSTOM_GIF}
            In Custom image mode it is only used for states that have no image.
        {/if}
        These settings do not affect the colors on this page.
    </p>

    <!-- Height first: it governs the shape of everything previewed below. -->
    {#if config.ui_level !== INVISIBLE}
        <div>
            <label
                for="status_bar_height"
                class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
            >
                Line height: {bar_height}
                {bar_height === 1 ? 'pixel' : 'pixels'}
                {#if bar_height >= DEVICE_SCREEN_PX}<span
                        class="font-normal text-gray-500 dark:text-gray-400">(full screen)</span
                    >{/if}
            </label>
            <input
                id="status_bar_height"
                type="range"
                min="1"
                max={DEVICE_SCREEN_PX}
                value={bar_height}
                disabled={height_locked}
                oninput={(e) => (config.status_bar_height = Number(e.currentTarget.value))}
                aria-describedby="status_bar_height_description"
                class="w-full accent-rayhunter-blue disabled:opacity-50"
            />
            <p
                id="status_bar_height_description"
                class="text-xs text-gray-500 dark:text-gray-400 mt-1"
            >
                {#if height_locked}
                    High visibility always fills the screen, so this has nothing to draw. Turn on
                    "step aside briefly when a button is pressed" below and it becomes the height of
                    the line left behind while the device's own screens show.
                {:else if pause_uses_height}
                    This is the line drawn while a button press holds the full screen back. Some
                    devices do not show their top rows at all, so if nothing appears during that
                    moment, raise this. The Moxee needs about 6.
                {:else}
                    How tall the line is on the device's {DEVICE_SCREEN_PX} pixel screen. At the maximum
                    it fills the screen, which is what High visibility mode does.
                {/if}
            </p>
            {#if !height_locked && bar_height <= 2}
                <p class="text-xs text-amber-600 dark:text-amber-400 mt-1">
                    A line this thin is easy to miss at a glance. It is the long-standing default,
                    but consider a taller line if you want to notice warnings across a room.
                </p>
            {/if}
        </div>
    {/if}

    <!-- Colorblind mode sits immediately above the colors it changes. -->
    <div>
        <div class="flex items-center">
            <input
                id="colorblind_mode"
                type="checkbox"
                bind:checked={config.colorblind_mode}
                aria-describedby="colorblind_mode_description"
                class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
            />
            <label
                for="colorblind_mode"
                class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
            >
                Colorblind mode — use blue instead of green
            </label>
        </div>
        <p id="colorblind_mode_description" class="text-xs text-gray-500 dark:text-gray-400 mt-1">
            Changes the built-in recording color from green to blue, which most forms of color
            blindness can tell apart from the warning colors. Warning colors are unchanged, and
            severity is also shown by line pattern (dotted, dashed, solid) regardless of color.
        </p>
        {#if colorblind_overridden}
            <p class="text-xs text-amber-600 dark:text-amber-400 mt-1">
                This is currently having no effect: you have set your own Recording color below, and
                an explicit choice takes precedence. Reset Recording to use blue.
            </p>
        {/if}
    </div>

    <div class="space-y-3">
        {#each rows as row (row.key)}
            {@const customized = config.display_colors[row.key] !== null}
            {@const dim = is_too_dark(effective_color(row.key))}
            <div>
                <div class="flex items-center gap-3">
                    <input
                        id="display_color_{row.key}"
                        type="color"
                        value={effective_color(row.key)}
                        oninput={(e) => (config.display_colors[row.key] = e.currentTarget.value)}
                        aria-describedby="display_color_{row.key}_description"
                        class="h-8 w-12 shrink-0 cursor-pointer rounded-sm border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-900 p-0.5"
                    />
                    <div class="min-w-0 flex-1">
                        <label
                            for="display_color_{row.key}"
                            class="block text-sm font-medium text-gray-700 dark:text-gray-200"
                        >
                            {row.label}
                        </label>
                        <p
                            id="display_color_{row.key}_description"
                            class="text-xs text-gray-500 dark:text-gray-400"
                        >
                            {row.description}{customized ? '' : ' — using built-in color'}
                        </p>
                    </div>

                    <!-- Scale model of the device screen: the bar covers the
                         same fraction of it that it will on the device. -->
                    <div
                        class="hidden shrink-0 overflow-hidden rounded-sm border border-gray-300 dark:border-gray-600 bg-black sm:block"
                        style:width="{PREVIEW_PX}px"
                        style:height="{PREVIEW_PX}px"
                        aria-hidden="true"
                    >
                        <div
                            style:height="{preview_bar_px}px"
                            style:background={line_background(row.key, row.pattern)}
                        ></div>
                    </div>

                    <button
                        type="button"
                        onclick={() => reset_color(row.key)}
                        disabled={!customized}
                        class="shrink-0 text-xs text-rayhunter-blue underline disabled:cursor-default disabled:text-gray-300 dark:disabled:text-gray-600 disabled:no-underline"
                    >
                        Reset
                    </button>
                </div>
                {#if dim}
                    <p class="text-xs text-amber-600 dark:text-amber-400 mt-1">
                        This color is very dark. The device screen is black, so it may be almost
                        invisible.
                    </p>
                {/if}
            </div>
        {/each}
    </div>

    {#if clashing.length > 0}
        <p class="text-xs text-amber-600 dark:text-amber-400">
            {#each clashing as [a, b], i (a + b)}{i > 0 ? ' ' : ''}<strong>{a}</strong> and
                <strong>{b}</strong> are close enough in color to be hard to tell apart.{/each}
            You may not be able to judge how serious a warning is at a glance. Line patterns still differ,
            which helps.
        </p>
    {/if}

    {#if dim_states.length > 0}
        <p class="text-xs text-gray-500 dark:text-gray-400">
            Nothing is blocked — these are only warnings, and you can save anyway.
        </p>
    {/if}

    <button
        type="button"
        onclick={reset_all}
        disabled={!any_customized}
        class="text-xs text-rayhunter-blue underline disabled:cursor-default disabled:text-gray-300 dark:disabled:text-gray-600 disabled:no-underline"
    >
        Reset all colors to defaults
    </button>
</div>
