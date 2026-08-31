<script lang="ts">
    import Modal from './Modal.svelte';
    import PacketDetail from './PacketDetail.svelte';
    import Explainer from './Explainer.svelte';
    import {
        list_packets,
        apply_filters,
        direction_arrow,
        cells_present,
        open_action,
        DEFAULT_FILTERS,
        type PacketList,
        type PacketFilters,
    } from '../packets.svelte';

    let {
        shown = $bindable(),
        recording,
        /** Packet to open on, when arriving from a warning. */
        focusPacket = null,
        /**
         * Bumped by the parent once per request. Identifies the request rather
         * than the packet, so asking for the same packet a second time still
         * brings it back into view after paging away from it.
         */
        focusNonce = 0,
        /** Packets that produced a warning, mapped to their worst severity. */
        alertPackets = new Map<number, string>(),
    }: {
        shown: boolean;
        recording: string;
        focusPacket?: number | null;
        focusNonce?: number;
        alertPackets?: Map<number, string>;
    } = $props();

    const PAGE = 100;
    const CONTEXT = 10;

    let list = $state<PacketList | null>(null);
    let selected = $state<number | null>(null);
    let error = $state('');
    let loading = $state(false);
    let filters = $state<PacketFilters>({ ...DEFAULT_FILTERS });
    let offset = $state(1);
    /** True while showing a window centred on a warning rather than a page. */
    let contextMode = $state(false);

    let visible = $derived(list ? apply_filters(list.packets, filters) : []);
    let cells = $derived(list ? cells_present(list.packets) : []);
    let jumpTo = $state('');

    const SEVERITY_CLASS: Record<string, string> = {
        High: 'bg-red-600 text-white',
        Medium: 'bg-orange-400 text-orange-950',
        Low: 'bg-yellow-200 text-yellow-950',
        Informational: 'bg-gray-200 text-gray-800 dark:bg-gray-700 dark:text-gray-100',
    };

    /** Jump straight to a packet number, which beats paging to reach it. */
    function jump() {
        const wanted = parseInt(jumpTo, 10);
        if (!Number.isFinite(wanted) || wanted < 1) return;
        contextMode = false;
        selected = wanted;
        // Centre it rather than starting the page there, so its neighbours are
        // visible, which is usually why somebody went looking for it.
        offset = Math.max(1, wanted - Math.floor(PAGE / 2));
        load({ offset });
    }
    let hiddenCount = $derived(list ? list.packets.length - visible.length : 0);

    async function load(params: { offset?: number; around?: number }) {
        loading = true;
        error = '';
        try {
            list = await list_packets(recording, {
                ...(params.around !== undefined
                    ? { around: params.around, context: CONTEXT }
                    : { offset: params.offset ?? 1, limit: PAGE }),
            });
        } catch (e) {
            error = `${e}`;
            list = null;
        } finally {
            loading = false;
        }
    }

    // Which focus request has already been acted on. Deliberately a plain
    // variable rather than $state: the effect below both reads and writes it,
    // and a reactive one would feed straight back into that effect.
    let handledNonce: number | null = null;

    // Opening from a warning lands on that packet with its neighbours around
    // it, because several detectors are stateful and the sequence is usually
    // more telling than the one message that tripped them.
    //
    // Only ever acts on a *change* of focus. Re-running the body unconditionally
    // meant every run assigned a fresh `filters` object, which invalidated the
    // derived views, which ran the effect again: an endless loop that refetched
    // the same window forever and pinned a device that has one core and a
    // recording to keep up with. It also stole the list back from anyone who
    // paged or jumped away, roughly twice a second.
    $effect(() => {
        if (!shown) {
            // Reopening on the same warning should focus it again.
            handledNonce = null;
            return;
        }
        const action = open_action({ packet: focusPacket, nonce: focusNonce }, handledNonce, {
            shown,
            contextMode,
            hasList: list !== null,
        });
        if (action.kind === 'nothing') return;
        handledNonce = focusNonce;

        if (action.kind === 'focus') {
            contextMode = true;
            selected = action.packet;
            // Unfiltered, so the packet asked for is never filtered out from
            // under the person who clicked through to it.
            filters = { ...DEFAULT_FILTERS, decodedOnly: false };
            load({ around: action.packet });
        } else {
            // Arriving by the browse button. Leaving the previous focused
            // window on screen, banner and all, would misdescribe what is
            // being shown.
            contextMode = false;
            selected = null;
            offset = 1;
            load({ offset: 1 });
        }
    });

    function page(delta: number) {
        contextMode = false;
        offset = Math.max(1, offset + delta * PAGE);
        selected = null;
        load({ offset });
    }

    function show_whole_page() {
        contextMode = false;
        offset = Math.max(1, (selected ?? 1) - Math.floor(PAGE / 2));
        load({ offset });
    }

    function row_class(packetNum: number, isFocus: boolean): string {
        if (packetNum === selected) return 'bg-rayhunter-blue/20';
        if (isFocus) return 'bg-red-100 dark:bg-red-950';
        return '';
    }
