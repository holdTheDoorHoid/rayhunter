<script lang="ts">
    import Explainer from './Explainer.svelte';
    import {
        rsrp_quality,
        operator_name,
        format_plmn,
        split_cell_id,
        reading_age,
        is_stale,
        missing_protections,
        skipped_percent,
        health_verdict,
        tracking_area_changes,
        type CellInfo,
    } from '../cellInfo';
    import {
        band_for_earfcn,
        downlink_mhz,
        uplink_earfcn,
        uplink_mhz,
        signal_bars,
        asu_from_rsrp,
        timing_advance_metres,
    } from '../lteBands';

    let { info, recording }: { info: CellInfo | null; recording: boolean } = $props();

    const QUALITY_CLASS = {
        excellent: 'text-green-700 dark:text-green-300',
        good: 'text-green-700 dark:text-green-300',
        fair: 'text-amber-600 dark:text-amber-400',
        poor: 'text-red-600 dark:text-red-400',
    } as const;

    function dbm(value: number): string {
        return `${value.toFixed(1)} dBm`;
    }

    function db(value: number): string {
        return `${value.toFixed(1)} dB`;
    }

    function clock(iso: string): string {
        return new Date(iso).toLocaleTimeString();
    }

    let serving = $derived(info?.serving ?? null);
    let identity = $derived(serving?.identity ?? null);
    let split = $derived(identity?.cell_id != null ? split_cell_id(identity.cell_id) : null);
    let carrier = $derived(operator_name(identity?.mcc ?? null, identity?.mnc ?? null));
    let plmn = $derived(format_plmn(identity?.mcc ?? null, identity?.mnc ?? null));
    // Ticks so the age text counts up rather than freezing at render time.
    let now = $state(Date.now());
    $effect(() => {
        const t = setInterval(() => (now = Date.now()), 5000);
        return () => clearInterval(t);
    });

    let stale = $derived(serving ? is_stale(serving.last_seen, now) : false);
    let encryption = $derived(info?.encryption ?? null);
    let missing = $derived(missing_protections(encryption));
    let unencrypted = $derived(missing.length > 0);
    let verdict = $derived(info ? health_verdict(info.health, 120, now) : 'starting');
    let tacChanges = $derived(info ? tracking_area_changes(info.history) : []);
    let quality = $derived(serving ? rsrp_quality(serving.signal.rsrp_dbm) : null);
    let bandInfo = $derived(serving ? band_for_earfcn(serving.earfcn) : null);
    let dl = $derived(serving ? downlink_mhz(serving.earfcn) : null);
    let ul = $derived(serving ? uplink_mhz(serving.earfcn) : null);
    let ulEarfcn = $derived(serving ? uplink_earfcn(serving.earfcn) : null);
    let bars = $derived(serving ? signal_bars(serving.signal.rsrp_dbm) : 0);
    let asu = $derived(serving ? asu_from_rsrp(serving.signal.rsrp_dbm) : null);

    /** Rows for the advanced table. Anything unknown is shown as such rather than hidden. */
    let advanced = $derived.by(() => {
        if (!serving) return [] as { label: string; value: string; hint?: string }[];
        const rows: { label: string; value: string; hint?: string }[] = [];
        const unknown = 'not captured';
        rows.push({ label: 'Cell identity', value: identity?.cell_id?.toString() ?? unknown });
        if (split) {
            rows.push({ label: 'Base station (eNodeB)', value: split.enodeb_id.toString() });
            rows.push({ label: 'Sector', value: split.sector_id.toString() });
        }
        rows.push({ label: 'Tracking area code', value: identity?.tac?.toString() ?? unknown });
        rows.push({ label: 'Network (PLMN)', value: plmn ?? unknown });
        rows.push({ label: 'Physical cell id (PCI)', value: serving.pci.toString() });
        rows.push({
            label: 'Band',
            value: bandInfo ? `${bandInfo.band} (${bandInfo.name})` : unknown,
        });
        rows.push({ label: 'Downlink EARFCN', value: serving.earfcn.toString() });
        rows.push({ label: 'Downlink frequency', value: dl !== null ? `${dl} MHz` : unknown });
        rows.push({ label: 'Uplink EARFCN', value: ulEarfcn?.toString() ?? unknown });
        rows.push({ label: 'Uplink frequency', value: ul !== null ? `${ul} MHz` : unknown });
        rows.push({
            label: 'Duplex',
            value: bandInfo
                ? bandInfo.tdd
                    ? 'TDD, one frequency shared'
                    : 'FDD, separate frequencies'
                : unknown,
        });
        rows.push({ label: 'RSRP', value: `${serving.signal.rsrp_dbm.toFixed(1)} dBm` });
        rows.push({ label: 'RSRQ', value: `${serving.signal.rsrq_db.toFixed(1)} dB` });
        rows.push({ label: 'RSSI', value: `${serving.signal.rssi_dbm.toFixed(1)} dBm` });
        rows.push({
            label: 'RSRP (averaged)',
            value:
                serving.signal.avg_rsrp_dbm !== null && serving.signal.avg_rsrp_dbm !== undefined
                    ? `${serving.signal.avg_rsrp_dbm.toFixed(1)} dBm`
                    : unknown,
        });
        rows.push({
            label: 'Reselection search threshold',
            value: serving.search_threshold?.toString() ?? unknown,
        });
        rows.push({ label: 'Signal bars', value: `${bars} of 4` });
        rows.push({ label: 'ASU', value: asu?.toString() ?? unknown });
        rows.push({
            label: 'Timing advance',
            value:
                serving.timing_advance !== null && serving.timing_advance !== undefined
                    ? `${serving.timing_advance} (about ${timing_advance_metres(serving.timing_advance)} m away)`
                    : 'not seen yet',
        });
        rows.push({ label: 'Last measurement', value: clock(serving.last_seen) });
        return rows;
    });
