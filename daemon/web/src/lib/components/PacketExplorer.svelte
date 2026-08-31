<script lang="ts">
    import Modal from './Modal.svelte';
    import PacketDetail from './PacketDetail.svelte';
    import Explainer from './Explainer.svelte';
    import {
        list_packets,
        apply_filters,
        direction_arrow,
        cells_present,
        DEFAULT_FILTERS,
        type PacketList,
        type PacketFilters,
    } from '../packets.svelte';

    let {
        shown = $bindable(),
        recording,
        /** Packet to open on, when arriving from a warning. */
        focusPacket = null,
    }: {
        shown: boolean;
        recording: string;
        focusPacket?: number | null;
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

    // Opening from a warning lands on that packet with its neighbours around
    // it, because several detectors are stateful and the sequence is usually
    // more telling than the one message that tripped them.
    $effect(() => {
        if (!shown) return;
        if (focusPacket !== null) {
            contextMode = true;
            selected = focusPacket;
            // Unfiltered, so the packet asked for is never filtered out from
            // under the person who clicked through to it.
            filters = { ...DEFAULT_FILTERS, decodedOnly: false };
            load({ around: focusPacket });
        } else if (!list) {
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
