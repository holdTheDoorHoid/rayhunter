<script lang="ts">
    import {
        get_config,
        set_config,
        test_notification,
        get_wifi_status,
        get_system_stats,
        scan_wifi_networks,
        GpsMode,
        ClockSyncMode,
        enabled_notifications,
        get_devices,
        rename_device,
        revoke_device,
        mint_pair_code,
        change_passphrase,
        get_tls_info,
        short_date,
        type DeviceInfo,
        type PairCode,
        type TlsInfo,
        delete_web_user,
        type Config,
        type WifiStatus,
        type WifiNetwork,
    } from '../utils.svelte';
    import { onMount } from 'svelte';
    import Modal from './Modal.svelte';
    import ExpandableInput from './ExpandableInput.svelte';
    import DeviceColorSettings from './DeviceColorSettings.svelte';
    import DeviceGifSettings from './DeviceGifSettings.svelte';
    import Explainer from './Explainer.svelte';
    import { HEURISTICS } from '../heuristics';
    import { theme, type ThemePreference } from '../theme.svelte';
    import { help } from '../helpVisibility.svelte';
    import {
        TIME_PRESETS_MINUTES,
        SIZE_PRESETS_MB,
        MIN_MINUTES,
        MIN_SIZE_MB,
        format_interval,
        rotation_summary,
        rotation_warning,
    } from '../recordingRotation';

    let { shown = $bindable() }: { shown: boolean } = $props();
    let config = $state<Config | null>(null);

    let loading = $state(false);
    let saving = $state(false);
    let testingNotification = $state(false);
    let message = $state('');
    let messageType = $state<'success' | 'error' | null>(null);
    let testMessage = $state('');
    let testMessageType = $state<'success' | 'error' | null>(null);
    let wifiStatus = $state<WifiStatus | null>(null);
    /**
     * Whether this device lets ADB be changed from here at all.
     *
     * Read from the device rather than inferred from the configured device
     * type: the type in the config is not always right (a Moxee installed by
     * the shared path calls itself an Orbic), and guessing wrong here would
     * offer a switch that writes a value nobody has checked.
     */
    let adbState = $state<'enabled' | 'disabled' | 'not_adjustable' | null>(null);
    let wifiStatusTimer = $state<ReturnType<typeof setInterval> | null>(null);
    let scanning = $state(false);
    let scanResults = $state<WifiNetwork[]>([]);
    let dnsServersInput = $state('');
    // Whether each rotation limit is being typed in rather than picked from the
    // dropdown. Sticky, so choosing "Other" and then happening to type a value
    // that matches a preset does not snap the control back under the cursor.
    let customRotationTime = $state(false);
    let customRotationSize = $state(false);

    // Accounts are managed by their own endpoints rather than the settings
    // save, because the hashes are redacted on the way out and posting the
    // whole config back would otherwise blank them and lock everyone out.
    let userMessage = $state('');
    let userError = $state('');
    let savingUser = $state(false);

    async function remove_user(username: string) {
        if (!config) return;
        savingUser = true;
        userMessage = '';
        userError = '';
        try {
            userMessage = await delete_web_user(username);
            config.web_users = config.web_users.filter((u) => u.username !== username);
        } catch (e) {
            userError = `${e}`;
        } finally {
            savingUser = false;
        }
    }

    // Pairing: the browsers this unit trusts, and the ways to add one. Read
    // from their own endpoints rather than the config, which never holds them.
    let devices = $state<DeviceInfo[]>([]);
    let pairCode = $state<PairCode | null>(null);
    let tlsInfo = $state<TlsInfo | null>(null);
    let renaming = $state<string | null>(null);
    let renameValue = $state('');
    let pairMessage = $state('');
    let pairError = $state('');
    let currentPassphrase = $state('');
    let newPassphrase = $state('');
    let newPassphraseAgain = $state('');
    let savingPassphrase = $state(false);

    onMount(async () => {
        try {
            devices = await get_devices();
        } catch (e) {
            pairError = `${e}`;
        }
        try {
            tlsInfo = await get_tls_info();
        } catch {
            tlsInfo = null;
        }
    });

    async function refresh_devices() {
        try {
            devices = await get_devices();
        } catch (e) {
            pairError = `${e}`;
        }
    }

    async function make_pair_code() {
        pairMessage = '';
        pairError = '';
        try {
            pairCode = await mint_pair_code();
        } catch (e) {
            pairError = `${e}`;
        }
    }

    function start_rename(d: DeviceInfo) {
        renaming = d.id;
        renameValue = d.name;
    }

    async function commit_rename(id: string) {
        pairError = '';
        try {
            await rename_device(id, renameValue);
            renaming = null;
            await refresh_devices();
        } catch (e) {
            pairError = `${e}`;
        }
    }

    async function revoke(d: DeviceInfo) {
        const what = d.current
            ? 'Remove this browser? You will have to pair it again to get back in.'
            : `Remove "${d.name}"? It will have to be paired again.`;
        if (!confirm(what)) return;
        pairError = '';
        try {
            await revoke_device(d.id);
            if (d.current) {
                location.assign('/pair');
                return;
            }
            await refresh_devices();
        } catch (e) {
            pairError = `${e}`;
        }
    }

    async function save_passphrase() {
        pairMessage = '';
        pairError = '';
        if (newPassphrase !== newPassphraseAgain) {
            pairError = 'The two new passphrases do not match.';
            return;
        }
        savingPassphrase = true;
        try {
            await change_passphrase(currentPassphrase, newPassphrase);
            pairMessage = 'Passphrase changed.';
            currentPassphrase = '';
            newPassphrase = '';
            newPassphraseAgain = '';
        } catch (e) {
            pairError = `${e}`;
        } finally {
            savingPassphrase = false;
        }
    }

    function certificate_download(): string {
        return tlsInfo
            ? `data:application/x-pem-file,${encodeURIComponent(tlsInfo.certificate_pem)}`
            : '';
    }

    /**
     * The sections of the configuration page.
     *
     * Named for what somebody came to change rather than for how the code is
     * organised, which is why storage and GPS sit together under Recordings
     * and the demo control sits with the heuristics it fakes.
     */
    const TABS = [
        { id: 'display', label: 'Display', hint: 'What the device shows on its own screen' },
        { id: 'detection', label: 'Detection', hint: 'Which heuristics run, and the demo control' },
        {
            id: 'recordings',
            label: 'Recordings',
            hint: 'Space, splitting, location, and copying them off the device',
        },
        {
            id: 'notifications',
            label: 'Notifications',
            hint: 'Being told when something happens',
        },
        { id: 'network', label: 'Network', hint: 'Connecting the device to a WiFi network' },
    ] as const;

    let active = $state<(typeof TABS)[number]['id']>('display');
    let tab_buttons = $state<HTMLButtonElement[]>([]);

    /**
     * Arrow keys move between tabs, which is what a tablist is expected to do.
     * Without it the role promises a behaviour that is not there, which is
     * worse for a screen reader than plain buttons would have been.
     */
    function tab_keydown(event: KeyboardEvent) {
        const keys = ['ArrowRight', 'ArrowLeft', 'Home', 'End'];
        if (!keys.includes(event.key)) return;
        event.preventDefault();
        const current = TABS.findIndex((t) => t.id === active);
        let next = current;
        if (event.key === 'ArrowRight') next = (current + 1) % TABS.length;
        if (event.key === 'ArrowLeft') next = (current - 1 + TABS.length) % TABS.length;
        if (event.key === 'Home') next = 0;
        if (event.key === 'End') next = TABS.length - 1;
        active = TABS[next].id;
        tab_buttons[next]?.focus();
    }

    async function load_config() {
        try {
            loading = true;
            config = await get_config();
            // A daemon older than this UI won't send display_colors at all, so
            // fill in an empty set rather than letting the color pickers read
            // properties of undefined.
            const gifs = config.display_gifs;
            config.display_gifs = {
                paused: gifs?.paused ?? null,
                recording: gifs?.recording ?? null,
                warning_low: gifs?.warning_low ?? null,
                warning_medium: gifs?.warning_medium ?? null,
                warning_high: gifs?.warning_high ?? null,
            };
            const colors = config.display_colors;
            config.display_colors = {
                paused: colors?.paused ?? null,
                recording: colors?.recording ?? null,
                warning_low: colors?.warning_low ?? null,
                warning_medium: colors?.warning_medium ?? null,
                warning_high: colors?.warning_high ?? null,
            };
            dnsServersInput = config.dns_servers ? config.dns_servers.join(', ') : '';
            // A daemon older than this UI sends neither, and both mean "never
            // rotate", which is what null already says.
            config.max_recording_size_mb = config.max_recording_size_mb ?? null;
            config.max_recording_minutes = config.max_recording_minutes ?? null;
            const minutes = config.max_recording_minutes;
            customRotationTime = minutes !== null && !TIME_PRESETS_MINUTES.includes(minutes);
            const sizeMb = config.max_recording_size_mb;
            customRotationSize = sizeMb !== null && !SIZE_PRESETS_MB.includes(sizeMb);
            message = '';
            messageType = null;
            poll_wifi_status();
        } catch (error) {
            message = `Failed to load config: ${error}`;
            messageType = 'error';
        } finally {
            loading = false;
        }
    }

    /**
     * Set the handful of settings that actually affect how long a battery
     * lasts, so nobody has to know which ones those are.
     *
     * Deliberately does not touch the detectors. Turning detection off would
     * save a little and defeat the point of the device.
     *
     * Applied to the form, not saved: the person still sees what changed and
     * presses save, so this can never quietly reconfigure a device.
     */
    let lowPowerApplied = $state(false);
    function apply_low_power() {
        if (!config) return;
        config.ui_level = 0;
        config.keep_screen_on = 0;
        config.auto_check_updates = false;
        config.wifi_ap_button_toggle = true;
        lowPowerApplied = true;
    }

    async function load_adb_state() {
        try {
            const stats = await get_system_stats();
            adbState = stats.adb?.state ?? null;
        } catch {
            // Not worth failing the settings page over. The control simply
            // does not appear.
            adbState = null;
        }
    }

    async function save_config() {
        if (!config) return;

        const trimmed = dnsServersInput.trim();
        config.dns_servers =
            trimmed.length > 0
                ? trimmed
                      .split(',')
                      .map((s) => s.trim())
                      .filter((s) => s.length > 0)
                : null;

        try {
            saving = true;
            await set_config(config);
            message =
                'Config saved successfully! Rayhunter is restarting now. Reload the page in a few seconds.';
            messageType = 'success';
        } catch (error) {
            message = `Failed to save config: ${error}`;
            messageType = 'error';
        } finally {
            saving = false;
        }
    }

    async function poll_wifi_status() {
        if (wifiStatusTimer) clearInterval(wifiStatusTimer);
        try {
            wifiStatus = await get_wifi_status();
        } catch {
            wifiStatus = null;
        }
        wifiStatusTimer = setInterval(async () => {
            try {
                wifiStatus = await get_wifi_status();
            } catch {
                wifiStatus = null;
            }
        }, 5000);
    }

    let scanError = $state('');

    async function do_scan() {
        scanning = true;
        scanError = '';
        try {
            scanResults = await scan_wifi_networks();
        } catch (error) {
            scanResults = [];
            scanError = `Scan failed: ${error}`;
        } finally {
            scanning = false;
        }
    }

    function select_network(network: WifiNetwork) {
        if (config) {
            config.wifi_ssid = network.ssid;
            config.wifi_password = '';
            config.wifi_security =
                network.security === 'WPA3' || network.security === 'WPA3 (transition)'
                    ? 'sae'
                    : 'wpa_psk';
            scanResults = [];
        }
    }

    async function send_test_notification() {
        try {
            testingNotification = true;
            testMessage = '';
            testMessageType = null;
            await test_notification();
            testMessage = 'Test notification sent successfully!';
            testMessageType = 'success';
        } catch (error) {
            testMessage = `${error}`;
            testMessageType = 'error';
        } finally {
            testingNotification = false;
        }
    }

    $effect(() => {
        if (shown && !config) {
            load_config();
            load_adb_state();
        }
        if (!shown && wifiStatusTimer) {
            clearInterval(wifiStatusTimer);
            wifiStatusTimer = null;
        }
        return () => {
            if (wifiStatusTimer) {
                clearInterval(wifiStatusTimer);
                wifiStatusTimer = null;
            }
        };
    });
