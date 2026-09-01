<script lang="ts">
    import Modal from './Modal.svelte';
    import Explainer from './Explainer.svelte';
    import {
        fetch_wifi_survey,
        fetch_wifi_rules,
        save_wifi_rules,
        type SurveyResponse,
        type SurveyEntry,
        type WifiRules,
        type UserRule,
        type UserRuleSet,
    } from '../utils.svelte';

    /**
     * What is on the air around the device, and what to be told about.
     *
     * This shows access points that are transmitting. It cannot show devices
     * that only listen or only probe, because the Wi-Fi firmware on every Orbic
     * variant tested refuses monitor mode. That limit is stated in the panel
     * itself rather than left for someone to work out from an empty table.
     */
    let { shown = $bindable() }: { shown: boolean } = $props();

    let survey = $state<SurveyResponse | null>(null);
    let scanning = $state(false);
    let error = $state<string | null>(null);
    let rules = $state<WifiRules | null>(null);
    let tab = $state<'networks' | 'alerts'>('networks');
    let auto = $state(false);
    let auto_timer: ReturnType<typeof setInterval> | null = null;

    /** Draft rule being added, kept separate so a half-typed rule is never saved. */
    let draft_name = $state('');
    let draft_kind = $state<'mac_prefix' | 'ssid_contains' | 'mac'>('mac_prefix');
    let draft_value = $state('');
    let draft_severity = $state<'low' | 'medium' | 'high'>('medium');
    let rules_error = $state<string | null>(null);
    let rules_saving = $state(false);

    async function scan() {
        if (scanning) return;
        scanning = true;
        error = null;
        try {
            survey = await fetch_wifi_survey();
        } catch (e) {
            error = `${e}`;
        } finally {
            scanning = false;
        }
    }

    async function load_rules() {
        try {
            rules = await fetch_wifi_rules();
        } catch (e) {
            rules_error = `${e}`;
        }
    }

    /** Scanning briefly occupies the radio, so the repeat is slow on purpose. */
    $effect(() => {
        if (auto && shown) {
            auto_timer = setInterval(scan, 15000);
        } else if (auto_timer) {
            clearInterval(auto_timer);
            auto_timer = null;
        }
        return () => {
            if (auto_timer) {
                clearInterval(auto_timer);
                auto_timer = null;
            }
        };
    });

    $effect(() => {
        if (shown && !survey && !scanning) {
            scan();
            load_rules();
        }
    });

    async function add_rule() {
        if (!rules || !draft_name.trim() || !draft_value.trim()) return;
        rules_saving = true;
        rules_error = null;
        const criterion =
            draft_kind === 'ssid_contains'
                ? { type: 'ssid_contains', substring: draft_value.trim() }
                : draft_kind === 'mac'
                  ? { type: 'mac', mac: draft_value.trim() }
                  : { type: 'mac_prefix', prefix: draft_value.trim() };
        const rule: UserRule = {
            // The id only has to be unique within the file; the daemon
            // namespaces it under "user." before it ever reaches a detection.
            id: `r${Date.now().toString(36)}`,
            name: draft_name.trim(),
            description: '',
            enabled: true,
            technology: 'wifi',
            severity: draft_severity,
            criteria: [criterion],
            cooldown_secs: 0,
        };
        const next: UserRuleSet = {
            rules: [...rules.rules.rules, rule],
            allowlist: rules.rules.allowlist,
        };
        try {
            rules = await save_wifi_rules(next);
            draft_name = '';
            draft_value = '';
            // Re-run so the new rule is reflected against what is on the air.
            await scan();
        } catch (e) {
            rules_error = `${e}`;
        } finally {
            rules_saving = false;
        }
    }

    async function remove_rule(id: string) {
        if (!rules) return;
        rules_saving = true;
        rules_error = null;
        try {
            rules = await save_wifi_rules({
                rules: rules.rules.rules.filter((r) => r.id !== id),
                allowlist: rules.rules.allowlist,
            });
            await scan();
        } catch (e) {
            rules_error = `${e}`;
        } finally {
            rules_saving = false;
        }
    }

    async function silence(bssid: string) {
        if (!rules) return;
        rules_saving = true;
        rules_error = null;
        try {
            rules = await save_wifi_rules({
                rules: rules.rules.rules,
                allowlist: [
                    ...rules.rules.allowlist,
                    { prefix: bssid, label: 'silenced from the survey' },
                ],
            });
            await scan();
        } catch (e) {
            rules_error = `${e}`;
        } finally {
            rules_saving = false;
        }
    }

    /** Strongest first, then alerting ones above the rest. */
    let sorted = $derived.by((): SurveyEntry[] => {
        if (!survey) return [];
        return [...survey.networks].sort((a, b) => {
            if (a.alerts.length !== b.alerts.length) return b.alerts.length - a.alerts.length;
            return (b.signal_dbm ?? -999) - (a.signal_dbm ?? -999);
        });
    });

    /** Rough bars from dBm, so signal reads at a glance without a number. */
    function bars(dbm: number | null): string {
        if (dbm === null) return '····';
        if (dbm >= -55) return '████';
        if (dbm >= -67) return '███·';
        if (dbm >= -78) return '██··';
        return '█···';
    }

    function confidence_class(c: string): string {
        switch (c) {
            case 'high':
                return 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200';
            case 'medium':
                return 'bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-200';
            case 'low':
                return 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-100';
            default:
                return 'bg-gray-200 text-gray-700 dark:bg-gray-700 dark:text-gray-200';
        }
    }
