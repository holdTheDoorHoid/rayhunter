<script lang="ts">
    // Proof of holding the unit: the owner passphrase, then a code the unit
    // shows on its own screen (or a press of its button on a unit with no
    // screen). Used before anything that would be dangerous in the wrong
    // hands: the terminal, and turning ADB on.
    import { stepup_start, stepup_confirm, stepup_status } from '../utils.svelte';

    let {
        reason,
        onopened,
        oncancel,
    }: { reason: string; onopened: () => void; oncancel: () => void } = $props();

    let stage = $state<'passphrase' | 'code' | 'button'>('passphrase');
    let passphrase = $state('');
    let code = $state('');
    let error = $state('');
    let busy = $state(false);
    let poll: ReturnType<typeof setInterval> | null = null;

    async function start() {
        error = '';
        busy = true;
        try {
            const r = await stepup_start(passphrase);
            passphrase = '';
            if (r.has_screen) {
                stage = 'code';
            } else {
                stage = 'button';
                poll = setInterval(async () => {
                    try {
                        if ((await stepup_status()).active) done();
                    } catch {
                        /* keep waiting */
                    }
                }, 1000);
            }
        } catch (e) {
            error = `${e}`;
        } finally {
            busy = false;
        }
    }

    async function confirm() {
        error = '';
        busy = true;
        try {
            await stepup_confirm(code);
            code = '';
            done();
        } catch (e) {
            error = `${e}`;
            if (`${e}`.includes('start again')) stage = 'passphrase';
        } finally {
            busy = false;
        }
    }

    function done() {
        if (poll) clearInterval(poll);
        poll = null;
        onopened();
    }

    function cancel() {
        if (poll) clearInterval(poll);
        poll = null;
        oncancel();
    }
</script>

<div
    class="my-3 rounded-md border border-amber-400 bg-amber-50 p-3 text-sm dark:border-amber-600 dark:bg-amber-950"
>
    <p class="mb-2">{reason}</p>
    {#if stage === 'passphrase'}
        <form
            class="flex flex-wrap gap-2"
            onsubmit={(e) => {
                e.preventDefault();
                start();
            }}
        >
            <input
                type="password"
                bind:value={passphrase}
                autocomplete="current-password"
                placeholder="Owner passphrase"
                aria-label="Owner passphrase"
                class="flex-1 rounded-md border border-gray-300 px-2 py-1 text-sm dark:border-gray-600"
            />
            <button
                type="submit"
                disabled={busy || !passphrase}
                class="rounded-md border border-gray-300 px-3 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
            >
                Continue
            </button>
            <button type="button" onclick={cancel} class="text-xs underline">cancel</button>
        </form>
    {:else if stage === 'code'}
        <p class="mb-2">Type the four-digit code now showing on the unit's screen.</p>
        <form
            class="flex flex-wrap gap-2"
            onsubmit={(e) => {
                e.preventDefault();
                confirm();
            }}
        >
            <input
                type="text"
                bind:value={code}
                inputmode="numeric"
                autocomplete="one-time-code"
                placeholder="0000"
                aria-label="Code from the unit's screen"
                class="w-24 rounded-md border border-gray-300 px-2 py-1 font-mono text-sm tracking-widest dark:border-gray-600"
            />
            <button
                type="submit"
                disabled={busy || code.length < 4}
                class="rounded-md border border-gray-300 px-3 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
            >
                Confirm
            </button>
            <button type="button" onclick={cancel} class="text-xs underline">cancel</button>
        </form>
    {:else}
        <p>Now press the button on the unit. Waiting…</p>
        <button type="button" onclick={cancel} class="mt-1 text-xs underline">cancel</button>
    {/if}
    {#if error}
        <p class="mt-1 text-xs text-red-600 dark:text-red-400">{error}</p>
    {/if}
</div>
