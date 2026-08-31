<script lang="ts">
    import type { Snippet } from 'svelte';
    import { onMount } from 'svelte';

    let {
        shown = $bindable(),
        title,
        children,
    }: { shown: boolean; title: string; children: Snippet } = $props();

    onMount(() => {
        const handler = () => {
            document.documentElement.style.setProperty('--scroll-y', `${window.scrollY}px`);
        };
        window.addEventListener('scroll', handler);

        // Escape is the expected way out of a dialog, and it is also a second
        // route that does not depend on the close button being visible. A
        // styling mistake on that button should never leave someone stuck.
        const onkeydown = (event: KeyboardEvent) => {
            if (event.key === 'Escape' && shown) {
                shown = false;
            }
        };
        window.addEventListener('keydown', onkeydown);

        return () => {
            window.removeEventListener('scroll', handler);
            window.removeEventListener('keydown', onkeydown);
        };
    });

    // Holding the page still while a dialog is open means pinning the body,
    // which has to be undone on the way out. Releasing it from a teardown
    // rather than an else branch covers being unmounted while still open: that
    // path skipped the else entirely and left the page unable to scroll, with
    // nothing on screen to explain why.
    $effect(() => {
        if (!shown) return;

        const body = document.body;
        const scrollY = document.documentElement.style.getPropertyValue('--scroll-y');
        body.style.position = 'fixed';
        body.style.top = `-${scrollY}`;

        return () => {
            const offset = body.style.top;
            body.style.position = '';
            body.style.top = '';
            window.scrollTo(0, parseInt(offset || '0') * -1);
        };
    });
</script>

{#if shown}
    <div
        class="fixed left-5 right-5 top-5 bottom-5 z-50 bg-white dark:bg-gray-900 border border-gray-200 dark:border-gray-700 rounded-md
		flex flex-col p-2 drop-shadow-sm"
    >
        <div class="flex justify-between items-center p-1">
            <span class="text-2xl">{title}</span>
            <button
                onclick={() => (shown = false)}
                aria-label="Close"
                title="Close"
                class="rounded-sm p-1 text-gray-700 hover:bg-gray-100 hover:text-gray-900 dark:text-gray-200 dark:hover:bg-gray-800 dark:hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2"
            >
                <svg
                    xmlns="http://www.w3.org/2000/svg"
                    aria-hidden="true"
                    width="24"
                    height="24"
                    fill="currentColor"
                    viewBox="0 0 24 24"
                >
                    <!-- No fill on the path: it must inherit currentColor, or the
                         icon keeps a fixed dark value and disappears against a
                         dark background, leaving no visible way out. -->
                    <path
                        fill-rule="evenodd"
                        clip-rule="evenodd"
                        d="M5.29289 5.29289C5.68342 4.90237 6.31658 4.90237 6.70711 5.29289L12 10.5858L17.2929 5.29289C17.6834 4.90237 18.3166 4.90237 18.7071 5.29289C19.0976 5.68342 19.0976 6.31658 18.7071 6.70711L13.4142 12L18.7071 17.2929C19.0976 17.6834 19.0976 18.3166 18.7071 18.7071C18.3166 19.0976 17.6834 19.0976 17.2929 18.7071L12 13.4142L6.70711 18.7071C6.31658 19.0976 5.68342 19.0976 5.29289 18.7071C4.90237 18.3166 4.90237 17.6834 5.29289 17.2929L10.5858 12L5.29289 6.70711C4.90237 6.31658 4.90237 5.68342 5.29289 5.29289Z"
                    />
                </svg>
            </button>
        </div>
        <div class="overflow-y-auto flex-1">
            {@render children()}
        </div>
    </div>
{/if}
