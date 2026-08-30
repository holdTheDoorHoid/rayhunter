<script lang="ts">
    import Explainer from './Explainer.svelte';
    import {
        rsrp_quality,
        operator_name,
        format_plmn,
        split_cell_id,
        type CellInfo,
    } from '../cellInfo';

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
    let quality = $derived(serving ? rsrp_quality(serving.signal.rsrp_dbm) : null);
</script>

<div class="border border-gray-300 dark:border-gray-700 rounded-md p-4">
    <div class="flex items-baseline justify-between gap-2">
        <h2 class="text-xl">Cell Site</h2>
        {#if serving}
            <span class="text-xs text-gray-500 dark:text-gray-400">
                updated {clock(serving.last_seen)}
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
                <div class="text-lg">{serving.band ?? 'unknown'}</div>
                <div class="text-xs text-gray-500 dark:text-gray-400">
                    EARFCN {serving.earfcn}
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

        {#if identity}
            <div class="mt-3 border-t border-gray-200 dark:border-gray-700 pt-3">
                <div class="grid grid-cols-2 gap-x-4 gap-y-2 text-sm sm:grid-cols-4">
                    <div>
                        <div class="text-xs text-gray-500 dark:text-gray-400">Cell identity</div>
                        <div class="font-mono">{identity.cell_id}</div>
                    </div>
                    {#if split}
                        <div>
                            <div class="text-xs text-gray-500 dark:text-gray-400">Base station</div>
                            <div class="font-mono">{split.enodeb_id}</div>
                        </div>
                        <div>
                            <div class="text-xs text-gray-500 dark:text-gray-400">Sector</div>
                            <div class="font-mono">{split.sector_id}</div>
                        </div>
                    {/if}
                    <div>
                        <div class="text-xs text-gray-500 dark:text-gray-400">Tracking area</div>
                        <div class="font-mono">{identity.tac}</div>
                    </div>
                </div>

                <Explainer summary="How this tower identifies itself in its broadcast.">
                    <p>
                        The <strong>cell identity</strong> is unique to this cell across the whole
                        operator network. It splits into two halves that are more useful separately:
                        the <strong>base station</strong> is the physical site, and the
                        <strong>sector</strong> is which face of it you are on. A large site usually has
                        three sectors pointing in different directions. If the sector changes but the
                        base station does not, you moved around one tower rather than to a new one.
                    </p>
                    <p>
                        The <strong>tracking area</strong> is the group of towers the network uses to
                        find your phone when someone calls. Crossing between areas makes your phone announce
                        itself, which is one of the moments a surveillance device can be waiting for.
                    </p>
                </Explainer>
            </div>
        {:else}
            <p class="mt-3 text-xs text-gray-500 dark:text-gray-400">
                This tower's identifying broadcast has not been captured yet, so its operator and
                cell identity are still unknown. It usually arrives within a few seconds.
            </p>
        {/if}

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
                                <th class="pb-1 font-normal">RSRQ</th>
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
                                    <td class="py-1">{db(n.signal.rsrq_db)}</td>
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
                <p class="mt-2 text-xs text-gray-500 dark:text-gray-400">
                    Every cell this device has attached to since Rayhunter started. Moving through a
                    city produces a long list; sitting still and seeing many is more interesting.
                </p>
            </details>
        {/if}
    {/if}
</div>

<style>
    summary::-webkit-details-marker {
        display: none;
    }
</style>
