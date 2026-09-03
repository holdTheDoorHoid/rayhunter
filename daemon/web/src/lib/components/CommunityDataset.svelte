<script lang="ts">
    import { onMount } from 'svelte';
    import type { TelemetryConfig, TelemetryProbe, TelemetryStatus } from '../utils.svelte';
    import {
        get_telemetry_status,
        probe_telemetry_server,
        telemetry_rotate_key,
        telemetry_send_now,
    } from '../utils.svelte';
    import Explainer from './Explainer.svelte';

    /**
     * The settings for contributing recordings to a community dataset.
     *
     * Everything here is the owner's choice, and the defaults are the least
     * that leaves the device. The page says what each choice sends rather
     * than naming a tier and hoping, and it warns rather than blocks: the
     * device refuses only the one combination that cannot be honest, a full
     * contribution without the acknowledgement.
     */
    let { telemetry = $bindable() }: { telemetry: TelemetryConfig } = $props();

    let status: TelemetryStatus | null = $state(null);
    let probe: TelemetryProbe | null = $state(null);
    let probing = $state(false);
    let probeError = $state('');
    let actionMessage = $state('');
    let allowedNetworksText = $state(telemetry.allowed_networks.join(', '));
    let minAgeMinutes = $state(Math.round(telemetry.min_age_secs / 60));
    let pollMinutes = $state(Math.round(telemetry.poll_interval_secs / 60));

    const date_formatter = new Intl.DateTimeFormat(undefined, {
        timeStyle: 'short',
        dateStyle: 'short',
    });

    function when(iso: string | null): string {
        if (!iso) return 'never';
        const d = new Date(iso);
        return isNaN(d.getTime()) ? iso : date_formatter.format(d);
    }

    async function refresh_status() {
        try {
            status = await get_telemetry_status();
        } catch {
            // The page still works without it; the panel says it could not load.
            status = null;
        }
    }

    onMount(() => {
        refresh_status();
        const timer = setInterval(() => {
            if (!document.hidden) refresh_status();
        }, 10_000);
        return () => clearInterval(timer);
    });

    async function check_server() {
        probing = true;
        probeError = '';
        probe = null;
        try {
            probe = await probe_telemetry_server(telemetry.server_url);
        } catch (e) {
            probeError = `${e}`;
        } finally {
            probing = false;
        }
    }

    function pin_keys() {
        if (!probe) return;
        telemetry.ingest_public_key = probe.info.ingest_public_key;
        telemetry.archive_public_key = probe.info.archive_public_key ?? null;
        telemetry.server_name = probe.info.name;
        probe = { ...probe, matches_pinned: true };
    }

    function set_acknowledged(checked: boolean) {
        telemetry.full_tier_acknowledged_at = checked ? new Date().toISOString() : null;
    }

    function update_allowed_networks() {
        telemetry.allowed_networks = allowedNetworksText
            .split(',')
            .map((s) => s.trim())
            .filter((s) => s.length > 0);
    }

    function update_min_age() {
        telemetry.min_age_secs = Math.max(0, Math.round(minAgeMinutes)) * 60;
    }

    function update_poll() {
        telemetry.poll_interval_secs = Math.max(1, Math.round(pollMinutes)) * 60;
    }

    async function send_now() {
        actionMessage = '';
        try {
            await telemetry_send_now();
            actionMessage = 'Asked the device to look for recordings to send now.';
            setTimeout(refresh_status, 1500);
        } catch (e) {
            actionMessage = `${e}`;
        }
    }

    async function new_identity() {
        actionMessage = '';
        try {
            const key_id = await telemetry_rotate_key();
            actionMessage = `New signing identity ${key_id}. Earlier contributions can still be withdrawn.`;
            await refresh_status();
        } catch (e) {
            actionMessage = `${e}`;
        }
    }

    let keysPinned = $derived(!!telemetry.ingest_public_key);
    let fullNeedsArchiveKey = $derived(telemetry.tier === 'full' && !telemetry.archive_public_key);
    let fullNeedsAck = $derived(
        telemetry.tier === 'full' && telemetry.full_tier_acknowledged_at === null
    );
    let shortKey = (key: string | null) => (key ? key.slice(0, 16) : '');

    const inputClass =
        'w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-hidden focus:ring-2 focus:ring-rayhunter-blue';
    const labelClass = 'block text-sm font-medium text-gray-700 dark:text-gray-200 mb-1';
    const helpClass = 'text-xs text-gray-500 dark:text-gray-400 mt-1';
    const checkClass =
        'h-4 w-4 text-rayhunter-blue focus:ring-rayhunter-blue border-gray-300 dark:border-gray-600 rounded-sm';
    const buttonClass =
        'bg-rayhunter-blue hover:bg-rayhunter-dark-blue disabled:opacity-50 disabled:cursor-not-allowed text-white font-bold py-2 px-4 rounded-md';
    const warnClass =
        'mt-2 p-3 rounded-md border border-amber-400 bg-amber-50 text-amber-900 dark:bg-amber-950 dark:text-amber-200 dark:border-amber-700 text-sm space-y-1';