</script>

<div class="border border-gray-300 dark:border-gray-700 rounded-md p-4">
    <div class="flex items-baseline justify-between gap-2">
        <h2 class="text-xl">
            Cell Site
            {#if verdict === 'blind' || verdict === 'stalled'}
                <span
                    class="ml-1 rounded-sm bg-red-100 px-1.5 py-0.5 text-xs font-medium text-red-800 dark:bg-red-950 dark:text-red-300"
                >
                    {verdict === 'stalled' ? 'no messages arriving' : 'mostly undecodable'}
                </span>
            {:else if verdict === 'degraded'}
                <span
                    class="ml-1 rounded-sm bg-amber-100 px-1.5 py-0.5 text-xs font-medium text-amber-800 dark:bg-amber-950 dark:text-amber-300"
                >
                    partial coverage
                </span>
            {/if}
        </h2>
        {#if serving}
            <span
                class="text-xs {stale
                    ? 'text-amber-600 dark:text-amber-400'
                    : 'text-gray-500 dark:text-gray-400'}"
            >
                measured {reading_age(serving.last_seen, now)}
            </span>
        {/if}
    </div>

    {#if !info?.has_data}
        <p class="mt-2 text-sm text-gray-500 dark:text-gray-400">
            {#if recording}
                Waiting for the first measurement from the radio. This usually appears within a
                minute of starting a recording.
            {:else}
                Nothing to show while recording is stopped. These readings come from the modem
                diagnostic stream, which only runs during a recording. Start one to see the tower
                you are connected to.
            {/if}
        </p>
    {:else if serving}
        <!-- Serving cell: the one tower we can actually identify. -->
        <div class="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 sm:grid-cols-4">
            <div>
                <div class="text-xs text-gray-500 dark:text-gray-400">Signal (RSRP)</div>
                <div class="text-lg {quality ? QUALITY_CLASS[quality] : ''}">
                    {dbm(serving.signal.rsrp_dbm)}
                </div>
                <div class="text-xs text-gray-500 dark:text-gray-400 capitalize">{quality}</div>
            </div>
            <div>
                <div class="text-xs text-gray-500 dark:text-gray-400">Quality (RSRQ)</div>
                <div class="text-lg">{db(serving.signal.rsrq_db)}</div>
            </div>
            <div>
                <div class="text-xs text-gray-500 dark:text-gray-400">Band</div>
                <div class="text-lg">
                    {bandInfo ? `${bandInfo.band}` : (serving.band ?? 'unknown')}
                    {#if bandInfo}<span class="text-sm text-gray-500 dark:text-gray-400"
                            >{bandInfo.name}</span
                        >{/if}
                </div>
                <div class="text-xs text-gray-500 dark:text-gray-400">
                    {#if dl !== null}{dl} MHz ·
                    {/if}EARFCN {serving.earfcn}
                </div>
            </div>
            <div>
                <div class="text-xs text-gray-500 dark:text-gray-400">Operator</div>
                <div class="text-lg">{carrier ?? plmn ?? 'unknown'}</div>
                {#if carrier && plmn}
                    <div class="text-xs text-gray-500 dark:text-gray-400">{plmn}</div>
                {/if}
            </div>
        </div>

        {#if stale}
            <p class="mt-2 text-xs text-amber-600 dark:text-amber-400">
                This reading is not current. The modem only reports measurements while it is
                actively working, so an attached device sitting idle stops sending them. The values
                below are the last ones seen, not live.
            </p>
        {/if}

        {#if encryption}
            <div
                class="mt-3 rounded-md border p-2 {unencrypted
                    ? 'border-red-500 bg-red-100 dark:border-red-800 dark:bg-red-950'
                    : 'border-gray-200 dark:border-gray-700'}"
            >
                <div class="text-xs text-gray-500 dark:text-gray-400">Protection in use</div>
                <div class="mt-1 grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
                    {#if encryption.rrc_cipher || encryption.rrc_integrity}
                        <div class="col-span-2 text-xs text-gray-500 dark:text-gray-400">
                            Radio link, between your device and the tower
                        </div>
                        <div>Encryption: {encryption.rrc_cipher ?? 'not seen'}</div>
                        <div>Integrity: {encryption.rrc_integrity ?? 'not seen'}</div>
                    {/if}
                    {#if encryption.nas_cipher || encryption.nas_integrity}
                        <div class="col-span-2 mt-1 text-xs text-gray-500 dark:text-gray-400">
                            Core network, signalling with your operator
                        </div>
                        <div>Encryption: {encryption.nas_cipher ?? 'not seen'}</div>
                        <div>Integrity: {encryption.nas_integrity ?? 'not seen'}</div>
                    {/if}
                </div>
                {#if missing.length > 0}
                    <p class="mt-2 text-xs font-medium text-red-700 dark:text-red-300">
                        Missing: {missing.join(', ')}.
                        {#if missing.some((m) => m.includes('encryption'))}
                            Traffic without encryption can be read by anything within range.
                        {/if}
                        {#if missing.some((m) => m.includes('integrity'))}
                            Signalling without integrity protection can be altered or forged in
                            transit.
                        {/if}
                    </p>
                {/if}
                <Explainer
                    summary="Which cipher is protecting what passes between your device and the network."
                >
                    <p>
                        The <strong>radio link</strong> cipher protects everything between your
                        device and the tower. The <strong>core network</strong> cipher protects signalling
                        with the operator behind it. EEA1, EEA2 and EEA3 are real encryption; EEA0 means
                        none at all.
                    </p>
                    <p>
                        Fake base stations commonly select no encryption, because they do not hold
                        the keys real encryption would need. Rayhunter raises a warning when that
                        happens, but seeing which cipher is in use lets you check rather than
                        assume.
                    </p>
                </Explainer>
            </div>
        {/if}

        <Explainer
            summary="You are connected to this tower. These are the readings your modem reports for it."
        >
            <p>
                <strong>RSRP</strong> is how strongly your device hears this tower's reference signal,
                in dBm. It is a negative number, and closer to zero is stronger. Around minus 80 is excellent,
                minus 90 is good, minus 100 is workable, and below that the connection struggles.
            </p>
            <p>
                <strong>RSRQ</strong> is quality rather than strength, in dB. A strong signal in a crowded
                place can still be poor quality. Roughly minus 10 or better is healthy.
            </p>
            <p>
                <strong>Band</strong> and <strong>EARFCN</strong> say which slice of radio spectrum this
                tower uses. The EARFCN is the exact channel number; the band is the range it falls in.
                A tower appearing on an unexpected band for your operator is worth noticing.
            </p>
        </Explainer>

        {#if !identity}
            <p class="mt-3 text-xs text-gray-500 dark:text-gray-400">
                This tower's identifying broadcast has not been captured yet, so its operator and
                cell identity are still unknown. It usually arrives within a few seconds.
            </p>
        {/if}

        <details class="group mt-3 border-t border-gray-200 dark:border-gray-700 pt-3">
            <summary
                class="cursor-pointer list-none text-sm text-rayhunter-blue underline marker:hidden"
            >
                Advanced radio details
            </summary>
            <div class="mt-2 overflow-x-auto">
                <table class="w-full text-sm">
                    <tbody>
                        {#each advanced as row (row.label)}
                            <tr class="border-t border-gray-100 dark:border-gray-800">
                                <td class="py-1 pr-4 text-gray-500 dark:text-gray-400"
                                    >{row.label}</td
                                >
                                <td class="py-1 text-right font-mono">{row.value}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>

            <Explainer summary="What the less obvious numbers here mean.">
                <p>
                    The <strong>cell identity</strong> is unique to this cell across the operator's
                    whole network, and splits into a <strong>base station</strong> and a
                    <strong>sector</strong>. A large site usually has three sectors pointing in
                    different directions, so a sector change without a base station change means you
                    moved around one tower rather than to a new one.
                </p>
                <p>
                    The <strong>tracking area code</strong> groups towers for the purpose of finding your
                    phone when someone calls it. Crossing between areas makes your phone announce itself,
                    which is one of the moments a surveillance device can wait for.
                </p>
                <p>
                    <strong>EARFCN</strong> is the channel number and the frequency is derived from it.
                    Uplink and downlink are separate frequencies on most bands, and a single shared one
                    on time division bands. A tower on a band your operator does not use in your area
                    is worth a second look.
                </p>
                <p>
                    <strong>Timing advance</strong> is how far ahead the tower tells your phone to transmit
                    so its signal arrives on time, which is a direct read on distance. Each step is roughly
                    78 metres of signal path. It measures the path the signal actually takes, so it reads
                    high where the signal bounces off buildings, and it only updates when your phone performs
                    random access rather than continuously.
                </p>
                <p>
                    <strong>ASU</strong> and <strong>bars</strong> are the same measurement as RSRP presented
                    the way phones show it. The dBm figure is the real reading.
                </p>
            </Explainer>
        </details>

        <!-- Neighbours: signals we can measure but not identify. -->
        <div class="mt-4 border-t border-gray-200 dark:border-gray-700 pt-3">
            <h3 class="text-sm font-medium text-gray-700 dark:text-gray-200">
                Neighbouring cells{info.neighbors.length ? ` (${info.neighbors.length})` : ''}
            </h3>

            {#if info.neighbors.length === 0}
                <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">
                    None reported right now. Your modem only measures neighbours when it is
                    considering moving, so an empty list is normal on a strong stable connection.
                </p>
            {:else}
                <div class="mt-2 overflow-x-auto">
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="text-left text-xs text-gray-500 dark:text-gray-400">
                                <th class="pr-4 pb-1 font-normal">Signal ref.</th>
                                <th class="pr-4 pb-1 font-normal">Channel</th>
                                <th class="pr-4 pb-1 font-normal">RSRP</th>
                                <th class="pr-4 pb-1 font-normal">RSRQ</th>
                                <th class="pb-1 font-normal">Margin</th>
                            </tr>
                        </thead>
                        <tbody>
                            {#each info.neighbors as n (`${n.earfcn}-${n.pci}`)}
                                <tr class="border-t border-gray-100 dark:border-gray-800">
                                    <td class="py-1 pr-4 font-mono">{n.pci}</td>
                                    <td class="py-1 pr-4 font-mono">{n.earfcn}</td>
                                    <td
                                        class="py-1 pr-4 {QUALITY_CLASS[
                                            rsrp_quality(n.signal.rsrp_dbm)
                                        ]}">{dbm(n.signal.rsrp_dbm)}</td
                                    >
                                    <td class="py-1 pr-4">{db(n.signal.rsrq_db)}</td>
                                    <td class="py-1">{n.s_rxlev ?? '—'}</td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                </div>
            {/if}

            <Explainer
                summary="Other towers your modem can hear, strongest first. These cannot be identified, only measured."
            >
                <p>
                    The <strong>signal reference</strong> is a physical cell identity. It is not a tower
                    identity and cannot be looked up. There are only about 500 of these values and networks
                    reuse them constantly, so two unrelated towers can share one. All it does is let your
                    modem tell apart the signals it hears on one channel.
                </p>
                <p>
                    A tower only reveals who it really is once your device connects to it, which is
                    why the serving cell above has a full identity and these do not. Treating a
                    neighbour reference as a tower name would be inventing information.
                </p>
                <p>
                    Still useful to watch: a neighbour that appears suddenly with an unusually
                    strong signal, especially on a band your operator does not normally use, is the
                    sort of thing worth a second look.
                </p>
            </Explainer>
        </div>

        <!-- History: everything attached to during this run. -->
        {#if info.history.length > 1}
            <details class="group mt-4 border-t border-gray-200 dark:border-gray-700 pt-3">
                <summary
                    class="cursor-pointer list-none text-sm text-rayhunter-blue underline marker:hidden"
                >
                    Cells seen this run ({info.history.length})
                </summary>
                <div class="mt-2 overflow-x-auto">
                    <table class="w-full text-sm">
                        <thead>
                            <tr class="text-left text-xs text-gray-500 dark:text-gray-400">
                                <th class="pr-4 pb-1 font-normal">Cell identity</th>
                                <th class="pr-4 pb-1 font-normal">Signal ref.</th>
                                <th class="pr-4 pb-1 font-normal">Channel</th>
                                <th class="pr-4 pb-1 font-normal">Best RSRP</th>
                                <th class="pr-4 pb-1 font-normal">First seen</th>
                                <th class="pb-1 font-normal">Last seen</th>
                            </tr>
                        </thead>
                        <tbody>
                            {#each info.history as h (`${h.earfcn}-${h.pci}-${h.first_seen}`)}
                                <tr class="border-t border-gray-100 dark:border-gray-800">
                                    <td class="py-1 pr-4 font-mono">
                                        {h.identity?.cell_id ?? '—'}
                                    </td>
                                    <td class="py-1 pr-4 font-mono">{h.pci}</td>
                                    <td class="py-1 pr-4 font-mono">{h.earfcn}</td>
                                    <td class="py-1 pr-4">{dbm(h.best_rsrp_dbm)}</td>
                                    <td class="py-1 pr-4">{clock(h.first_seen)}</td>
                                    <td class="py-1">{clock(h.last_seen)}</td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                </div>
                {#if tacChanges.length > 0}
                    <div class="mt-3 border-t border-gray-200 dark:border-gray-700 pt-2">
                        <div class="text-xs font-medium text-gray-700 dark:text-gray-200">
                            Tracking area changes ({tacChanges.length})
                        </div>
                        <ul class="mt-1 space-y-0.5">
                            {#each tacChanges as change (change.at)}
                                <li class="text-xs text-gray-500 dark:text-gray-400">
                                    {clock(change.at)} &mdash; area
                                    <span class="font-mono">{change.from}</span> to
                                    <span class="font-mono">{change.to}</span>
                                </li>
                            {/each}
                        </ul>
                        <Explainer summary="Why a change of tracking area is worth noticing.">
                            <p>
                                A tracking area is the group of towers the network uses to find your
                                phone when someone calls. Crossing between them makes your device
                                announce itself, which is one of the few moments equipment listening
                                nearby can rely on hearing from a phone that would otherwise stay
                                quiet.
                            </p>
                            <p>
                                Changes are ordinary when you are travelling. Repeated changes while
                                you have not moved are not, and are worth looking at alongside the
                                warnings.
                            </p>
                        </Explainer>
                    </div>
                {/if}

                <p class="mt-2 text-xs text-gray-500 dark:text-gray-400">
                    Every cell this device has attached to since Rayhunter started. Moving through a
                    city produces a long list; sitting still and seeing many is more interesting.
                </p>
            </details>
        {/if}

        {#if info.health.messages_seen > 0}
            <div class="mt-3 border-t border-gray-200 dark:border-gray-700 pt-2">
                <p class="text-xs text-gray-500 dark:text-gray-400">
                    Understood {(100 - skipped_percent(info.health)).toFixed(1)}% of
                    {info.health.messages_seen.toLocaleString()} messages this run.
                    {#if verdict === 'stalled'}
                        Nothing has arrived recently, so nothing is currently being examined.
                    {:else if verdict === 'blind' || verdict === 'degraded'}
                        A quiet screen is less reassuring than usual while this figure is low.
                    {/if}
                </p>
                <Explainer summary="Why this number matters more than it looks.">
                    <p>
                        Rayhunter can only warn about traffic it can decode. If it is understanding
                        very little, or nothing is arriving at all, an absence of warnings means it
                        cannot tell rather than that nothing is wrong. Those two look identical
                        otherwise, which is why the figure is here.
                    </p>
                    <p>
                        Some messages are always skipped. Encrypted signalling and message types
                        Rayhunter does not parse are both normal, so a small share is expected and
                        not a fault.
                    </p>
                </Explainer>
            </div>
        {/if}
    {/if}
</div>

<style>
    summary::-webkit-details-marker {
        display: none;
    }
</style>
