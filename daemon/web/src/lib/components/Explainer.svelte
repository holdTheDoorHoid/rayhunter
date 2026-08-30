<script lang="ts">
    import type { Snippet } from 'svelte';

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
        children,
    }: {
        /** One plain sentence, always visible. */
        summary: string;
        /** Text on the toggle. Keep it a promise of what opens. */
        label?: string;
        /** The fuller explanation. */
        children: Snippet;
    } = $props();
</script>

<p class="text-xs text-gray-500">{summary}</p>
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
    <div class="mt-1 space-y-2 border-l-2 border-gray-200 pl-3 text-xs text-gray-600">
        {@render children()}
    </div>
</details>

<style>
    /* Safari still shows a disclosure triangle without this. */
    summary::-webkit-details-marker {
        display: none;
    }
</style>
