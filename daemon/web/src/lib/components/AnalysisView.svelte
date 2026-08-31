<script lang="ts">
    import { type ReportMetadata } from '$lib/analysis.svelte';
    import type { ManifestEntry } from '$lib/manifest.svelte';
    import { gps_mode_label } from '$lib/utils.svelte';
    import { AnalysisManager } from '$lib/analysisManager.svelte';
    import { AnalysisRowType } from '$lib/analysis.svelte';
    import AnalysisTable from './AnalysisTable.svelte';
    import ReAnalyzeButton from './ReAnalyzeButton.svelte';
    import PacketExplorer from './PacketExplorer.svelte';
    let {
        entry,
        manager,
        current,
    }: {
        entry: ManifestEntry;
        manager: AnalysisManager;
        current: boolean;
    } = $props();

    // The explorer lives here because this is where warnings are shown, so
    // "view packet" and the browse button can share one instance.
    let explorerShown = $state(false);
    let focusPacket = $state<number | null>(null);
    // Counts requests rather than packets, so clicking the same warning twice
    // is two requests and the explorer honours both.
    let focusNonce = $state(0);

    /**
     * Packets that produced a warning, and the worst severity each reached.
     *
     * Without this, browsing gives no clue where the interesting messages are,
     * and finding the one that fired an alert means going back to the warnings
     * list and clicking through.
     */
    let alertPackets = $derived.by(() => {
        const map = new Map<number, string>();
        const report = entry.analysis_report;
        // The field also carries an error string when analysis failed, in
        // which case there are no rows to read.
        if (!report || typeof report === 'string') return map;
        const rank = ['Informational', 'Low', 'Medium', 'High'];
        for (const row of report.rows) {
            if (row.type !== AnalysisRowType.Analysis) continue;
            if (row.packet_num === undefined) continue;
            for (const event of row.events) {
                if (!event) continue;
                const existing = map.get(row.packet_num);
                if (!existing || rank.indexOf(event.event_type) > rank.indexOf(existing)) {
                    map.set(row.packet_num, event.event_type);
                }
            }
        }
        return map;
    });

    function browse() {
        focusPacket = null;
        focusNonce += 1;
        explorerShown = true;
    }

    function view_packet(packetNum: number) {
        focusPacket = packetNum;
        focusNonce += 1;
        explorerShown = true;
    }

    const date_formatter = new Intl.DateTimeFormat(undefined, {
        timeStyle: 'long',
        dateStyle: 'short',
    });
</script>

<div class="container mt-2">
    {#if entry.analysis_report === undefined}
        <p>Report unavailable, try refreshing.</p>
    {:else if typeof entry.analysis_report === 'string'}
        <p>Error getting analysis report: {entry.analysis_report}</p>
    {:else}
        {@const metadata: ReportMetadata = entry.analysis_report.metadata}
        {@const numWarnings: number = entry.get_num_warnings() || 0}
        <div class="flex flex-col gap-2">
            {#if !!numWarnings || !current}
                <div class="flex flex-row justify-between items-center">
                    {#if !!numWarnings}
                        <div
                            class="text-red-700 dark:text-red-300 border-red-500 border rounded-lg text-blue-600 dark:text-blue-400 px-2 py-1 mr-12"
                        >
                            Your Rayhunter device raised {`${numWarnings}`} warning{`${
                                numWarnings > 1 ? 's' : ''
                            }`}!
                            <a
                                href="https://efforg.github.io/rayhunter/faq.html#red"
                                class="text-blue-600 dark:text-blue-400 underline">Read the FAQ</a
                            > to learn what you can do about it
                        </div>
                    {/if}
                    {#if !current}
                        <ReAnalyzeButton {entry} {manager} />
                    {/if}
                    <button
                        type="button"
                        onclick={browse}
                        class="bg-blue-500 hover:bg-blue-700 text-white text-sm py-1 px-3 rounded-sm"
                    >
                        Packets
                    </button>
                </div>
            {/if}
            {#if entry.analysis_report.rows.length > 0}
                <AnalysisTable report={entry.analysis_report} onViewPacket={view_packet} />
            {:else}
                <p>No warnings to display!</p>
            {/if}
            <div>
                <p class="text-lg underline">Metadata</p>
                {#if metadata !== undefined && metadata.rayhunter !== undefined}
                    <p><b>Rayhunter version:</b> {metadata.rayhunter.rayhunter_version}</p>
                    <p><b>Device system OS:</b> {metadata.rayhunter.system_os}</p>
                {:else}
                    <p>N/A (analysis generated by an older version of rayhunter)</p>
                {/if}
                {#if entry.upload_time}
                    <p>
                        <b>WebDAV uploaded at:</b>
                        <span class="text-green-700 dark:text-green-300"
                            >{date_formatter.format(entry.upload_time)}</span
                        >
                    </p>
                {/if}
                <p>
                    <b>GPS Mode:</b>
                    {gps_mode_label(entry.gps_mode)}
                </p>
            </div>
            {#if metadata && metadata.analyzers}
                <div>
                    <p class="text-lg underline">Enabled Analyzers</p>
                    {#each metadata.analyzers as analyzer}
                        <p><b>{analyzer.name}:</b> {analyzer.description}</p>
                    {/each}
                </div>
            {/if}
        </div>
    {/if}
</div>

<!-- Not wrapped in {#if}: Modal handles its own visibility, and destroying it
     while it still thinks it is open skips the cleanup that unlocks page
     scrolling. -->
<PacketExplorer
    bind:shown={explorerShown}
    recording={entry.name}
    {focusPacket}
    {focusNonce}
    {alertPackets}
/>