</script>

<div class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-6 space-y-3">
    <h3 class="text-lg font-semibold text-gray-800 dark:text-gray-100 mb-2">Community dataset</h3>
    <p class="text-xs text-gray-500 dark:text-gray-400">
        Send recordings that raised a warning to a community-run collection, so patterns across many
        devices can be seen and captures can be studied. Off unless you turn it on. EFF does not run
        such a service; anyone can, with the tools in this repository.
    </p>

    <div class="flex items-center">
        <input
            id="telemetry_enabled"
            type="checkbox"
            bind:checked={telemetry.enabled}
            class={checkClass}
        />
        <label for="telemetry_enabled" class="ml-2 block text-sm text-gray-700 dark:text-gray-200">
            Contribute recordings to a community dataset
        </label>
    </div>
    <Explainer summary="What leaves the device, and what never does.">
        <p>
            By default only the <strong>shareable</strong> version of a recording is sent: the capture
            with this device's own IMSI, IMEI and temporary identity set to zero, the analysis report,
            the device details with the home network and WiFi removed, and one location rounded to about
            ten kilometres. The raw capture, your recording names and notes, the WiFi network name, and
            any recording with a demo warning never go.
        </p>
        <p>
            Everything is encrypted on this device to the service's keys and signed with a key of
            this device's own before it leaves. Uploads wait an hour by default and, by default,
            only happen over WiFi, so nothing is ever sent from where a warning happened or through
            the tower that raised it.
        </p>
        <p>
            Nothing appears on the service's public site until a person has looked at it, and you
            can withdraw any contribution from the recording's row in the history.
        </p>
    </Explainer>

    {#if telemetry.enabled}
        <!-- The service -->
        <div>
            <label for="telemetry_server_url" class={labelClass}>Service address</label>
            <div class="flex gap-2">
                <input
                    id="telemetry_server_url"
                    type="url"
                    bind:value={telemetry.server_url}
                    placeholder="https://data.example.org"
                    class={inputClass}
                />
                <button
                    type="button"
                    class={buttonClass}
                    onclick={check_server}
                    disabled={probing || telemetry.server_url.trim() === ''}
                >
                    {probing ? 'Checking…' : 'Check server'}
                </button>
            </div>
            <p class={helpClass}>
                Checking reads the service's description and shows the fingerprints of its keys.
                Nothing is sent until you pin those keys and save.
            </p>
            {#if probeError}
                <p class="mt-1 text-sm text-red-700 dark:text-red-300">{probeError}</p>
            {/if}
            {#if probe}
                <div
                    class="mt-2 p-3 rounded-md border border-gray-200 dark:border-gray-700 text-sm space-y-1"
                >
                    <div><strong>{probe.info.name}</strong></div>
                    {#if probe.info.description}<div>{probe.info.description}</div>{/if}
                    {#if probe.info.contact}<div>Contact: {probe.info.contact}</div>{/if}
                    {#if probe.info.site_url}
                        <div>
                            Published at <a
                                class="text-blue-700 dark:text-blue-300 underline"
                                href={probe.info.site_url}
                                target="_blank"
                                rel="noopener">{probe.info.site_url}</a
                            >
                        </div>
                    {/if}
                    <div>
                        Accepts: {probe.info.accepted_tiers.join(', ')}. Summary up to {Math.round(
                            probe.info.max_summary_bytes / 1048576
                        )} MB{probe.info.archive_public_key
                            ? `, full capture up to ${Math.round(probe.info.max_capture_bytes / 1048576)} MB`
                            : ''}.
                    </div>
                    <div class="font-mono text-xs break-all">
                        Ingest key {probe.ingest_key_id}: {probe.ingest_fingerprint}
                    </div>
                    {#if probe.archive_key_id}
                        <div class="font-mono text-xs break-all">
                            Archive key {probe.archive_key_id}: {probe.archive_fingerprint}
                        </div>
                    {/if}
                    {#if probe.matches_pinned === false}
                        <div class={warnClass}>
                            These are not the keys pinned on this device. A service changing its
                            keys is unusual. If its operator announced the change, pin the new ones;
                            if not, do not.
                        </div>
                    {/if}
                    <button type="button" class={buttonClass} onclick={pin_keys}>
                        {probe.matches_pinned ? 'Keys already pinned' : 'Pin these keys'}
                    </button>
                </div>
            {/if}
            {#if keysPinned}
                <p class={helpClass}>
                    Pinned: {telemetry.server_name ?? 'service'}, ingest key {shortKey(
                        telemetry.ingest_public_key
                    )}…{telemetry.archive_public_key ? ', archive key pinned' : ''}
                </p>
            {:else}
                <p class="mt-1 text-sm text-amber-700 dark:text-amber-300">
                    No keys pinned yet. Check the server and pin its keys before saving, or saving
                    will be refused.
                </p>
            {/if}
        </div>

        <!-- What to send -->
        <fieldset>
            <legend class={labelClass}>What to send</legend>
            <div class="space-y-2">
                <label class="flex items-start gap-2 text-sm text-gray-700 dark:text-gray-200">
                    <input
                        type="radio"
                        name="telemetry_tier"
                        value="summary"
                        bind:group={telemetry.tier}
                        class="mt-1"
                    />
                    <span>
                        <strong>Shareable version</strong> (recommended). The capture with this device's
                        identifiers zeroed, the analysis, and non-identifying device details. No raw capture.
                    </span>
                </label>
                <label class="flex items-start gap-2 text-sm text-gray-700 dark:text-gray-200">
                    <input
                        type="radio"
                        name="telemetry_tier"
                        value="full"
                        bind:group={telemetry.tier}
                        class="mt-1"
                    />
                    <span>
                        <strong>Full recording.</strong> Everything above plus the raw capture with your
                        identifiers intact, encrypted to a key the service keeps offline, for researchers
                        who need the whole thing.
                    </span>
                </label>
            </div>
            {#if telemetry.tier === 'full'}
                <div class={warnClass}>
                    <p><strong>The full recording identifies you.</strong> It contains:</p>
                    <ul class="list-disc ml-5">
                        <li>
                            your SIM's permanent identity (IMSI) and this device's IMEI, wherever
                            the network asked for them
                        </li>
                        <li>your home network (carrier)</li>
                        <li>
                            the location track at the precision chosen below, and signal strengths
                            that help place it
                        </li>
                        <li>every message the device exchanged with towers during the recording</li>
                    </ul>
                    <p>
                        The service's internet-facing server cannot read it; only whoever holds the
                        offline archive key can. You are trusting that person with who you are and
                        where you were.
                    </p>
                    <label class="flex items-start gap-2 mt-1">
                        <input
                            type="checkbox"
                            checked={telemetry.full_tier_acknowledged_at !== null}
                            onchange={(e) =>
                                set_acknowledged((e.currentTarget as HTMLInputElement).checked)}
                            class={checkClass + ' mt-0.5'}
                        />
                        <span
                            >I understand what the full recording contains and who can read it.</span
                        >
                    </label>
                    {#if fullNeedsAck}
                        <p>Saving will be refused until this is ticked.</p>
                    {/if}
                    {#if fullNeedsArchiveKey}
                        <p>
                            The pinned service offers no archive key, so it cannot take full
                            recordings. Choose the shareable version, or check the server again.
                        </p>
                    {/if}
                    <label class="flex items-center gap-2 mt-1">
                        <input
                            type="checkbox"
                            bind:checked={telemetry.include_notes}
                            class={checkClass}
                        />
                        <span>Also include the names and notes I typed for recordings</span>
                    </label>
                </div>
            {/if}
        </fieldset>

        <!-- Which recordings -->
        <div>
            <label for="telemetry_min_severity" class={labelClass}>Which recordings</label>
            <select
                id="telemetry_min_severity"
                bind:value={telemetry.min_severity}
                class={inputClass}
            >
                <option value="low">Any recording that raised a warning</option>
                <option value="medium">Only medium or high warnings</option>
                <option value="high">Only high warnings</option>
            </select>
            <div class="flex items-center mt-2">
                <input
                    id="telemetry_include_clean"
                    type="checkbox"
                    bind:checked={telemetry.include_clean_recordings}
                    class={checkClass}
                />
                <label
                    for="telemetry_include_clean"
                    class="ml-2 block text-sm text-gray-700 dark:text-gray-200"
                >
                    Also send recordings that raised no warning, as baseline data
                </label>
            </div>
            <p class={helpClass}>
                Recordings containing a demo warning and the recording currently being written are
                never sent. Any recording can be kept out from its row in the history.
            </p>
        </div>

        <!-- Location -->
        <div>
            <label for="telemetry_location" class={labelClass}>Location</label>
            <select id="telemetry_location" bind:value={telemetry.location} class={inputClass}>
                <option value="none">Do not send a location</option>
                <option value="coarse">Rounded to about 10 km (recommended)</option>
                <option value="neighborhood">Rounded to about 1 km</option>
                <option value="exact">Exact, as recorded</option>
            </select>
            <p class={helpClass}>
                Only applies when this device records a location (see the Recordings section). A
                cell identity already says roughly where the device was, at the resolution of a
                tower's coverage; a 10 km point adds nothing an attacker could not get from it, and
                makes a map possible.
            </p>
            {#if telemetry.location === 'exact'}
                <p class="mt-1 text-sm text-amber-700 dark:text-amber-300">
                    An exact location with a timestamp says where you were and when. With the
                    shareable version, that is the one thing in the bundle that is about you rather
                    than about the device.
                </p>
            {/if}
        </div>

        <!-- When and how -->
        <fieldset>
            <legend class={labelClass}>When to upload</legend>
            <div class="space-y-2">
                <label class="flex items-start gap-2 text-sm text-gray-700 dark:text-gray-200">
                    <input
                        type="radio"
                        name="telemetry_network"
                        value="wifi_only"
                        bind:group={telemetry.network}
                        class="mt-1"
                    />
                    <span>
                        <strong>Only over WiFi</strong> (recommended). Waits until this device's WiFi
                        client is joined to a network, so uploads happen at home and not through a tower
                        that may be the one being reported.
                    </span>
                </label>
                <label class="flex items-start gap-2 text-sm text-gray-700 dark:text-gray-200">
                    <input
                        type="radio"
                        name="telemetry_network"
                        value="any"
                        bind:group={telemetry.network}
                        class="mt-1"
                    />
                    <span
                        ><strong>Over WiFi or cellular data</strong>, whichever the device has.</span
                    >
                </label>
            </div>
            {#if telemetry.network === 'wifi_only'}
                <div class="mt-2">
                    <label for="telemetry_allowed_networks" class={labelClass}>
                        Only from these WiFi networks (optional)
                    </label>
                    <input
                        id="telemetry_allowed_networks"
                        type="text"
                        bind:value={allowedNetworksText}
                        oninput={update_allowed_networks}
                        placeholder="Home, Office"
                        class={inputClass}
                    />
                    <p class={helpClass}>
                        Network names, separated by commas. Empty means any network the client
                        joins.
                    </p>
                </div>
            {:else}
                <p class="mt-1 text-sm text-amber-700 dark:text-amber-300">
                    Over cellular data, an upload from where a warning happened goes through the
                    tower that raised it, and the operator of a fake tower would see this device
                    talking to the service. The minimum age below still applies.
                </p>
            {/if}
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-3 mt-2">
                <div>
                    <label for="telemetry_min_age" class={labelClass}>
                        Wait after a recording closes (minutes)
                    </label>
                    <input
                        id="telemetry_min_age"
                        type="number"
                        min="0"
                        bind:value={minAgeMinutes}
                        oninput={update_min_age}
                        class={inputClass}
                    />
                    <p class={helpClass}>
                        An hour by default, so nothing is sent from where the warning was.
                    </p>
                </div>
                <div>
                    <label for="telemetry_poll" class={labelClass}>Check every (minutes)</label>
                    <input
                        id="telemetry_poll"
                        type="number"
                        min="1"
                        bind:value={pollMinutes}
                        oninput={update_poll}
                        class={inputClass}
                    />
                </div>
            </div>
        </fieldset>

        <!-- Status -->
        <div class="p-3 rounded-md border border-gray-200 dark:border-gray-700 text-sm space-y-1">
            <div class="font-semibold text-gray-800 dark:text-gray-100">Right now</div>
            {#if !status}
                <div class="text-gray-600 dark:text-gray-400">Status not available.</div>
            {:else if !status.enabled}
                <div class="text-gray-600 dark:text-gray-400">
                    Not running. Save this page to start contributing with the settings above.
                </div>
            {:else}
                <div>
                    Uploads: {status.network}{status.busy ? `, working on ${status.busy}` : ''}
                </div>
                {#if status.server_keys_changed}
                    <div class="text-amber-700 dark:text-amber-300">
                        The service presented keys other than the pinned ones. Nothing is being
                        sent. Check the server above to see them.
                    </div>
                {/if}
                <div>
                    Sent this session: {status.submitted_count}. Last success: {when(
                        status.last_success_at
                    )}.
                </div>
                {#if status.last_error}
                    <div class="text-red-700 dark:text-red-300">
                        Last problem: {status.last_error}{status.next_attempt_at
                            ? ` (next try ${when(status.next_attempt_at)})`
                            : ''}
                    </div>
                {/if}
                {#if status.queued.length > 0}
                    <div>Waiting to send: {status.queued.join(', ')}</div>
                {/if}
                {#if status.skipped.length > 0}
                    <details>
                        <summary class="cursor-pointer">
                            Not sending {status.skipped.length} recording{status.skipped.length ===
                            1
                                ? ''
                                : 's'}
                        </summary>
                        <ul class="ml-4 list-disc text-gray-600 dark:text-gray-400">
                            {#each status.skipped as s (s.name)}
                                <li>{s.name}: {s.reason}</li>
                            {/each}
                        </ul>
                    </details>
                {/if}
                <div class="text-gray-600 dark:text-gray-400">
                    Signing identity {status.submitter_key_id ??
                        'not made yet'}{status.key_created_at
                        ? `, made ${when(status.key_created_at)}`
                        : ''}. Replaced every {telemetry.key_rotation_days} days.
                </div>
                <div class="flex gap-2 mt-1">
                    <button type="button" class={buttonClass} onclick={send_now}>Send now</button>
                    <button type="button" class={buttonClass} onclick={new_identity}>
                        New signing identity
                    </button>
                </div>
            {/if}
            {#if actionMessage}
                <div class="text-gray-700 dark:text-gray-300">{actionMessage}</div>
            {/if}
        </div>
    {/if}
</div>