</script>

<Modal bind:shown title="Configuration">
    <div class="p-2">
        {#if loading}
            <div class="text-center py-4">Loading config...</div>
        {:else if config}
            <!-- Kept out of the form on purpose. These two are stored in this
                 browser and take effect the moment they change, while
                 everything in the form below is written to the device and needs
                 applying. Mixing them taught people to press Apply for a
                 setting that never needed it. -->
            <div class="rounded-md border border-gray-200 p-3 dark:border-gray-700">
                <p class="mb-2 text-xs text-gray-500 dark:text-gray-400">
                    These two are remembered by this browser and change straight away.
                </p>
                <div class="grid gap-3 sm:grid-cols-2">
                    <div>
                        <label
                            for="theme_preference"
                            class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                        >
                            Appearance of this page
                        </label>
                        <select
                            id="theme_preference"
                            value={theme.preference}
                            onchange={(e) => theme.set(e.currentTarget.value as ThemePreference)}
                            class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                        >
                            <option value="system">Match my device</option>
                            <option value="light">Light</option>
                            <option value="dark">Dark</option>
                        </select>
                        <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                            This web page only. The device's own screen is set under Display.
                        </p>
                    </div>

                    <div>
                        <div class="flex items-center">
                            <input
                                id="show_help"
                                type="checkbox"
                                checked={help.shown}
                                onchange={(e) => help.set(e.currentTarget.checked)}
                                aria-describedby="show_help_description"
                                class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
                            />
                            <label
                                for="show_help"
                                class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                            >
                                Show explanations
                            </label>
                        </div>
                        <p
                            id="show_help_description"
                            class="text-xs text-gray-500 dark:text-gray-400 mt-1"
                        >
                            Keeps the "what this means" sections throughout the interface. Turn it
                            off once you know your way around and the pages become considerably
                            shorter. Readings, settings and their own labels stay either way; only
                            the explanations go.
                        </p>
                    </div>
                </div>
            </div>

            <!-- Tabs rather than one long scroll. The page had grown past
                 the point where anything could be found by scrolling, and
                 these five names are how people actually describe what they
                 came to change. They wrap rather than scroll sideways, so
                 every section stays visible on a phone. -->
            <div
                role="tablist"
                aria-label="Configuration sections"
                class="sticky top-0 z-10 -mx-2 mt-4 flex flex-wrap gap-1 border-b border-gray-200 bg-white px-2 pt-1 pb-2 dark:border-gray-700 dark:bg-gray-900"
            >
                {#each TABS as tab, i (tab.id)}
                    <button
                        type="button"
                        role="tab"
                        id="tab-{tab.id}"
                        aria-selected={active === tab.id}
                        aria-controls="panel-{tab.id}"
                        tabindex={active === tab.id ? 0 : -1}
                        title={tab.hint}
                        bind:this={tab_buttons[i]}
                        onclick={() => (active = tab.id)}
                        onkeydown={tab_keydown}
                        class="rounded-full px-3 py-1 text-sm {active === tab.id
                            ? 'bg-rayhunter-blue text-white'
                            : 'text-gray-700 hover:bg-gray-100 dark:text-gray-200 dark:hover:bg-gray-800'}"
                    >
                        {tab.label}
                    </button>
                {/each}
            </div>

            <form
                class="space-y-4"
                onsubmit={(e) => {
                    e.preventDefault();
                    save_config();
                }}
            >
                {#if active === 'display'}
                    <div class="mb-4 rounded-md border border-gray-200 p-3 dark:border-gray-700">
                        <div class="flex flex-wrap items-center gap-2">
                            <button
                                type="button"
                                onclick={apply_low_power}
                                class="rounded-md border border-gray-300 px-3 py-1 text-sm dark:border-gray-600"
                            >
                                Set up for longest battery life
                            </button>
                            {#if lowPowerApplied}
                                <span class="text-xs text-green-700 dark:text-green-300">
                                    Applied below. Review and save.
                                </span>
                            {/if}
                        </div>
                        <p class="mt-2 text-xs text-gray-500 dark:text-gray-400">
                            Turns the device display off, stops Rayhunter holding the screen awake,
                            stops the periodic update check, and allows switching WiFi off from the
                            buttons. It changes the settings below rather than saving them, so you
                            can see what it did.
                        </p>
                        <Explainer summary="What actually uses the battery, measured.">
                            <p>
                                Rayhunter itself is not the expensive part. Measured while
                                recording, the daemon used about <strong>1% of one core</strong> on
                                an Orbic RC400L and about <strong>6.5%</strong> on a TP-Link M7350. The
                                display mode made no measurable difference to that on either.
                            </p>
                            <p>
                                What costs real power on a hotspot is the radio and the screen. So
                                the settings worth changing are the ones that stop the screen being
                                held on and let you switch the WiFi access point off when you are
                                not using it, which is what this does. Detection is left alone: it
                                is cheap, and switching it off would defeat the point of carrying
                                the device.
                            </p>
                        </Explainer>
                    </div>
                    <div
                        id="panel-display"
                        role="tabpanel"
                        aria-labelledby="tab-display"
                        class="space-y-4"
                    >
                        <p class="text-xs text-gray-500 dark:text-gray-400">
                            What the device itself shows, and how its buttons behave. None of this
                            affects what is recorded or detected.
                        </p>

                        <div>
                            <label
                                for="ui_level"
                                class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                            >
                                Device UI Level
                            </label>
                            <select
                                id="ui_level"
                                bind:value={config.ui_level}
                                class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                            >
                                <option value={0}>Invisible mode</option>
                                <option value={1}>Subtle mode (colored line)</option>
                                <option value={4}>High visibility (full screen color)</option>
                                <option value={5}>Custom image</option>
                                <option value={2}>Demo mode (orca gif)</option>
                                <option value={3}>EFF logo</option>
                            </select>
                            <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                Note: Rayhunter draws over the device's native UI, so some
                                flickering is expected
                            </p>
                        </div>

                        <!-- The status line is drawn in every mode but Invisible, so
                     hiding these anywhere else would hide a live setting. -->
                        {#if config.ui_level !== 0}
                            <DeviceColorSettings bind:config />
                        {/if}

                        {#if config.ui_level === 5}
                            <DeviceGifSettings bind:config />
                        {/if}

                        <div>
                            <label
                                for="key_input_mode"
                                class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                            >
                                Device Input Mode
                            </label>
                            <select
                                id="key_input_mode"
                                bind:value={config.key_input_mode}
                                class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                            >
                                <option value={0}>Disable button control</option>
                                <option value={1}
                                    >Double-tap power button to start new recording</option
                                >
                            </select>
                        </div>

                        <div>
                            <label
                                for="keep_screen_on"
                                class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                            >
                                Keep the screen on
                            </label>
                            <select
                                id="keep_screen_on"
                                bind:value={config.keep_screen_on}
                                class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                            >
                                <option value={0}>Let the screen turn itself off</option>
                                <option value={2}>Keep it on while plugged in</option>
                                <option value={1}>Keep it on always</option>
                            </select>
                            {#if config.keep_screen_on === 1}
                                <p class="mt-1 text-xs text-amber-600 dark:text-amber-400">
                                    On battery this will flatten the device considerably faster.
                                    Pick "while plugged in" unless you have a reason not to.
                                </p>
                            {/if}
                            <!-- Only offered for the levels that actually cover the
                                 device's own screens. Mirrors covers_the_screen in
                                 daemon/src/display/generic_framebuffer.rs. In Subtle
                                 a thin line was never hiding anything, so there would
                                 be nothing to step aside from and this would be a
                                 control that visibly does nothing. -->
                            <div class="border-t border-gray-200 dark:border-gray-700 pt-3 mt-4">
                                <h4 class="text-sm font-medium text-gray-700 dark:text-gray-200">
                                    Who can use this interface
                                </h4>
                                <p class="mt-1 text-xs text-gray-600 dark:text-gray-300">
                                    Only phones and computers paired with this unit. Add one with a
                                    code from here, or with the owner passphrase on the pairing
                                    page.
                                </p>

                                <h5
                                    class="mt-3 text-sm font-medium text-gray-700 dark:text-gray-200"
                                >
                                    Trusted devices
                                </h5>
                                {#if devices.length === 0}
                                    <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                        None paired yet.
                                    </p>
                                {:else}
                                    <ul class="mt-1 space-y-1">
                                        {#each devices as d (d.id)}
                                            <li
                                                class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm"
                                            >
                                                {#if renaming === d.id}
                                                    <input
                                                        type="text"
                                                        bind:value={renameValue}
                                                        maxlength="64"
                                                        class="w-44 rounded-md border border-gray-300 px-2 py-0.5 text-sm dark:border-gray-600"
                                                    />
                                                    <button
                                                        type="button"
                                                        onclick={() => commit_rename(d.id)}
                                                        class="text-xs underline"
                                                    >
                                                        save
                                                    </button>
                                                    <button
                                                        type="button"
                                                        onclick={() => (renaming = null)}
                                                        class="text-xs underline"
                                                    >
                                                        cancel
                                                    </button>
                                                {:else}
                                                    <span>{d.name}</span>
                                                    {#if d.current}
                                                        <span
                                                            class="rounded bg-blue-100 px-1 text-xs text-blue-800 dark:bg-blue-900 dark:text-blue-100"
                                                        >
                                                            this browser
                                                        </span>
                                                    {/if}
                                                    <span
                                                        class="text-xs text-gray-500 dark:text-gray-400"
                                                    >
                                                        added {short_date(d.created)}, last seen {short_date(
                                                            d.last_seen
                                                        )}
                                                    </span>
                                                    <button
                                                        type="button"
                                                        onclick={() => start_rename(d)}
                                                        class="text-xs underline"
                                                    >
                                                        rename
                                                    </button>
                                                    <button
                                                        type="button"
                                                        onclick={() => revoke(d)}
                                                        class="text-xs text-red-600 underline dark:text-red-400"
                                                    >
                                                        remove
                                                    </button>
                                                {/if}
                                            </li>
                                        {/each}
                                    </ul>
                                {/if}

                                <button
                                    type="button"
                                    onclick={make_pair_code}
                                    class="mt-2 rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
                                >
                                    Add a phone or computer
                                </button>
                                {#if pairCode}
                                    <div class="mt-2 flex items-start gap-3">
                                        <div class="w-32 shrink-0 rounded bg-white p-1">
                                            <!-- The SVG comes from the unit itself, not from anything typed. -->
                                            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                                            {@html pairCode.svg}
                                        </div>
                                        <div class="text-sm">
                                            <p>
                                                On the new device, join this unit's WiFi and scan
                                                this, or open the pairing page and type
                                                <code class="font-mono text-base tracking-widest"
                                                    >{pairCode.code.slice(0, 3)}
                                                    {pairCode.code.slice(3)}</code
                                                >.
                                            </p>
                                            <p
                                                class="mt-1 text-xs text-gray-500 dark:text-gray-400"
                                            >
                                                Good for {Math.round(pairCode.expires_in_secs / 60)} minutes
                                                and one device. Making another replaces it.
                                            </p>
                                            <p
                                                class="mt-1 font-mono text-xs break-all text-gray-500 dark:text-gray-400"
                                            >
                                                {pairCode.url.toLowerCase()}
                                            </p>
                                        </div>
                                    </div>
                                {/if}

                                <h5
                                    class="mt-4 text-sm font-medium text-gray-700 dark:text-gray-200"
                                >
                                    Owner passphrase
                                </h5>
                                <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                    Adds a device from the pairing page, and opens the terminal.
                                    Changing it does not sign anything out.
                                </p>
                                <div class="mt-2 flex flex-wrap items-end gap-2">
                                    <div>
                                        <label
                                            for="current_passphrase"
                                            class="block text-xs text-gray-600 dark:text-gray-300"
                                            >Current</label
                                        >
                                        <input
                                            id="current_passphrase"
                                            type="password"
                                            bind:value={currentPassphrase}
                                            autocomplete="current-password"
                                            class="w-36 rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
                                        />
                                    </div>
                                    <div>
                                        <label
                                            for="new_passphrase"
                                            class="block text-xs text-gray-600 dark:text-gray-300"
                                            >New</label
                                        >
                                        <input
                                            id="new_passphrase"
                                            type="password"
                                            bind:value={newPassphrase}
                                            autocomplete="new-password"
                                            class="w-36 rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
                                        />
                                    </div>
                                    <div>
                                        <label
                                            for="new_passphrase_again"
                                            class="block text-xs text-gray-600 dark:text-gray-300"
                                            >New again</label
                                        >
                                        <input
                                            id="new_passphrase_again"
                                            type="password"
                                            bind:value={newPassphraseAgain}
                                            autocomplete="new-password"
                                            class="w-36 rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
                                        />
                                    </div>
                                    <button
                                        type="button"
                                        onclick={save_passphrase}
                                        disabled={savingPassphrase ||
                                            !currentPassphrase ||
                                            newPassphrase.length < 8}
                                        class="rounded-md border border-gray-300 px-2 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
                                    >
                                        {savingPassphrase ? 'Saving…' : 'Change'}
                                    </button>
                                </div>
                                {#if pairMessage}
                                    <p class="mt-1 text-xs text-green-700 dark:text-green-300">
                                        {pairMessage}
                                    </p>
                                {/if}
                                {#if pairError}
                                    <p class="mt-1 text-xs text-red-600 dark:text-red-400">
                                        {pairError}
                                    </p>
                                {/if}

                                {#if tlsInfo}
                                    <h5
                                        class="mt-4 text-sm font-medium text-gray-700 dark:text-gray-200"
                                    >
                                        This unit's certificate
                                    </h5>
                                    <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                        Made by the unit itself, which is why browsers warn about it
                                        once. Its fingerprint, to compare with what your browser
                                        shows:
                                    </p>
                                    <code class="block font-mono text-xs break-all"
                                        >{tlsInfo.fingerprint_sha256}</code
                                    >
                                    <a
                                        href={certificate_download()}
                                        download="rayhunter.pem"
                                        class="mt-1 inline-block text-xs underline"
                                    >
                                        Download the certificate
                                    </a>
                                {/if}

                                {#if config.web_users.length > 0}
                                    <h5
                                        class="mt-4 text-sm font-medium text-gray-700 dark:text-gray-200"
                                    >
                                        Accounts from before pairing
                                    </h5>
                                    <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">
                                        These no longer open the interface on their own. Signing in
                                        with one on the pairing page pairs that browser, once.
                                        Remove them when every device has been paired.
                                    </p>
                                    <ul class="mt-1 space-y-1">
                                        {#each config.web_users as user (user.username)}
                                            <li class="flex items-center gap-2 text-sm">
                                                <span class="font-mono">{user.username}</span>
                                                <button
                                                    type="button"
                                                    onclick={() => remove_user(user.username)}
                                                    disabled={savingUser}
                                                    class="text-xs text-red-600 underline disabled:opacity-40 dark:text-red-400"
                                                >
                                                    remove
                                                </button>
                                            </li>
                                        {/each}
                                    </ul>
                                    {#if userMessage}
                                        <p class="mt-1 text-xs text-green-700 dark:text-green-300">
                                            {userMessage}
                                        </p>
                                    {/if}
                                    {#if userError}
                                        <p class="mt-1 text-xs text-red-600 dark:text-red-400">
                                            {userError}
                                        </p>
                                    {/if}
                                {/if}

                                <Explainer
                                    summary="How pairing protects this unit, and what it does not."
                                >
                                    <p>
                                        Every request to this interface travels over an encrypted
                                        connection to the unit, and only a browser the unit has
                                        paired with is answered. Pairing happens once per browser:
                                        by scanning the code on the unit's screen when it is new,
                                        with a code made here, or with the owner passphrase.
                                    </p>
                                    <p>
                                        What this protects against is anyone else on the WiFi: a
                                        guest, a family member, whoever was given the hotspot
                                        password for another reason. They can no longer read the
                                        recordings or change anything, even if they can capture the
                                        traffic.
                                    </p>
                                    <p>
                                        What it does not protect against is someone holding the
                                        unit. The USB cable is root, and a button press on a unit
                                        that nobody has paired with yet lets its holder pair. If
                                        every paired device and the passphrase are lost, the pairing
                                        records are removed over USB and the unit starts over.
                                    </p>
                                </Explainer>
                            </div>

                            <div class="mt-3 flex items-center">
                                <input
                                    id="show_subscriber_identity"
                                    type="checkbox"
                                    bind:checked={config.show_subscriber_identity}
                                    class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
                                />
                                <label
                                    for="show_subscriber_identity"
                                    class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                >
                                    Show this device's own IMSI, IMEI and temporary identity
                                </label>
                            </div>
                            {#if config.show_subscriber_identity}
                                <p class="mt-1 text-xs text-amber-600 dark:text-amber-400">
                                    The web interface has no password. While this is on, anyone who
                                    can reach this page can read your IMSI, and on a hotspot that
                                    means anyone on its WiFi.
                                </p>
                            {/if}
                            <Explainer
                                summary="Why this is off by default, and what the numbers are worth watching for."
                            >
                                <p>
                                    Your IMSI identifies you as a subscriber and never changes. It
                                    is the identifier an IMSI catcher exists to collect, which is
                                    why a detector that published it unasked would be working
                                    against its own purpose.
                                </p>
                                <p>
                                    What is worth watching is not the number but how often it was
                                    sent. A network normally issues a temporary identity and uses
                                    that, rotating it so sessions cannot be linked. Being asked for
                                    the permanent one repeatedly is what a device collecting IMSIs
                                    makes happen.
                                </p>
                                <p>
                                    Turn it on when you want to see that count, and off again
                                    afterwards. Nothing is recorded differently either way; this
                                    only decides whether the web interface will say it.
                                </p>
                            </Explainer>

                            {#if [2, 3, 4, 5].includes(config.ui_level)}
                                <div class="mt-3 flex items-center">
                                    <input
                                        id="pause_display_on_keypress"
                                        type="checkbox"
                                        bind:checked={config.pause_display_on_keypress}
                                        class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
                                    />
                                    <label
                                        for="pause_display_on_keypress"
                                        class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                    >
                                        Step aside briefly when a button is pressed
                                    </label>
                                </div>
                                <Explainer
                                    summary="Lets you read the device's own screens, the wifi password included, without losing the status indicator."
                                >
                                    <p>
                                        Rayhunter paints over the device's own interface. In the
                                        display levels that fill the screen, a custom image or high
                                        visibility, that interface is completely hidden, and that
                                        includes the pages showing the wifi name and password.
                                        Somebody who has not written the password down can end up
                                        locked out of their own hotspot, which is a steep price for
                                        a change to the colour of a status light.
                                    </p>
                                    <p>
                                        With this on, pressing a button shrinks Rayhunter to its
                                        thin status line for twenty seconds, which is enough to find
                                        a password and type it somewhere. It does not go dark: a
                                        button press must never be able to hide a high severity
                                        warning, so the line stays and keeps its colour. Pressing
                                        more buttons extends the twenty seconds rather than cutting
                                        it short.
                                    </p>
                                    <p>
                                        Nothing about detection changes. Recording and analysis
                                        carry on throughout, and this only affects the levels that
                                        cover the screen; a thin line was never hiding anything to
                                        begin with.
                                    </p>
                                    <p>
                                        The line left behind uses the height set above. On a device
                                        whose top rows are not visible, a two pixel line cannot be
                                        seen at all, so raise it until it can.
                                    </p>
                                </Explainer>
                            {/if}

                            <Explainer
                                summary="Why the screen goes dark while Rayhunter is plainly running."
                            >
                                <p>
                                    The device blanks its own screen on a timer, and drawing to the
                                    screen does not count as activity to that timer. So Rayhunter
                                    can be recording perfectly well with nothing to show for it, and
                                    the only way to check is to press a button. That defeats the
                                    point of a status light you are meant to notice out of the
                                    corner of your eye.
                                </p>
                                <p>
                                    Keeping it on holds the backlight and stops the device
                                    suspending. That is why the plugged in option exists, and why it
                                    is the one to pick: a backlight held on is one of the quickest
                                    ways to flatten one of these batteries. Set to plugged in, the
                                    screen stays lit on a desk and the device goes back to saving
                                    power the moment you unplug it.
                                </p>
                                <p>
                                    Currently implemented for the Orbic. Other devices accept the
                                    setting and ignore it rather than failing.
                                </p>
                            </Explainer>
                        </div>
                    </div>
                {:else if active === 'detection'}
                    <div
                        id="panel-detection"
                        role="tabpanel"
                        aria-labelledby="tab-detection"
                        class="space-y-4"
                    >
                        <div class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6">
                            <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-2">
                                Analyzer Heuristic Settings
                            </h3>
                            <p class="text-xs text-gray-500 dark:text-gray-400 mb-4">
                                Each of these watches for a different sign that a tower is not
                                behaving the way a real one should. Leaving them on costs you
                                nothing except the occasional false alarm. Open any entry to read
                                what it looks for and why it matters.
                            </p>
                            <div class="space-y-4">
                                {#each HEURISTICS as h (h.key)}
                                    <div>
                                        <div class="flex items-center">
                                            <input
                                                id={h.key}
                                                type="checkbox"
                                                bind:checked={config.analyzers[h.key]}
                                                class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
                                            />
                                            <label
                                                for={h.key}
                                                class="ml-2 block text-sm font-medium text-gray-700 dark:text-gray-200"
                                            >
                                                {h.title}
                                            </label>
                                            {#if h.tag === 'noisy'}
                                                <span
                                                    class="ml-2 rounded-sm bg-amber-100 dark:bg-amber-950 px-1.5 py-0.5 text-[10px] font-medium text-amber-800 dark:text-amber-300"
                                                    >very noisy</span
                                                >
                                            {:else if h.tag === 'informational'}
                                                <span
                                                    class="ml-2 rounded-sm bg-gray-100 dark:bg-gray-800 px-1.5 py-0.5 text-[10px] font-medium text-gray-600 dark:text-gray-300"
                                                    >notes only</span
                                                >
                                            {/if}
                                        </div>
                                        <div class="ml-6">
                                            <Explainer summary={h.summary}>
                                                <p>
                                                    <strong>What it looks for.</strong>
                                                    {h.detects}
                                                </p>
                                                <p><strong>Why it matters.</strong> {h.matters}</p>
                                                {#if h.noise}
                                                    <p><strong>Worth knowing.</strong> {h.noise}</p>
                                                {/if}
                                            </Explainer>
                                        </div>
                                    </div>
                                {/each}
                            </div>
                        </div>
                        <div
                            class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-3"
                        >
                            <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4">
                                Demonstration
                            </h3>
                            <div class="flex items-center">
                                <input
                                    id="demo_mode"
                                    type="checkbox"
                                    bind:checked={config.demo_mode}
                                    aria-describedby="demo_mode_description"
                                    class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
                                />
                                <label
                                    for="demo_mode"
                                    class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                >
                                    Enable the demo warning button
                                </label>
                            </div>
                            <Explainer
                                keepSummary
                                summary="Adds a button to the main page that fakes a surveillance detection, for showing Rayhunter to an audience."
                            >
                                <p>
                                    The button injects a synthetic message into the recording that
                                    looks to Rayhunter like a tower switching encryption off, which
                                    is one of the clearest signs of a fake base station. It goes
                                    through the real detectors, so the warning appears in the
                                    history and turns the device red exactly as a genuine one would.
                                    That is the point: it demonstrates how Rayhunter actually works
                                    rather than painting a warning on screen.
                                </p>
                                <p>
                                    <strong>The fake message is written into your recording.</strong
                                    > Every warning it produces is labelled as a demo in its own text,
                                    so it can be recognised later by somebody who was not present. Even
                                    so, do not send a recording containing demo data to EFF or treat it
                                    as evidence.
                                </p>
                                <p>
                                    Leave this off when you are actually hunting. It only adds a
                                    button, and the device refuses the request entirely while this
                                    is unchecked, but there is no reason to have it available by
                                    accident.
                                </p>
                            </Explainer>
                        </div>
                    </div>
                {:else if active === 'recordings'}
                    <div
                        id="panel-recordings"
                        role="tabpanel"
                        aria-labelledby="tab-recordings"
                        class="space-y-4"
                    >
                        <div class="space-y-3">
                            <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4">
                                Clock
                            </h3>

                            <div>
                                <label
                                    for="clock_sync_mode"
                                    class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                >
                                    Clock Sync
                                </label>
                                <select
                                    id="clock_sync_mode"
                                    bind:value={config.clock_sync_mode}
                                    class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                >
                                    <option value={ClockSyncMode.Prompt}>
                                        Prompt (ask before syncing)
                                    </option>
                                    <option value={ClockSyncMode.Autosync}>
                                        Autosync (copy browser clock automatically)
                                    </option>
                                    <option value={ClockSyncMode.Off}>
                                        Off (never warn or sync)
                                    </option>
                                </select>
                                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                    What to do when the device's clock disagrees with your
                                    browser's.
                                </p>
                            </div>

                            <Explainer
                                summary="Why this sits with recordings, and what syncing does not do."
                            >
                                <p>
                                    Some devices have no battery-backed clock, so they lose the time
                                    whenever they reboot. That matters here because every recording
                                    is stamped with the device's clock: if it is wrong, so is the
                                    record of when anything was seen.
                                </p>
                                <p>
                                    Syncing applies an offset held in memory only. It is not written
                                    to the device's clock and is lost when Rayhunter restarts, so
                                    Autosync re-applies it each time you open the web interface.
                                </p>
                            </Explainer>
                        </div>

                        <div
                            class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-3"
                        >
                            <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4">
                                Storage Management
                            </h3>

                            <div>
                                <label
                                    for="min_space_to_start_recording_mb"
                                    class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                >
                                    Minimum Space to Start Recording (MB)
                                </label>
                                <input
                                    id="min_space_to_start_recording_mb"
                                    type="number"
                                    min="1"
                                    bind:value={config.min_space_to_start_recording_mb}
                                    class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                />
                                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                    Recording will not start if less than this amount of disk space
                                    is free
                                </p>
                            </div>

                            <div>
                                <label
                                    for="min_space_to_continue_recording_mb"
                                    class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                >
                                    Minimum Space to Continue Recording (MB)
                                </label>
                                <input
                                    id="min_space_to_continue_recording_mb"
                                    type="number"
                                    min="1"
                                    bind:value={config.min_space_to_continue_recording_mb}
                                    class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                />
                                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                    Recording will stop automatically if disk space drops below this
                                    level
                                </p>
                            </div>

                            <div class="border-t border-gray-200 dark:border-gray-700 pt-3 mt-4">
                                <div class="flex items-center">
                                    <input
                                        id="auto_delete_clean_recordings"
                                        type="checkbox"
                                        bind:checked={config.auto_delete_clean_recordings}
                                        class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
                                    />
                                    <label
                                        for="auto_delete_clean_recordings"
                                        class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                    >
                                        Delete recordings that found nothing when space runs low
                                    </label>
                                </div>
                                <Explainer
                                    summary="Keeps the device recording instead of stopping when the disk fills, without ever removing a recording that found something."
                                >
                                    <p>
                                        A device left running fills its storage and then stops
                                        recording, which is the moment it stops being a detector.
                                        Most recordings find nothing, and those are the ones safe to
                                        lose.
                                    </p>
                                    <p>
                                        Only recordings that have been analysed and raised no
                                        warning at all are removed, oldest first, and only as many
                                        as it takes to make room. A recording that raised anything
                                        is never touched. Neither is one still being written, one
                                        that has not been analysed yet, or one still waiting to be
                                        uploaded to a WebDAV server. Not knowing what is in a
                                        recording is not a reason to delete it.
                                    </p>
                                    <p>
                                        Giving a recording a name or notes also protects it.
                                        Stopping to label one says it matters to you, which is a
                                        better signal than anything the device can work out for
                                        itself.
                                    </p>
                                    <p>
                                        Informational notes do not count as findings. They are
                                        diagnostics rather than detections, so a recording carrying
                                        only those is still one that found nothing.
                                    </p>
                                    <p>
                                        Off unless you turn it on. Every deletion is written to the
                                        log with the name of what was removed.
                                    </p>
                                </Explainer>
                            </div>

                            <div class="border-t border-gray-200 dark:border-gray-700 pt-3 mt-4">
                                <h4
                                    class="text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                >
                                    Start a new recording automatically
                                </h4>
                                <p class="text-xs text-gray-500 dark:text-gray-400 mb-3">
                                    {rotation_summary(
                                        config.max_recording_size_mb,
                                        config.max_recording_minutes
                                    )}
                                </p>

                                <div class="grid gap-3 sm:grid-cols-2">
                                    <div>
                                        <label
                                            for="max_recording_minutes"
                                            class="block text-sm text-gray-700 dark:text-gray-200 mb-1"
                                        >
                                            After a length of time
                                        </label>
                                        <select
                                            id="max_recording_minutes"
                                            value={customRotationTime
                                                ? 'custom'
                                                : String(config.max_recording_minutes ?? '')}
                                            onchange={(e) => {
                                                const picked = e.currentTarget.value;
                                                if (picked === 'custom') {
                                                    customRotationTime = true;
                                                    if (!config) return;
                                                    config.max_recording_minutes ??= 45;
                                                } else {
                                                    customRotationTime = false;
                                                    if (!config) return;
                                                    config.max_recording_minutes =
                                                        picked === '' ? null : Number(picked);
                                                }
                                            }}
                                            class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                        >
                                            <option value="">Never</option>
                                            {#each TIME_PRESETS_MINUTES as minutes (minutes)}
                                                <option value={String(minutes)}
                                                    >Every {format_interval(minutes)}</option
                                                >
                                            {/each}
                                            <option value="custom">Another length of time</option>
                                        </select>
                                        {#if customRotationTime}
                                            <div class="mt-2 flex items-center gap-2">
                                                <input
                                                    type="number"
                                                    min={MIN_MINUTES}
                                                    aria-label="Minutes between recordings"
                                                    bind:value={config.max_recording_minutes}
                                                    class="w-28 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                                />
                                                <span
                                                    class="text-sm text-gray-600 dark:text-gray-300"
                                                    >minutes</span
                                                >
                                            </div>
                                        {/if}
                                    </div>

                                    <div>
                                        <label
                                            for="max_recording_size_mb"
                                            class="block text-sm text-gray-700 dark:text-gray-200 mb-1"
                                        >
                                            At a size
                                        </label>
                                        <select
                                            id="max_recording_size_mb"
                                            value={customRotationSize
                                                ? 'custom'
                                                : String(config.max_recording_size_mb ?? '')}
                                            onchange={(e) => {
                                                const picked = e.currentTarget.value;
                                                if (picked === 'custom') {
                                                    customRotationSize = true;
                                                    if (!config) return;
                                                    config.max_recording_size_mb ??= 20;
                                                } else {
                                                    customRotationSize = false;
                                                    if (!config) return;
                                                    config.max_recording_size_mb =
                                                        picked === '' ? null : Number(picked);
                                                }
                                            }}
                                            class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                        >
                                            <option value="">Never</option>
                                            {#each SIZE_PRESETS_MB as mb (mb)}
                                                <option value={String(mb)}>At {mb} MB</option>
                                            {/each}
                                            <option value="custom">Another size</option>
                                        </select>
                                        {#if customRotationSize}
                                            <div class="mt-2 flex items-center gap-2">
                                                <input
                                                    type="number"
                                                    min={MIN_SIZE_MB}
                                                    aria-label="Megabytes per recording"
                                                    bind:value={config.max_recording_size_mb}
                                                    class="w-28 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                                />
                                                <span
                                                    class="text-sm text-gray-600 dark:text-gray-300"
                                                    >MB</span
                                                >
                                            </div>
                                        {/if}
                                    </div>
                                </div>

                                {#if rotation_warning(config.max_recording_size_mb, config.max_recording_minutes)}
                                    <p class="mt-2 text-xs text-amber-600 dark:text-amber-400">
                                        {rotation_warning(
                                            config.max_recording_size_mb,
                                            config.max_recording_minutes
                                        )}
                                    </p>
                                {/if}

                                <Explainer summary="What splitting a recording changes.">
                                    <p>
                                        A capture left running is one file that grows until the disk
                                        fills. Splitting it keeps any single recording small enough
                                        to download over the device's own wifi, and means each piece
                                        is analysed and readable while capture carries on rather
                                        than only once you stop it.
                                    </p>
                                    <p>
                                        Set both and whichever arrives first wins, so a size limit
                                        acts as a ceiling on a busy stretch while the time limit
                                        still divides a quiet one. Detection is unaffected either
                                        way: analysers run over each recording as it closes, exactly
                                        as they do when you stop one by hand.
                                    </p>
                                    <p>
                                        A warning already on the display stays there across an
                                        automatic split. Rotation is the device's own housekeeping,
                                        and letting it clear a warning you had not read yet would
                                        hide the one thing you are here for.
                                    </p>
                                </Explainer>
                            </div>
                        </div>
                        <div
                            class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-3"
                        >
                            <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4">
                                GPS Settings
                            </h3>
                            <div>
                                <label
                                    for="gps_mode"
                                    class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >GPS Mode</label
                                >
                                <select
                                    id="gps_mode"
                                    bind:value={config.gps_mode}
                                    class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-none focus:ring-2 focus:ring-rayhunter-blue"
                                >
                                    <option value={GpsMode.Disabled}>Disabled</option>
                                    <option value={GpsMode.Fixed}>Fixed coordinates</option>
                                    <option value={GpsMode.Api}>API endpoint</option>
                                </select>
                                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                    {#if config.gps_mode === GpsMode.Api}
                                        POST latitude and longitude to <code>/api/gps</code> from any
                                        device on the network. Timestamp is derived from packet capture
                                        timing.
                                    {:else if config.gps_mode === GpsMode.Fixed}
                                        GPS coordinates are fixed to the values below.
                                    {:else}
                                        GPS is disabled; no coordinates will be tracked.
                                    {/if}
                                </p>
                            </div>
                            {#if config.gps_mode === GpsMode.Fixed}
                                <div>
                                    <label
                                        for="gps_fixed_latitude"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                        >Fixed Latitude</label
                                    >
                                    <input
                                        id="gps_fixed_latitude"
                                        type="number"
                                        min="-90"
                                        max="90"
                                        step="any"
                                        required
                                        bind:value={config.gps_fixed_latitude}
                                        placeholder="e.g. 37.7749"
                                        class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-none focus:ring-2 focus:ring-rayhunter-blue"
                                    />
                                    <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                        Decimal degrees, -90 to 90
                                    </p>
                                </div>
                                <div>
                                    <label
                                        for="gps_fixed_longitude"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                        >Fixed Longitude</label
                                    >
                                    <input
                                        id="gps_fixed_longitude"
                                        type="number"
                                        min="-180"
                                        max="180"
                                        step="any"
                                        required
                                        bind:value={config.gps_fixed_longitude}
                                        placeholder="e.g. -122.4194"
                                        class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-none focus:ring-2 focus:ring-rayhunter-blue"
                                    />
                                    <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                        Decimal degrees, -180 to 180
                                    </p>
                                </div>
                            {/if}
                        </div>
                        <div
                            class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-3"
                        >
                            <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4">
                                WebDAV Upload
                            </h3>
                            <p class="text-xs text-gray-500 dark:text-gray-400">
                                Once a recording has been closed for at least the configured age,
                                both the .qmdl and .ndjson files are uploaded in the background to
                                the WebDAV server.
                            </p>

                            <ExpandableInput
                                bind:value={config.webdav.url}
                                checkboxId="webdav_enabled"
                                inputId="webdav_url"
                                label="Enable WebDAV upload"
                                inputLabel="Server URL"
                                inputPlaceholder="https://dav.example.com/rayhunter/"
                                inputHelp="Files are uploaded via HTTP PUT under this base URL. No folders are created, and folders in this base URL are assumed to exist already."
                            >
                                <div>
                                    <label
                                        for="webdav_username"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >
                                        Username
                                    </label>
                                    <input
                                        id="webdav_username"
                                        type="text"
                                        bind:value={config.webdav.username}
                                        class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                    />
                                    <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                        Optional. Leave blank for unauthenticated uploads.
                                    </p>
                                </div>

                                <div>
                                    <label
                                        for="webdav_password"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >
                                        Password
                                    </label>
                                    <input
                                        id="webdav_password"
                                        type="password"
                                        bind:value={config.webdav.password}
                                        class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                    />
                                    <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                        A password without a username will be rejected and the
                                        request will be sent unauthenticated.
                                    </p>
                                </div>

                                <div>
                                    <label
                                        for="webdav_upload_timeout_secs"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >
                                        Upload Timeout (seconds)
                                    </label>
                                    <input
                                        id="webdav_upload_timeout_secs"
                                        type="number"
                                        min="1"
                                        bind:value={config.webdav.upload_timeout_secs}
                                        class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                    />
                                </div>

                                <div>
                                    <label
                                        for="webdav_poll_interval_secs"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >
                                        Poll Interval (seconds)
                                    </label>
                                    <input
                                        id="webdav_poll_interval_secs"
                                        type="number"
                                        min="1"
                                        bind:value={config.webdav.poll_interval_secs}
                                        class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                    />
                                    <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                        How often the worker checks for new entries to upload.
                                    </p>
                                </div>

                                <div>
                                    <label
                                        for="webdav_min_age_secs"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >
                                        Minimum Age Before Upload (seconds)
                                    </label>
                                    <input
                                        id="webdav_min_age_secs"
                                        type="number"
                                        min="0"
                                        bind:value={config.webdav.min_age_secs}
                                        class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                    />
                                    <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                        How long a recording must be closed before it becomes
                                        eligible for upload.
                                    </p>
                                </div>

                                <div class="flex items-center">
                                    <input
                                        id="webdav_delete_on_upload"
                                        type="checkbox"
                                        bind:checked={config.webdav.delete_on_upload}
                                        class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
                                    />
                                    <label
                                        for="webdav_delete_on_upload"
                                        class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                    >
                                        Delete on successful upload
                                    </label>
                                </div>
                                <p class="text-xs text-gray-500 dark:text-gray-400">
                                    When enabled, the local files are removed after a successful
                                    upload. Otherwise the manifest is just marked as uploaded.
                                </p>
                                <p class="text-xs text-gray-500 dark:text-gray-400">
                                    This is the second setting on this page that can delete a
                                    recording, alongside clearing out ones that found nothing
                                    further up. They are independent, and with both on a recording
                                    can leave the device for either reason.
                                </p>
                            </ExpandableInput>
                        </div>
                    </div>
                {:else if active === 'notifications'}
                    <div
                        id="panel-notifications"
                        role="tabpanel"
                        aria-labelledby="tab-notifications"
                        class="space-y-4"
                    >
                        <div
                            class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-3"
                        >
                            <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4">
                                Notification Settings
                            </h3>

                            <div class="flex items-center">
                                <input
                                    id="auto_check_updates"
                                    type="checkbox"
                                    bind:checked={config.auto_check_updates}
                                    class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
                                />
                                <label
                                    for="auto_check_updates"
                                    class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                >
                                    Automatically check for software updates
                                </label>
                            </div>
                            <p class="text-xs text-gray-500 dark:text-gray-400">
                                When enabled, Rayhunter periodically checks GitHub for new releases
                                and shows an update notice in the web UI.
                            </p>

                            <ExpandableInput
                                bind:value={config.ntfy_url}
                                checkboxId="ntfy_enabled"
                                inputId="ntfy_url"
                                label="Enable ntfy notifications"
                                inputLabel="ntfy URL"
                                inputPlaceholder="https://ntfy.sh/my-rayhunter"
                                inputHelp="Test button below uses the saved configuration URL, not the input above"
                            >
                                <div>
                                    <button
                                        type="button"
                                        onclick={send_test_notification}
                                        disabled={testingNotification}
                                        class="bg-rayhunter-blue hover:bg-rayhunter-dark-blue disabled:opacity-50 disabled:cursor-not-allowed text-white font-bold py-2 px-4 rounded-md flex flex-row gap-1 items-center"
                                    >
                                        {#if testingNotification}
                                            <div
                                                class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"
                                            ></div>
                                            Sending...
                                        {:else}
                                            <svg
                                                class="w-4 h-4"
                                                fill="none"
                                                stroke="currentColor"
                                                viewBox="0 0 24 24"
                                            >
                                                <path
                                                    stroke-linecap="round"
                                                    stroke-linejoin="round"
                                                    stroke-width="2"
                                                    d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8"
                                                ></path>
                                            </svg>
                                            Send Test Notification
                                        {/if}
                                    </button>
                                    {#if testMessage}
                                        <div
                                            class="mt-2 p-2 rounded-sm text-sm {testMessageType ===
                                            'error'
                                                ? 'bg-red-100 dark:bg-red-950 text-red-700 dark:text-red-300'
                                                : 'bg-green-100 dark:bg-green-950 text-green-700 dark:text-green-300'}"
                                        >
                                            {testMessage}
                                        </div>
                                    {/if}
                                </div>

                                <div class="space-y-2">
                                    <div
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >
                                        Enabled Notification Types
                                    </div>
                                    <div class="flex items-center">
                                        <input
                                            type="checkbox"
                                            id="enable_warning_notifications"
                                            value="Warning"
                                            bind:group={config.enabled_notifications}
                                        />
                                        <label
                                            for="enable_warning_notifications"
                                            class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                        >
                                            Warnings
                                        </label>
                                    </div>
                                    <div class="flex items-center">
                                        <input
                                            type="checkbox"
                                            id="enable_lowbattery_notifications"
                                            value="LowBattery"
                                            bind:group={config.enabled_notifications}
                                        />
                                        <label
                                            for="enable_lowbattery_notifications"
                                            class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                        >
                                            Low Battery
                                        </label>
                                    </div>
                                    <div class="flex items-center">
                                        <input
                                            type="checkbox"
                                            id="enable_update_notifications"
                                            value={enabled_notifications.Update}
                                            bind:group={config.enabled_notifications}
                                        />
                                        <label
                                            for="enable_update_notifications"
                                            class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                        >
                                            Software Updates
                                        </label>
                                    </div>
                                </div>
                            </ExpandableInput>
                        </div>
                    </div>
                {:else if active === 'network'}
                    <div
                        id="panel-network"
                        role="tabpanel"
                        aria-labelledby="tab-network"
                        class="space-y-4"
                    >
                        <div class="space-y-3">
                            <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4">
                                Switching WiFi off from the buttons
                            </h3>

                            <div class="flex items-center">
                                <input
                                    id="wifi_ap_button_toggle"
                                    type="checkbox"
                                    bind:checked={config.wifi_ap_button_toggle}
                                    class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 rounded-sm"
                                />
                                <label
                                    for="wifi_ap_button_toggle"
                                    class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                >
                                    Let a burst of button presses switch this device's WiFi off
                                </label>
                            </div>
                            <p class="text-xs text-gray-500 dark:text-gray-400">
                                Off by default. A hotspot running as a sensor does not need to be
                                broadcasting a network, and not broadcasting one saves power and
                                draws less attention.
                            </p>

                            {#if config.wifi_ap_button_toggle}
                                <div class="grid grid-cols-2 gap-3">
                                    <div>
                                        <label
                                            for="wifi_ap_toggle_presses"
                                            class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                        >
                                            Presses
                                        </label>
                                        <input
                                            id="wifi_ap_toggle_presses"
                                            type="number"
                                            min="4"
                                            max="20"
                                            bind:value={config.wifi_ap_toggle_presses}
                                            class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                        />
                                    </div>
                                    <div>
                                        <label
                                            for="wifi_ap_toggle_window_secs"
                                            class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                        >
                                            Within (seconds)
                                        </label>
                                        <input
                                            id="wifi_ap_toggle_window_secs"
                                            type="number"
                                            min="1"
                                            max="30"
                                            bind:value={config.wifi_ap_toggle_window_secs}
                                            class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                        />
                                    </div>
                                </div>
                                <p class="text-xs text-gray-500 dark:text-gray-400">
                                    Five presses within four seconds by default. Deliberate to do,
                                    and not something a device in a bag produces. Fewer than four
                                    presses is not accepted.
                                </p>

                                <div>
                                    <label
                                        for="wifi_ap_off_mode"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >
                                        How WiFi comes back
                                    </label>
                                    <select
                                        id="wifi_ap_off_mode"
                                        bind:value={config.wifi_ap_off_mode}
                                        class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                    >
                                        <option value="temporary">On its own, after a while</option>
                                        <option value="until_restart">
                                            Stays off until the device restarts
                                        </option>
                                    </select>
                                </div>

                                {#if config.wifi_ap_off_mode === 'temporary'}
                                    <div>
                                        <label
                                            for="wifi_ap_off_minutes"
                                            class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                        >
                                            Off for (minutes)
                                        </label>
                                        <input
                                            id="wifi_ap_off_minutes"
                                            type="number"
                                            min="1"
                                            bind:value={config.wifi_ap_off_minutes}
                                            class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                        />
                                    </div>
                                {/if}

                                <Explainer
                                    summary="How to get WiFi back, and why it can never lock you out."
                                >
                                    <p>
                                        <strong
                                            >Restarting the device always brings WiFi back.</strong
                                        >
                                        That is not something Rayhunter arranges, it is how the device
                                        behaves: the firmware starts the access point when it boots. So
                                        the worst case is a power cycle, which needs no cable, no menu
                                        and no password.
                                    </p>
                                    <p>
                                        Doing the gesture again while WiFi is off also brings it
                                        back, by restarting. On the hardware this was measured on,
                                        restarting is the only thing that reliably works: starting
                                        the access point again by hand does not.
                                    </p>
                                    <p>
                                        Because both routes restart the device, bringing WiFi back
                                        interrupts recording for about half a minute. Turning it off
                                        does not.
                                    </p>
                                </Explainer>
                            {/if}
                        </div>

                        {#if adbState && adbState !== 'not_adjustable'}
                            <div
                                class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-3"
                            >
                                <h3
                                    class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4"
                                >
                                    USB debugging (ADB)
                                </h3>

                                <div class="flex items-center">
                                    <input
                                        id="adb_enabled"
                                        type="checkbox"
                                        checked={config.adb_enabled ?? adbState === 'enabled'}
                                        onchange={(e) => {
                                            if (config)
                                                config.adb_enabled = e.currentTarget.checked;
                                        }}
                                        class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 rounded-sm"
                                    />
                                    <label
                                        for="adb_enabled"
                                        class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                    >
                                        Enable ADB over USB
                                    </label>
                                </div>
                                <p class="text-xs text-gray-500 dark:text-gray-400">
                                    Currently <strong
                                        >{adbState === 'enabled' ? 'on' : 'off'}</strong
                                    >. Changing this takes effect at the next restart, because the
                                    USB mode is chosen when the device boots.
                                </p>

                                <div
                                    class="rounded-md bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-950 dark:text-amber-200"
                                >
                                    ADB on this device runs as <strong>root</strong>. Anyone who can
                                    plug a cable into it gets complete control of it, without
                                    needing this page, the WiFi password or anything else. Worth
                                    having on if you are installing over USB or debugging; worth
                                    turning off before carrying the device somewhere.
                                </div>

                                <Explainer summary="Why this only appears on some devices.">
                                    <p>
                                        Devices pick their USB mode at boot from a number in a file,
                                        and the numbers mean different things on different hardware.
                                        A Moxee uses one value for the mode that includes ADB; an
                                        Orbic has a different value in the same file for a mode that
                                        also includes ADB.
                                    </p>
                                    <p>
                                        So this only offers to change a value that has been checked
                                        on real hardware. On anything else it does not appear, and
                                        whatever ADB the device already has is left exactly as it
                                        is. Writing the wrong number would select a USB mode nobody
                                        has tried, and getting that wrong takes the device off USB
                                        entirely, which is the one problem that needs a cable to
                                        fix.
                                    </p>
                                </Explainer>
                            </div>
                        {/if}

                        {#if config.device === 'orbic' || config.device === 'moxee' || config.device === 'tmobile' || config.device === 'wingtech'}
                            <div
                                class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-3"
                            >
                                <h3
                                    class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4"
                                >
                                    WiFi Client Mode
                                </h3>
                                <p class="text-xs text-gray-500 dark:text-gray-400">
                                    Connect the device to an existing WiFi network for internet
                                    access (e.g. notifications, remote access). The hotspot AP stays
                                    running alongside WiFi client mode.
                                </p>

                                <div class="flex items-center">
                                    <input
                                        id="wifi_enabled"
                                        type="checkbox"
                                        bind:checked={config.wifi_enabled}
                                        class="h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm"
                                    />
                                    <label
                                        for="wifi_enabled"
                                        class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                                    >
                                        Enable WiFi
                                    </label>
                                </div>
                                <p class="text-xs text-gray-500 dark:text-gray-400">
                                    Unchecking stops WiFi without clearing saved credentials.
                                </p>

                                {#if wifiStatus && config.wifi_enabled}
                                    {#if wifiStatus.state === 'connected'}
                                        <p class="text-xs text-green-600">
                                            Connected to "{wifiStatus.ssid}" ({wifiStatus.ip})
                                        </p>
                                    {:else if wifiStatus.state === 'connecting'}
                                        <p class="text-xs text-amber-600 dark:text-amber-400">
                                            Connecting...
                                        </p>
                                    {:else if wifiStatus.state === 'recovering'}
                                        <p class="text-xs text-amber-600 dark:text-amber-400">
                                            Recovering connection...
                                        </p>
                                    {:else if wifiStatus.state === 'dataPathDead'}
                                        <p class="text-xs text-amber-600 dark:text-amber-400">
                                            Data path stalled, attempting recovery...
                                        </p>
                                    {:else if wifiStatus.state === 'failed'}
                                        <p class="text-xs text-red-600 dark:text-red-400">
                                            Failed: {wifiStatus.error}
                                        </p>
                                    {/if}
                                {/if}

                                <div>
                                    <label
                                        for="wifi_ssid"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >
                                        WiFi Network Name (SSID)
                                    </label>
                                    <div class="flex gap-2">
                                        <input
                                            id="wifi_ssid"
                                            type="text"
                                            bind:value={config.wifi_ssid}
                                            placeholder="MyWiFiNetwork"
                                            class="flex-1 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                        />
                                        <button
                                            type="button"
                                            onclick={do_scan}
                                            disabled={scanning}
                                            class="px-3 py-2 text-sm bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 disabled:opacity-50 border border-gray-300 dark:border-gray-600 rounded-md"
                                        >
                                            {scanning ? 'Scanning...' : 'Scan'}
                                        </button>
                                    </div>
                                </div>

                                {#if scanError}
                                    <p class="text-xs text-red-600 dark:text-red-400">
                                        {scanError}
                                    </p>
                                {/if}

                                {#if scanResults.length > 0}
                                    <div
                                        class="border border-gray-200 dark:border-gray-700 rounded-md max-h-40 overflow-y-auto divide-y divide-gray-200 dark:divide-gray-700"
                                    >
                                        {#each scanResults as network}
                                            <button
                                                type="button"
                                                class="w-full px-3 py-2 text-left text-sm hover:bg-gray-50 dark:hover:bg-gray-800 flex justify-between"
                                                onclick={() => select_network(network)}
                                            >
                                                <span>{network.ssid}</span>
                                                <span class="text-gray-400 dark:text-gray-500"
                                                    >{network.signal_dbm} dBm &middot; {network.security}</span
                                                >
                                            </button>
                                        {/each}
                                    </div>
                                {/if}

                                {#if config.wifi_ssid}
                                    <div>
                                        <label
                                            for="wifi_security"
                                            class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                        >
                                            Security Type
                                        </label>
                                        <select
                                            id="wifi_security"
                                            bind:value={config.wifi_security}
                                            class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                        >
                                            <option value="wpa_psk">WPA2 (WPA-PSK)</option>
                                            <option value="sae">WPA3 (SAE)</option>
                                        </select>
                                    </div>
                                {/if}

                                <div>
                                    <label
                                        for="wifi_password"
                                        class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                    >
                                        WiFi Password
                                    </label>
                                    <input
                                        id="wifi_password"
                                        type="password"
                                        bind:value={config.wifi_password}
                                        placeholder="Enter password"
                                        class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                    />
                                    <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                        Changing the network requires re-entering the password.
                                    </p>
                                </div>

                                {#if config.wifi_ssid}
                                    <div>
                                        <label
                                            for="dns_servers"
                                            class="block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1"
                                        >
                                            DNS Servers
                                        </label>
                                        <input
                                            id="dns_servers"
                                            type="text"
                                            bind:value={dnsServersInput}
                                            placeholder="9.9.9.9, 149.112.112.112"
                                            class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue"
                                        />
                                        <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                            Comma-separated. Used when WiFi is active. Defaults to
                                            9.9.9.9, 149.112.112.112 (Quad9).
                                        </p>
                                    </div>
                                {/if}
                            </div>
                        {/if}
                    </div>
                {/if}

                <div
                    class="sticky bottom-0 z-10 -mx-2 mt-4 flex flex-wrap items-center gap-3 border-t border-gray-200 bg-white px-2 py-3 dark:border-gray-700 dark:bg-gray-900"
                >
                    <button
                        type="submit"
                        disabled={saving}
                        class="bg-blue-500 hover:bg-blue-700 disabled:opacity-50 text-white font-bold py-2 px-4 rounded-md flex flex-row gap-1 items-center"
                    >
                        {#if saving}
                            <div
                                class="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin"
                            ></div>
                            Saving...
                        {:else}
                            <svg
                                class="w-4 h-4"
                                fill="none"
                                stroke="currentColor"
                                viewBox="0 0 24 24"
                            >
                                <path
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    stroke-width="2"
                                    d="M5 13l4 4L19 7"
                                ></path>
                            </svg>
                            Apply and restart
                        {/if}
                    </button>
                    <p class="text-xs text-gray-500 dark:text-gray-400">
                        Applies every section, not only the one on screen, and restarts Rayhunter.
                    </p>
                </div>
            </form>
            {#if message}
                <div
                    class="mt-4 p-3 rounded-sm {messageType === 'error'
                        ? 'bg-red-100 dark:bg-red-950 text-red-700 dark:text-red-300'
                        : 'bg-green-100 dark:bg-green-950 text-green-700 dark:text-green-300'}"
                >
                    {message}
                </div>
            {/if}
        {:else}
            <div class="text-center py-4 text-red-600 dark:text-red-400">
                Failed to load configuration. Please try reloading the page.
            </div>
        {/if}
    </div>
</Modal>