</script>

<Modal bind:shown title="Nearby Wi-Fi">
    <div class="flex h-full min-h-0 flex-col">
        <div class="flex flex-wrap items-center gap-2">
            <button
                onclick={scan}
                disabled={scanning}
                class="rounded-md border border-gray-300 px-3 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
            >
                {scanning ? 'Scanning…' : 'Scan now'}
            </button>
            <label class="flex items-center gap-1 text-sm">
                <input type="checkbox" bind:checked={auto} />
                Repeat every 15s
            </label>
            <div class="ml-auto flex gap-1">
                <button
                    onclick={() => (tab = 'networks')}
                    class="rounded-md px-3 py-1 text-sm {tab === 'networks'
                        ? 'bg-gray-200 dark:bg-gray-700'
                        : ''}">Networks</button
                >
                <button
                    onclick={() => (tab = 'alerts')}
                    class="rounded-md px-3 py-1 text-sm {tab === 'alerts'
                        ? 'bg-gray-200 dark:bg-gray-700'
                        : ''}">Alert rules</button
                >
            </div>
        </div>

        {#if error}
            <p
                class="mt-2 rounded-md bg-red-100 p-2 text-sm text-red-800 dark:bg-red-900 dark:text-red-200"
            >
                {error}
            </p>
        {/if}

        {#if tab === 'networks'}
            {#if survey}
                <p class="mt-2 text-xs text-gray-600 dark:text-gray-400">
                    {survey.networks.length} access point{survey.networks.length === 1 ? '' : 's'} on
                    {survey.interface}
                    {#if survey.alerting > 0}
                        · <span class="font-semibold text-amber-700 dark:text-amber-300"
                            >{survey.alerting} matched an alert</span
                        >
                    {:else}
                        · nothing matched an alert
                    {/if}
                    · {survey.builtin_rules_enabled} built-in and {survey.user_rules_enabled} of your
                    rules active
                </p>
            {/if}

            <div class="mt-2 min-h-40 flex-1 overflow-auto rounded-md bg-gray-100 dark:bg-gray-800">
                {#if !survey && scanning}
                    <p class="p-3 text-sm text-gray-500 dark:text-gray-400">Scanning…</p>
                {:else if sorted.length === 0}
                    <p class="p-3 text-sm text-gray-500 dark:text-gray-400">
                        No access points found. That means nothing nearby is broadcasting — not that
                        nothing is nearby.
                    </p>
                {/if}
                {#each sorted as net (net.bssid ?? net.ssid ?? Math.random())}
                    <div
                        class="border-b border-gray-200 p-2 last:border-0 dark:border-gray-700 {net
                            .alerts.length > 0
                            ? 'bg-amber-50 dark:bg-amber-950'
                            : ''}"
                    >
                        <div class="flex flex-wrap items-baseline gap-2">
                            <span class="font-semibold">
                                {#if net.hidden}
                                    <span class="italic text-gray-500 dark:text-gray-400"
                                        >(hidden network)</span
                                    >
                                {:else}
                                    {net.ssid ?? '(no name)'}
                                {/if}
                            </span>
                            <span class="font-mono text-xs text-gray-600 dark:text-gray-400"
                                >{net.bssid ?? '—'}</span
                            >
                            {#if net.randomised_address}
                                <span
                                    class="rounded-full bg-gray-200 px-2 py-0.5 text-xs text-gray-700 dark:bg-gray-700 dark:text-gray-200"
                                    title="The address is locally administered, so it is set by software rather than a manufacturer. Vendor matching does not apply to it."
                                    >randomised</span
                                >
                            {/if}
                            <span class="ml-auto flex items-center gap-2 text-xs">
                                <span class="font-mono" title="{net.signal_dbm ?? '?'} dBm"
                                    >{bars(net.signal_dbm)}</span
                                >
                                <span class="text-gray-600 dark:text-gray-400">
                                    {net.signal_dbm ?? '?'} dBm
                                    {#if net.channel}· ch {net.channel}{/if}
                                    {#if net.band}· {net.band}{/if}
                                </span>
                                <button
                                    onclick={() => net.bssid && silence(net.bssid)}
                                    disabled={!net.bssid || rules_saving}
                                    title="Never show this device again"
                                    class="text-gray-500 hover:text-gray-800 disabled:opacity-30 dark:text-gray-400 dark:hover:text-gray-100"
                                    >silence</button
                                >
                            </span>
                        </div>
                        {#each net.alerts as alert (alert.signature_id)}
                            <div class="mt-1 rounded-md bg-white p-2 text-xs dark:bg-gray-900">
                                <div class="flex flex-wrap items-center gap-2">
                                    <span
                                        class="rounded-full px-2 py-0.5 font-semibold uppercase {confidence_class(
                                            alert.confidence
                                        )}">{alert.confidence}</span
                                    >
                                    <span class="font-semibold"
                                        >{alert.vendor}{#if alert.product}
                                            — {alert.product}{/if}</span
                                    >
                                    <span class="font-mono text-gray-500 dark:text-gray-400"
                                        >{alert.signature_id}</span
                                    >
                                </div>
                                <ul
                                    class="mt-1 list-inside list-disc text-gray-700 dark:text-gray-300"
                                >
                                    {#each alert.matched_fields as why (why)}
                                        <li>{why}</li>
                                    {/each}
                                </ul>
                            </div>
                        {/each}
                    </div>
                {/each}
            </div>
        {:else}
            <div class="mt-2 min-h-40 flex-1 overflow-auto">
                {#if rules_error}
                    <p
                        class="mb-2 rounded-md bg-red-100 p-2 text-sm text-red-800 dark:bg-red-900 dark:text-red-200"
                    >
                        {rules_error}
                    </p>
                {/if}

                <form
                    class="flex flex-wrap items-end gap-2 rounded-md bg-gray-100 p-2 dark:bg-gray-800"
                    onsubmit={(e) => {
                        e.preventDefault();
                        add_rule();
                    }}
                >
                    <label class="flex flex-col text-xs">
                        Name
                        <input
                            bind:value={draft_name}
                            placeholder="The white van"
                            class="w-40 rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
                        />
                    </label>
                    <label class="flex flex-col text-xs">
                        Match on
                        <select
                            bind:value={draft_kind}
                            class="rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
                        >
                            <option value="mac_prefix">Address prefix (OUI)</option>
                            <option value="mac">Exact address</option>
                            <option value="ssid_contains">Network name contains</option>
                        </select>
                    </label>
                    <label class="flex flex-col text-xs">
                        Value
                        <input
                            bind:value={draft_value}
                            placeholder={draft_kind === 'ssid_contains'
                                ? 'FlockSafety'
                                : '70:b3:d5:7c:b'}
                            class="w-48 rounded-md border border-gray-300 px-2 py-1 font-mono text-sm dark:border-gray-600"
                        />
                    </label>
                    <label class="flex flex-col text-xs">
                        Severity
                        <select
                            bind:value={draft_severity}
                            class="rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
                        >
                            <option value="low">Low</option>
                            <option value="medium">Medium</option>
                            <option value="high">High</option>
                        </select>
                    </label>
                    <button
                        type="submit"
                        disabled={rules_saving || !draft_name.trim() || !draft_value.trim()}
                        class="rounded-md border border-gray-300 px-3 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
                        >Add</button
                    >
                </form>

                <h3 class="mt-3 text-sm font-semibold">Your rules</h3>
                {#if !rules || rules.rules.rules.length === 0}
                    <p class="text-sm text-gray-500 dark:text-gray-400">
                        None yet. Anything added here is matched alongside the built-in list.
                    </p>
                {:else}
                    {#each rules.rules.rules as rule (rule.id)}
                        <div
                            class="mt-1 flex items-center gap-2 rounded-md bg-gray-100 p-2 text-sm dark:bg-gray-800"
                        >
                            <span class="font-semibold">{rule.name}</span>
                            <span class="font-mono text-xs text-gray-600 dark:text-gray-400">
                                {rule.criteria
                                    .map((c) => c.prefix ?? c.mac ?? c.substring ?? '')
                                    .join(', ')}
                            </span>
                            <span class="text-xs text-gray-500 dark:text-gray-400"
                                >{rule.severity}</span
                            >
                            <button
                                onclick={() => remove_rule(rule.id)}
                                disabled={rules_saving}
                                class="ml-auto text-gray-500 hover:text-red-600 disabled:opacity-30 dark:text-gray-400"
                                >remove</button
                            >
                        </div>
                    {/each}
                {/if}

                {#if rules && rules.rules.allowlist.length > 0}
                    <h3 class="mt-3 text-sm font-semibold">Silenced</h3>
                    {#each rules.rules.allowlist as entry (entry.prefix)}
                        <div class="mt-1 font-mono text-xs text-gray-600 dark:text-gray-400">
                            {entry.prefix}
                            {#if entry.label}<span class="font-sans"> — {entry.label}</span>{/if}
                        </div>
                    {/each}
                {/if}

                <h3 class="mt-3 text-sm font-semibold">Built in</h3>
                <p class="text-xs text-gray-600 dark:text-gray-400">
                    Shipped with Rayhunter and not editable here. Ones marked off were reported by
                    others but not independently checked, so they stay quiet until someone turns
                    them on deliberately.
                </p>
                {#if rules}
                    {#each rules.builtin as b (b.id)}
                        <div class="mt-1 flex items-baseline gap-2 text-xs">
                            <span
                                class="rounded-full px-2 py-0.5 {b.enabled
                                    ? 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200'
                                    : 'bg-gray-200 text-gray-600 dark:bg-gray-700 dark:text-gray-300'}"
                                >{b.enabled ? 'on' : 'off'}</span
                            >
                            <span class="font-semibold">{b.vendor}</span>
                            <span class="font-mono text-gray-500 dark:text-gray-400">{b.id}</span>
                        </div>
                    {/each}
                {/if}
            </div>
        {/if}

        <Explainer summary="What this can and cannot see.">
            {#if survey}
                {#each survey.limitations as limit (limit)}
                    <p>{limit}</p>
                {/each}
            {:else}
                <p>
                    This lists access points that are transmitting nearby. A device that is only
                    listening, or that only sends probe requests, cannot be seen.
                </p>
            {/if}
            <p>
                Scanning happens on the same radio that serves the hotspot, so it is deliberately
                rate limited. Repeating every fifteen seconds is safe; faster is refused.
            </p>
        </Explainer>
    </div>
</Modal>
