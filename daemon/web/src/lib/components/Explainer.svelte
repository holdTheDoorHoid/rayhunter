<script lang="ts">
    import type { Snippet } from 'svelte';
    import { help } from '../helpVisibility.svelte';

    /**
     * Progressive disclosure for a single setting or reading.
     *
     * A short summary is always on screen, and a fuller explanation opens
     * inline underneath. Built on a native `details` element on purpose: it
     * opens by tap as readily as by click, is reachable and announced by
     * keyboard and screen readers, and needs no JavaScript. Hover tooltips
     * were considered and rejected, since hovering does not exist on the
     * phones many people will read this on.
     */
    let {
        summary,
        label = 'What this means',
        keepSummary = false,
        children,
    }: {
        /** One plain sentence, introducing what opens below it. */
        summary: string;
        /** Text on the toggle. Keep it a promise of what opens. */
        label?: string;
        /**
         * Keep the summary on screen even with explanations turned off.
         *
         * For the configuration page, where the summary is a setting's own
         * description rather than a signpost, and removing it would leave a
         * bare checkbox with no statement of what it does.
         */
        keepSummary?: boolean;
        /** The fuller explanation. */
        children: Snippet;
    } = $props();
</script>

<!-- The summary goes with the explanation it introduces. These sentences read
     as signposts to what opens below ("What this means", "Why this matters"),
     so with nothing left to open they are a promise of an explanation that is
     no longer there, which is worse than silence. Settings are the exception,
     since there the summary is the description of the setting itself. -->
{#if help.shown || keepSummary}
    <p class="text-xs text-gray-500 dark:text-gray-400">{summary}</p>
{/if}
{#if help.shown}
    <details class="group mt-1">
        <summary
            class="inline-flex cursor-pointer list-none items-center gap-1 text-xs text-rayhunter-blue underline marker:hidden focus-visible:outline-2 focus-visible:outline-offset-2"
        >
            <svg
                class="h-3 w-3 shrink-0 transition-transform group-open:rotate-90"
                viewBox="0 0 12 12"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                aria-hidden="true"
            >
                <path d="M4 2l4 4-4 4" stroke-linecap="round" stroke-linejoin="round" />
            </svg>
            {label}
        </summary>
        <div
            class="mt-1 space-y-2 border-l-2 border-gray-200 dark:border-gray-700 pl-3 text-xs text-gray-600 dark:text-gray-300"
        >
            {@render children()}
        </div>
    </details>
{/if}

<style>
    /* Safari still shows a disclosure triangle without this. */
    summary::-webkit-details-marker {
        display: none;
    }
</style>