</script>

<Modal bind:shown title="Packet Explorer">
    <div class="p-2">
        <p class="text-xs text-gray-500 dark:text-gray-400">
            Messages from recording {recording}, decoded by the same path Rayhunter's detectors use.
            Numbers match the ones quoted in warnings.
        </p>

        <div class="mt-2 flex flex-wrap items-center gap-2">
            <input
                type="search"
                bind:value={filters.search}
                placeholder="Search name or channel"
                class="rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
            />
            <form
                class="flex items-center gap-1"
                onsubmit={(e) => {
                    e.preventDefault();
                    jump();
                }}
            >
                <input
                    type="number"
                    min="1"
                    bind:value={jumpTo}
                    placeholder="Go to #"
                    class="w-24 rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
                />
                <button
                    type="submit"
                    disabled={!jumpTo || loading}
                    class="rounded-md border border-gray-300 px-2 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
                    >Go</button
                >
            </form>
            <select
                bind:value={filters.protocol}
                class="rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
            >
                <option value="all">All protocols</option>
                <option value="rrc">LTE RRC</option>
                <option value="nas">LTE NAS</option>
            </select>
            <select
                bind:value={filters.direction}
                class="rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
            >
                <option value="all">Both directions</option>
                <option value="downlink">Downlink</option>
                <option value="uplink">Uplink</option>
            </select>
            <label class="flex items-center gap-1 text-sm">
                <input
                    type="checkbox"
                    bind:checked={filters.decodedOnly}
                    class="h-4 w-4 rounded-sm border-gray-300 text-rayhunter-blue dark:border-gray-600"
                />
                Signalling only
            </label>
        </div>

        {#if cells.length > 0}
            <p class="mt-2 text-xs text-gray-500 dark:text-gray-400">
                Cells in this window:
                {#each cells as c, i (c.pci)}{i > 0 ? ', ' : ' '}<button
                        type="button"
                        onclick={() => (filters.search = `pci${c.pci}`)}
                        class="font-mono underline">{c.pci}</button
                    >&nbsp;({c.count}){/each}
            </p>
        {/if}

        {#if contextMode}
            <p class="mt-2 text-xs text-amber-600 dark:text-amber-400">
                Showing packet {focusPacket} with {CONTEXT} either side.
                <button type="button" onclick={show_whole_page} class="underline"
                    >Show the surrounding page instead</button
                >
            </p>
        {/if}

        {#if error}
            <p class="mt-2 text-sm text-red-600 dark:text-red-400">{error}</p>
        {:else if loading && !list}
            <p class="mt-2 text-sm text-gray-500 dark:text-gray-400">Reading the recording…</p>
        {:else if list}
            <div class="mt-2 grid gap-3 lg:grid-cols-2">
                <div
                    class="max-h-96 overflow-auto rounded-md border border-gray-200 dark:border-gray-700"
                >
                    <table class="w-full text-sm">
                        <thead class="sticky top-0 bg-gray-100 text-left text-xs dark:bg-gray-800">
                            <tr>
                                <th class="px-2 py-1 font-normal">#</th>
                                <th class="px-2 py-1 font-normal">Dir</th>
                                <th class="px-2 py-1 font-normal">Protocol</th>
                                <th class="px-2 py-1 font-normal">Cell</th>
                                <th class="px-2 py-1 font-normal">Message</th>
                                <th class="px-2 py-1 font-normal">Alert</th>
                            </tr>
                        </thead>
                        <tbody>
                            {#each visible as p (p.packet_num)}
                                <tr
                                    class="cursor-pointer border-t border-gray-100 hover:bg-gray-50 dark:border-gray-800 dark:hover:bg-gray-800 {row_class(
                                        p.packet_num,
                                        p.packet_num === focusPacket
                                    )}"
                                    onclick={() => (selected = p.packet_num)}
                                >
                                    <td class="px-2 py-1 font-mono">{p.packet_num}</td>
                                    <td class="px-2 py-1">{direction_arrow(p.direction)}</td>
                                    <td class="px-2 py-1 text-xs">
                                        {p.protocol.replace('LTE ', '')}
                                        {#if p.channel}<span
                                                class="text-gray-500 dark:text-gray-400"
                                                >{p.channel}</span
                                            >{/if}
                                    </td>
                                    <td class="px-2 py-1 font-mono text-xs">
                                        {p.pci ?? ''}
                                    </td>
                                    <td class="px-2 py-1">
                                        {p.message_type ??
                                            (p.parse_status === 'decoded' ? '—' : p.parse_status)}
                                    </td>
                                    <td class="px-2 py-1">
                                        {#if alertPackets.get(p.packet_num)}
                                            <span
                                                class="rounded-sm px-1.5 py-0.5 text-[10px] font-medium {SEVERITY_CLASS[
                                                    alertPackets.get(p.packet_num)!
                                                ]}">{alertPackets.get(p.packet_num)}</span
                                            >
                                        {/if}
                                    </td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                    {#if visible.length === 0}
                        <p class="p-3 text-sm text-gray-500 dark:text-gray-400">
                            Nothing here matches the filters.
                        </p>
                    {/if}
                </div>

                <div>
                    {#if selected !== null}
                        <PacketDetail {recording} packetNum={selected} />
                    {:else}
                        <p class="text-sm text-gray-500 dark:text-gray-400">
                            Select a packet to decode it.
                        </p>
                    {/if}
                </div>
            </div>

            <div class="mt-2 flex flex-wrap items-center justify-between gap-2">
                <span class="text-xs text-gray-500 dark:text-gray-400">
                    Showing {visible.length} of {list.packets.length} fetched, from packet
                    {list.first_packet_num}.
                    {#if hiddenCount > 0}{hiddenCount} hidden by filters.{/if}
                </span>
                {#if !contextMode}
                    <div class="flex gap-2">
                        <button
                            type="button"
                            onclick={() => page(-1)}
                            disabled={offset <= 1 || loading}
                            class="rounded-md border border-gray-300 px-2 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
                        >
                            Previous
                        </button>
                        <button
                            type="button"
                            onclick={() => page(1)}
                            disabled={list.reached_end || loading}
                            class="rounded-md border border-gray-300 px-2 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
                        >
                            Next
                        </button>
                    </div>
                {/if}
            </div>

            <Explainer summary="What this is showing, and what it is not.">
                <p>
                    Every message in the recording gets a number, including ones that could not be
                    decoded, which is why the numbering matches what warnings quote. Most messages
                    carry no signalling at all; those are hidden unless you turn off
                    <strong>Signalling only</strong>.
                </p>
                <p>
                    Filters apply to the packets currently fetched rather than the whole recording,
                    because searching the entire file would mean decoding all of it on the device
                    for every keystroke. Page through to search further.
                </p>
                <p>
                    This is a quick look rather than a replacement for a protocol analyser. For
                    anything deeper, download the recording as a PCAP and open it in Wireshark.
                </p>
            </Explainer>
        {/if}
    </div>
</Modal>
