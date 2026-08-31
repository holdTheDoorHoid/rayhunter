<script lang="ts">
    import { AnalysisStatus } from '$lib/analysisManager.svelte';
    import type { EventType } from '$lib/analysis.svelte';
    import type { ManifestEntry } from '$lib/manifest.svelte';
    let {
        entry,
        onclick,
        analysis_visible,
    }: {
        entry: ManifestEntry;
        onclick: () => void;
        analysis_visible: boolean;
    } = $props();

    let summary = $derived.by(() => {
        if (entry.analysis_status === AnalysisStatus.Queued) {
            return 'Queued...';
        } else if (entry.analysis_status === AnalysisStatus.Running) {
            return 'Running...';
        } else if (entry.analysis_status === AnalysisStatus.Finished) {
            if (entry.analysis_report === undefined) {
                return 'Loading...';
            } else if (typeof entry.analysis_report === 'string') {
                return entry.analysis_report;
            } else {
                return `${entry.analysis_report.statistics.num_warnings} warnings`;
            }
        } else {
            return 'Loading...';
        }
    });

    /**
     * The counts to show, worst first, leaving out levels that scored nothing.
     *
     * One total says whether anything was found but not how bad it was, and
     * those are separate questions. Six low severity notes and one high
     * severity detection are the same number next to the word warnings while
     * meaning quite different things. Requested upstream as
     * EFForg/rayhunter#363, where a maintainer asked for exactly this: a count
     * per level rather than one figure standing for all of them.
     */
    const LEVELS: { key: EventType; label: string; class: string }[] = [
        {
            key: 'High',
            label: 'High',
            class: 'text-red-800 dark:text-red-200 border-red-500 dark:border-red-700 bg-red-200 dark:bg-red-900',
        },
        {
            key: 'Medium',
            label: 'Medium',
            class: 'text-orange-900 dark:text-orange-100 border-orange-500 dark:border-orange-700 bg-orange-200 dark:bg-orange-900',
        },
        {
            key: 'Low',
            label: 'Low',
            class: 'text-yellow-900 dark:text-yellow-100 border-yellow-500 dark:border-yellow-700 bg-yellow-200 dark:bg-yellow-900',
        },
        {
            key: 'Informational',
            label: 'Info',
            class: 'text-gray-700 dark:text-gray-200 border-gray-400 dark:border-gray-600 bg-gray-200 dark:bg-gray-700',
        },
    ];

    let severity_counts = $derived.by(() => {
        const report = entry.analysis_report;
        if (!report || typeof report === 'string') return [];
        // Older reports parsed before this existed have no breakdown.
        const counts = report.statistics.by_severity;
        if (!counts) return [];
        return LEVELS.map((level) => ({ ...level, count: counts[level.key] ?? 0 })).filter(
            (level) => level.count > 0
        );
    });

    let ready = $derived.by(() => {
        let finished = entry.analysis_status === AnalysisStatus.Finished;
        let report_available = entry.analysis_report !== undefined;
        return finished && report_available;
    });

    let button_class = $derived.by(() => {
        if (!ready) {
            return 'text-gray-700 dark:text-gray-200';
        } else if ((entry.get_num_warnings() || 0) < 1) {
            return 'text-green-800 dark:text-green-200 border-green-500 dark:border-green-700 bg-green-200 dark:bg-green-900 border rounded-full px-2';
        } else {
            return 'text-red-800 dark:text-red-200 border-red-500 dark:border-red-700 bg-red-200 dark:bg-red-900 border rounded-full px-2';
        }
    });
</script>

<button class="flex flex-row gap-1 lg:gap-2" disabled={!ready} {onclick}>
    <span class="flex flex-row items-center gap-1">
        {#if entry.analysis_status === AnalysisStatus.Queued || entry.analysis_status === AnalysisStatus.Running || (entry.analysis_status === AnalysisStatus.Finished && entry.analysis_report === undefined)}
            <svg
                class="animate-spin h-4 w-4 text-blue-600 dark:text-blue-400"
                xmlns="http://www.w3.org/2000/svg"
                fill="none"
                viewBox="0 0 24 24"
            >
                <circle
                    class="opacity-25"
                    cx="12"
                    cy="12"
                    r="10"
                    stroke="currentColor"
                    stroke-width="4"
                ></circle>
                <path
                    class="opacity-75"
                    fill="currentColor"
                    d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                ></path>
            </svg>
        {/if}
        {#if severity_counts.length > 0}
            <!-- Each pill names its level as well as colouring it. Severity
                 read from colour alone is unreadable to anyone who cannot
                 separate red from orange, and this is the one line on the page
                 that says how bad things are. -->
            <span class="flex flex-row flex-wrap items-center gap-1">
                {#each severity_counts as level (level.key)}
                    <span class="{level.class} border rounded-full px-2 whitespace-nowrap">
                        {level.count}
                        {level.label}
                    </span>
                {/each}
            </span>
        {:else}
            <span class={button_class}>{summary}</span>
        {/if}
    </span>
    <svg
        class="w-6 h-6 text-gray-800 dark:text-gray-100 transition-transform {analysis_visible
            ? 'rotate-180'
            : ''}"
        aria-hidden="true"
        xmlns="http://www.w3.org/2000/svg"
        width="24"
        height="24"
        fill="none"
        viewBox="0 0 24 24"
    >
        <path
            stroke="currentColor"
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="m19 9-7 7-7-7"
        />
    </svg>
</button>
