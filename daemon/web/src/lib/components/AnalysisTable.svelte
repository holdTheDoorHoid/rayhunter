<script lang="ts">
    import { AnalysisRowType, type AnalysisReport } from '$lib/analysis.svelte';
    let {
        report,
        /** Called with a packet number when somebody wants to see the message. */
        onViewPacket = null,
    }: {
        report: AnalysisReport;
        onViewPacket?: ((packetNum: number) => void) | null;
    } = $props();

    const date_formatter = new Intl.DateTimeFormat(undefined, {
        timeStyle: 'long',
        dateStyle: 'short',
    });

    const analyzers = $derived(report.metadata.analyzers);

    const skipped_messages: Map<string, number> = $derived.by(() => {
        let map = new Map();
        for (const row of report.rows) {
            if (row.type === AnalysisRowType.Skipped) {
                let count = map.get(row.reason);
                if (count === undefined) {
                    count = 0;
                }
                map.set(row.reason, count + 1);
            }
        }
        return map;
    });
</script>

<div>
    <p class="text-lg underline">Warnings and Informational Logs</p>
    {#if report.statistics.num_warnings === 0 && report.statistics.num_informational_logs === 0}
        <p>Nothing to show!</p>
    {:else}
        <div class="overflow-x-auto">
            <table class="table-auto text-left">
                <thead class="p-2">
                    <tr class="bg-gray-300 dark:bg-gray-700">
                        <th class="p-2">Timestamp</th>
                        <th class="p-2">Heuristic</th>
                        <th class="p-2">Warning</th>
                        <th class="p-2">Severity</th>
                        {#if onViewPacket}<th class="p-2">Packet</th>{/if}
                    </tr>
                </thead>
                <tbody>
                    {#each report.rows as row}
                        {#if row.type === AnalysisRowType.Analysis}
                            {@const parsed_date = new Date(row.packet_timestamp)}
                            {#each row.events as event, analyzerIndex}
                                {#if event !== null}
                                    {@const analyzer = analyzers[analyzerIndex]}
                                    {@const event_type_class = {
                                        Informational: '',
                                        // Each severity carries its own text
                                        // colour rather than inheriting: these
                                        // backgrounds are fixed in both themes,
                                        // so inherited light text vanished on
                                        // them in dark mode.
                                        Low: 'bg-yellow-200 text-yellow-950',
                                        Medium: 'bg-orange-400 text-orange-950',
                                        High: 'bg-red-600 text-white',
                                    }[event.event_type]}
                                    <tr
                                        class="even:bg-gray-200 odd:bg-white dark:even:bg-gray-800 dark:odd:bg-gray-900"
                                    >
                                        <td class="p-2">{date_formatter.format(parsed_date)}</td>
                                        <td class="p-2">{analyzer.name} v{analyzer.version}</td>
                                        <td class="p-2">{event.message}</td>
                                        <td class="p-2 {event_type_class} text-center"
                                            >{event.event_type}</td
                                        >
                                        {#if onViewPacket}
                                            <td class="p-2">
                                                {#if row.packet_num !== undefined}
                                                    <button
                                                        type="button"
                                                        onclick={() =>
                                                            onViewPacket(row.packet_num!)}
                                                        class="text-xs text-rayhunter-blue underline"
                                                        >View packet {row.packet_num}</button
                                                    >
                                                {:else}
                                                    <span
                                                        class="text-xs text-gray-500 dark:text-gray-400"
                                                        title="This recording was analysed before Rayhunter recorded packet numbers. Re-run the analysis to enable this."
                                                        >not recorded</span
                                                    >
                                                {/if}
                                            </td>
                                        {/if}
                                    </tr>
                                {/if}
                            {/each}
                        {/if}
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</div>
{#if report.statistics.num_skipped_packets > 0}
    <div>
        <p class="text-lg underline">Unparsed Messages</p>
        <p>
            These are due to a limitation or bug in Rayhunter's parser, and aren't usually a
            problem. We'll not accept bug reports about them unless something else is going wrong
            (such as false-positives or definite false-negatives)
        </p>
        <div class="overflow-x-auto">
            <table class="table-auto text-left">
                <thead class="p-2">
                    <tr class="bg-gray-300 dark:bg-gray-700">
                        <th scope="col" class="p-2">Total Msgs Affected</th>
                        <th scope="col">Reason/Error</th>
                    </tr>
                </thead>
                <tbody>
                    {#each skipped_messages.entries() as [message, count]}
                        <tr
                            class="even:bg-gray-200 odd:bg-white dark:even:bg-gray-800 dark:odd:bg-gray-900"
                        >
                            <td class="text-center">{count}</td>
                            <td>{message}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    </div>
{/if}
