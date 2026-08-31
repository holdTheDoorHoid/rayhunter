<script lang="ts">
    import Explainer from './Explainer.svelte';
    import { get_packet, direction_arrow, type PacketDetail } from '../packets.svelte';

    let {
        recording,
        packetNum,
    }: {
        recording: string;
        packetNum: number;
    } = $props();

    let detail = $state<PacketDetail | null>(null);
    let error = $state('');
    let loading = $state(false);

    // Refetches whenever the selected packet changes, since the list keeps this
    // component mounted and only swaps the number.
    $effect(() => {
        const wanted = packetNum;
        loading = true;
        error = '';
        get_packet(recording, wanted)
            .then((d) => {
                // Ignore a response that arrived after the selection moved on.
                if (wanted === packetNum) detail = d;
            })
            .catch((e) => {
                if (wanted === packetNum) error = `${e}`;
            })
            .finally(() => {
                if (wanted === packetNum) loading = false;
            });
    });
</script>

<div class="rounded-md border border-gray-300 dark:border-gray-700 p-3">
    {#if loading && !detail}
        <p class="text-sm text-gray-500 dark:text-gray-400">Decoding packet {packetNum}…</p>
    {:else if error}
        <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
    {:else if detail}
        <div class="flex flex-wrap items-baseline gap-x-3">
            <span class="text-lg">Packet {detail.packet_num}</span>
            <span class="text-sm text-gray-500 dark:text-gray-400">
                {detail.protocol}
                {#if detail.channel}&middot; {detail.channel}{/if}
                {#if detail.direction}&middot; {direction_arrow(detail.direction)}
                    {detail.direction}{/if}
            </span>
        </div>

        {#if detail.message_type}
            <div class="mt-1 text-base font-medium">{detail.message_type}</div>
        {/if}

        {#if detail.decode_error}
            <p class="mt-2 text-xs text-amber-600 dark:text-amber-400">
                {detail.decode_error === 'not a signalling message'
                    ? 'This message carried no signalling, so there is nothing to decode. Most of a recording looks like this.'
                    : `Could not be decoded: ${detail.decode_error}`}
            </p>
        {/if}

        {#if detail.decoded}
            <details class="group mt-3" open>
                <summary
                    class="cursor-pointer list-none text-sm text-rayhunter-blue underline marker:hidden"
                >
                    Decoded fields
                </summary>
                <pre
                    class="mt-1 max-h-96 overflow-auto rounded-sm bg-gray-100 p-2 text-xs dark:bg-gray-800">{detail.decoded}</pre>
                <Explainer summary="Where this comes from.">
                    <p>
                        This is the decoder's own view of the message, shown in full rather than
                        reduced to fields somebody chose in advance. It is the same interpretation
                        Rayhunter's detectors worked from, so if a warning fired on this packet, the
                        reason for it is somewhere in here.
                    </p>
                </Explainer>
            </details>
        {/if}

        {#if detail.raw_hex}
            <details class="group mt-3">
                <summary
                    class="cursor-pointer list-none text-sm text-rayhunter-blue underline marker:hidden"
                >
                    Raw bytes ({detail.payload_len})
                </summary>
                <pre
                    class="mt-1 max-h-48 overflow-auto rounded-sm bg-gray-100 p-2 font-mono text-xs break-all whitespace-pre-wrap dark:bg-gray-800">{detail.raw_hex}</pre>
                {#if detail.raw_truncated}
                    <p class="text-xs text-gray-500 dark:text-gray-400">
                        Shown truncated. Download the recording for the whole payload.
                    </p>
                {/if}
            </details>
        {/if}
    {/if}
</div>

<style>
    summary::-webkit-details-marker {
        display: none;
    }
</style>
