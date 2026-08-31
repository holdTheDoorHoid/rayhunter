<script lang="ts">
    import Modal from './Modal.svelte';
    import Explainer from './Explainer.svelte';
    import { run_terminal_command, type TerminalResult } from '../utils.svelte';

    /**
     * Run commands on the device from the browser.
     *
     * Each request is one command with nothing carried between them. There is
     * no shell session to hijack and nothing left open, which is the whole
     * reason it is built this way rather than as a real terminal.
     */
    let { shown = $bindable() }: { shown: boolean } = $props();

    interface Entry {
        command: string;
        result?: TerminalResult;
        error?: string;
    }

    let command = $state('');
    let history = $state<Entry[]>([]);
    let running = $state(false);
    /** Position when arrowing back through previous commands. */
    let recall = $state(-1);
    let output_el = $state<HTMLDivElement | null>(null);

    /**
     * Commands worth having to hand, since most of what anyone needs here is
     * the same handful of questions about a device they cannot see.
     */
    const SUGGESTIONS = [
        { label: 'Disk', command: 'df -h' },
        { label: 'Memory', command: 'free' },
        { label: 'Processes', command: 'ps' },
        { label: 'Recordings', command: 'ls -la /data/rayhunter/qmdl | tail -20' },
        { label: 'Kernel log', command: 'dmesg | tail -40' },
        { label: 'Uptime', command: 'uptime' },
    ];

    async function submit() {
        const text = command.trim();
        if (!text || running) return;
        running = true;
        recall = -1;
        // Where this command's entry will sit, so the result can be written
        // back through the array below.
        const index = history.length;
        history = [...history, { command: text }];
        command = '';
        try {
            const result = await run_terminal_command(text);
            // Through `history`, never through a reference kept from before it
            // was added. Putting an object into a `$state` array stores a proxy
            // of it, and the template reads that proxy; assigning to the plain
            // object we started with writes past the proxy, so the output never
            // appears and the entry reads "running" for ever. That was a real
            // bug: the request succeeded and returned the output every time.
            history[index].result = result;
        } catch (e) {
            history[index].error = `${e}`;
        } finally {
            running = false;
            // Scroll after the DOM has caught up with the new entry.
            queueMicrotask(() => output_el?.scrollTo({ top: output_el.scrollHeight }));
        }
    }

    /** Up and down walk previous commands, as a shell does. */
    function onkeydown(event: KeyboardEvent) {
        const past = history.map((h) => h.command);
        if (event.key === 'ArrowUp' && past.length) {
            event.preventDefault();
            recall = recall < 0 ? past.length - 1 : Math.max(0, recall - 1);
            command = past[recall];
        } else if (event.key === 'ArrowDown' && recall >= 0) {
            event.preventDefault();
            recall = recall + 1;
            if (recall >= past.length) {
                recall = -1;
                command = '';
            } else {
                command = past[recall];
            }
        }
    }
</script>

<Modal bind:shown title="Terminal">
    <div class="flex h-full flex-col p-2">
        <p class="text-xs text-gray-500 dark:text-gray-400">
            Commands run on the device as root, in a fresh shell each time. Nothing is carried
            between them, so a directory you change into does not persist.
        </p>

        <div class="mt-2 flex flex-wrap gap-1">
            {#each SUGGESTIONS as s (s.command)}
                <button
                    type="button"
                    onclick={() => (command = s.command)}
                    class="rounded-full border border-gray-300 px-2 py-0.5 text-xs text-gray-700 hover:bg-gray-100 dark:border-gray-600 dark:text-gray-200 dark:hover:bg-gray-800"
                >
                    {s.label}
                </button>
            {/each}
        </div>

        <div
            bind:this={output_el}
            class="mt-2 min-h-40 flex-1 overflow-auto rounded-md bg-gray-100 p-2 font-mono text-xs dark:bg-gray-800"
        >
            {#if history.length === 0}
                <p class="text-gray-500 dark:text-gray-400">
                    Nothing run yet. Pick one above, or type a command below.
                </p>
            {/if}
            {#each history as entry, i (i)}
                <div class="mb-2">
                    <!-- Rayhunter's blue is dark, which is fine on the light
                         panel and too dim to read on the dark one. -->
                    <div class="text-rayhunter-blue dark:text-indigo-300">$ {entry.command}</div>
                    {#if entry.error}
                        <pre
                            class="whitespace-pre-wrap text-red-600 dark:text-red-400">{entry.error}</pre>
                    {:else if entry.result}
                        {#if entry.result.stdout}
                            <pre class="whitespace-pre-wrap">{entry.result.stdout}</pre>
                        {/if}
                        {#if entry.result.stderr}
                            <pre
                                class="whitespace-pre-wrap text-amber-700 dark:text-amber-300">{entry
                                    .result.stderr}</pre>
                        {/if}
                        {#if entry.result.timed_out}
                            <div class="text-red-600 dark:text-red-400">
                                killed after 15 seconds
                            </div>
                        {:else if entry.result.exit_code !== null && entry.result.exit_code !== 0}
                            <div class="text-amber-700 dark:text-amber-300">
                                exit code {entry.result.exit_code}
                            </div>
                        {:else if !entry.result.stdout && !entry.result.stderr}
                            <div class="text-gray-500 dark:text-gray-400">(no output)</div>
                        {/if}
                    {:else}
                        <div class="text-gray-500 dark:text-gray-400">running…</div>
                    {/if}
                </div>
            {/each}
        </div>

        <form
            class="mt-2 flex gap-2"
            onsubmit={(e) => {
                e.preventDefault();
                submit();
            }}
        >
            <input
                type="text"
                bind:value={command}
                {onkeydown}
                disabled={running}
                spellcheck="false"
                autocapitalize="off"
                autocomplete="off"
                placeholder="Command to run on the device"
                aria-label="Command to run on the device"
                class="flex-1 rounded-md border border-gray-300 px-2 py-1 font-mono text-sm disabled:opacity-50 dark:border-gray-600"
            />
            <button
                type="submit"
                disabled={running || !command.trim()}
                class="rounded-md border border-gray-300 px-3 py-1 text-sm disabled:opacity-40 dark:border-gray-600"
            >
                {running ? 'Running…' : 'Run'}
            </button>
        </form>

        <Explainer summary="What this can and cannot do, and what having it on means.">
            <p>
                Commands run as root, because that is what the Rayhunter daemon runs as; it needs
                that to read the modem. So anything typed here can change or destroy anything on the
                device.
            </p>
            <p>
                Each command is run on its own, with a fresh shell and no memory of the last one.
                There is no session to take over and nothing left running afterwards. A command
                still going after fifteen seconds is killed, and very long output is cut short.
            </p>
            <p>
                This is off unless it was switched on when the device was flashed, and the web
                interface cannot switch it on. If it is available here, someone chose that while
                holding the device. Setting a password under Configuration is strongly worth doing
                while it is on.
            </p>
        </Explainer>
    </div>
</Modal>
