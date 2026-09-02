<script lang="ts">
    // The pairing page: how a browser becomes one this unit trusts.
    //
    // Reached three ways. From the setup link on the unit's screen
    // (`/s/<token>`), where the token is already in the address; from the
    // unit sending an unpaired browser here; or on purpose, to add a
    // browser with the owner passphrase. It works out which from the address
    // and from what the unit says about itself, and shows one form, not a
    // menu of them.
    import { onMount } from 'svelte';

    type Status = {
        setup_complete: boolean;
        window_open: boolean;
        seconds_left: number;
        paired_devices: number;
        has_passphrase: boolean;
        has_accounts: boolean;
        tls_port: number;
    };

    type Mode =
        | 'setup'
        | 'code'
        | 'passphrase'
        | 'account'
        | 'press'
        | 'press-setup'
        | 'closed'
        | 'loading';

    let status: Status | null = $state(null);
    let mode: Mode = $state('loading');
    let from_link = $state(false);
    let token = $state('');
    let passphrase = $state('');
    let confirm = $state('');
    let device_name = $state('');
    let username = $state('');
    let password = $state('');
    let busy = $state(false);
    let error = $state('');
    let fingerprint = $state('');
    let insecure = $state(false);
    let code = $state('');
    let from_code_link = $state(false);
    let press_id = $state('');
    let press_seconds = $state(0);
    let press_timer: ReturnType<typeof setInterval> | null = null;

    const MIN_PASSPHRASE = 8;

    async function get_json<T>(url: string): Promise<T> {
        const r = await fetch(url);
        if (!r.ok) throw new Error(await r.text());
        return (await r.json()) as T;
    }

    async function post_json(url: string, body: unknown): Promise<void> {
        const r = await fetch(url, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body),
        });
        if (!r.ok) throw new Error(await r.text());
    }

    function decide_mode(s: Status): Mode {
        if (!s.setup_complete && s.window_open) return 'setup';
        if (s.has_passphrase) return 'passphrase';
        if (s.has_accounts) return 'account';
        return 'closed';
    }

    onMount(async () => {
        insecure = location.protocol !== 'https:';
        const m = location.pathname.match(/^\/s\/([^/]+)/i);
        if (m) {
            token = decodeURIComponent(m[1]).toUpperCase();
            from_link = true;
        }
        const c = location.pathname.match(/^\/p\/([^/]+)/i);
        if (c) {
            code = decodeURIComponent(c[1]);
            from_code_link = true;
        }
        try {
            status = await get_json<Status>('/api/setup/status');
            mode = from_code_link ? 'code' : decide_mode(status);
        } catch (e) {
            error = e instanceof Error ? e.message : String(e);
            mode = 'closed';
        }
        try {
            const t = await get_json<{ fingerprint_sha256: string }>('/api/tls-info');
            fingerprint = t.fingerprint_sha256;
        } catch {
            fingerprint = '';
        }
    });

    function done() {
        // Paired. Everything from here on is the ordinary interface.
        location.replace('/');
    }

    async function submit_setup() {
        error = '';
        if (passphrase.length < MIN_PASSPHRASE) {
            error = `The passphrase needs at least ${MIN_PASSPHRASE} characters.`;
            return;
        }
        if (passphrase !== confirm) {
            error = 'The two passphrases do not match.';
            return;
        }
        busy = true;
        try {
            await post_json('/api/setup/complete', {
                token,
                passphrase,
                device_name: device_name || null,
                browser_unix_ms: Date.now(),
            });
            done();
        } catch (e) {
            error = e instanceof Error ? e.message : String(e);
            // The unit may have closed the window on us; re-read where we stand.
            try {
                status = await get_json<Status>('/api/setup/status');
                if (!status.window_open) mode = decide_mode(status);
            } catch {
                /* keep what we have */
            }
        } finally {
            busy = false;
        }
    }

    async function submit_passphrase() {
        error = '';
        busy = true;
        try {
            await post_json('/api/pair/passphrase', {
                passphrase,
                device_name: device_name || null,
            });
            done();
        } catch (e) {
            error = e instanceof Error ? e.message : String(e);
        } finally {
            busy = false;
        }
    }

    async function submit_account() {
        error = '';
        busy = true;
        try {
            await post_json('/api/pair/account', {
                username,
                password,
                device_name: device_name || null,
            });
            done();
        } catch (e) {
            error = e instanceof Error ? e.message : String(e);
        } finally {
            busy = false;
        }
    }

    async function submit_code() {
        error = '';
        busy = true;
        try {
            await post_json('/api/pair/code', { code, device_name: device_name || null });
            done();
        } catch (e) {
            error = e instanceof Error ? e.message : String(e);
        } finally {
            busy = false;
        }
    }

    // Pair by pressing the button on the unit: for units with no screen,
    // and for a phone that will not scan one. The unit says "press the
    // button" on its screen if it has one; a press within the window
    // approves this browser, which then chooses the passphrase as usual.
    async function request_press() {
        error = '';
        busy = true;
        try {
            const r = await fetch('/api/setup/press-request', { method: 'POST' });
            if (!r.ok) throw new Error(await r.text());
            const j = (await r.json()) as { id: string; seconds: number };
            press_id = j.id;
            press_seconds = j.seconds;
            mode = 'press';
            press_timer = setInterval(poll_press, 1000);
        } catch (e) {
            error = e instanceof Error ? e.message : String(e);
        } finally {
            busy = false;
        }
    }

    async function poll_press() {
        press_seconds = Math.max(0, press_seconds - 1);
        try {
            const r = await fetch(`/api/setup/press-status/${encodeURIComponent(press_id)}`);
            if (r.status === 410) throw new Error('Nobody pressed the button in time.');
            if (!r.ok) throw new Error(await r.text());
            const j = (await r.json()) as { approved: boolean };
            if (j.approved) {
                stop_polling();
                mode = 'press-setup';
            }
        } catch (e) {
            stop_polling();
            error = e instanceof Error ? e.message : String(e);
            mode = status ? decide_mode(status) : 'closed';
        }
    }

    function stop_polling() {
        if (press_timer) clearInterval(press_timer);
        press_timer = null;
    }

    async function submit_press_setup() {
        error = '';
        if (passphrase.length < MIN_PASSPHRASE) {
            error = `The passphrase needs at least ${MIN_PASSPHRASE} characters.`;
            return;
        }
        if (passphrase !== confirm) {
            error = 'The two passphrases do not match.';
            return;
        }
        busy = true;
        try {
            await post_json('/api/setup/complete-press', {
                id: press_id,
                passphrase,
                device_name: device_name || null,
                browser_unix_ms: Date.now(),
            });
            done();
        } catch (e) {
            error = e instanceof Error ? e.message : String(e);
        } finally {
            busy = false;
        }
    }

    function minutes_left(s: Status | null): string {
        if (!s) return '';
        const m = Math.max(1, Math.round(s.seconds_left / 60));
        return `${m} minute${m === 1 ? '' : 's'}`;
    }
</script>

<svelte:head>
    <title>Pair with Rayhunter</title>
</svelte:head>

<main class="mx-auto max-w-md p-4 text-gray-900 dark:text-gray-100">
    <div class="mb-6 flex items-center gap-3">
        <img src="/rayhunter_orca_only.png" alt="" class="h-12 w-12" />
        <h1 class="text-2xl font-bold">Rayhunter</h1>
    </div>

    {#if insecure}
        <div
            class="mb-4 rounded border border-yellow-400 bg-yellow-50 p-3 text-sm text-yellow-900 dark:border-yellow-600 dark:bg-yellow-950 dark:text-yellow-100"
        >
            This page is not on the secure address. Pairing only works over
            <strong>https</strong>, on port {status?.tls_port ?? 8443}.
        </div>
    {/if}

    {#if mode === 'loading'}
        <p class="text-gray-600 dark:text-gray-400">Asking the unit where it stands…</p>
    {:else if mode === 'setup'}
        <h2 class="mb-1 text-xl font-semibold">Welcome. This unit is yours to set up.</h2>
        <p class="mb-4 text-sm text-gray-600 dark:text-gray-400">
            Choose a passphrase. You will use it to add another phone or computer, or to get back in
            if you lose this one. This browser is trusted from now on and will not ask again. The
            setup code on the unit's screen is good for another
            {minutes_left(status)}.
        </p>
        <form
            class="space-y-4"
            onsubmit={(e) => {
                e.preventDefault();
                submit_setup();
            }}
        >
            <label class="block">
                <span class="text-sm font-medium">Code from the unit's screen</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 font-mono text-lg tracking-widest uppercase dark:border-gray-600 dark:bg-gray-800"
                    bind:value={token}
                    autocomplete="off"
                    autocapitalize="characters"
                    spellcheck="false"
                    placeholder="XXXX XXXX"
                    readonly={from_link}
                    required
                />
                {#if from_link}
                    <span class="text-xs text-gray-500">Read from the code you scanned.</span>
                {/if}
            </label>
            <label class="block">
                <span class="text-sm font-medium"
                    >Passphrase (at least {MIN_PASSPHRASE} characters)</span
                >
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    type="password"
                    bind:value={passphrase}
                    autocomplete="new-password"
                    minlength={MIN_PASSPHRASE}
                    required
                />
            </label>
            <label class="block">
                <span class="text-sm font-medium">Same passphrase again</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    type="password"
                    bind:value={confirm}
                    autocomplete="new-password"
                    required
                />
            </label>
            <label class="block">
                <span class="text-sm font-medium">Name for this phone or computer (optional)</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    bind:value={device_name}
                    maxlength="64"
                    placeholder="e.g. Sam's phone"
                />
            </label>
            {#if error}
                <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
            {/if}
            <button
                class="w-full rounded bg-blue-600 p-2 font-semibold text-white hover:bg-blue-700 disabled:opacity-50"
                type="submit"
                disabled={busy}
            >
                {busy ? 'Setting up…' : 'Set up and pair this browser'}
            </button>
        </form>
        <button
            class="mt-4 text-sm text-blue-700 underline dark:text-blue-300"
            onclick={request_press}
            disabled={busy}
        >
            Can't scan or read the code? Pair by pressing the button on the unit instead
        </button>
    {:else if mode === 'code'}
        <h2 class="mb-1 text-xl font-semibold">Pair this browser with a code</h2>
        <p class="mb-4 text-sm text-gray-600 dark:text-gray-400">
            Enter the six-digit code shown on a phone or computer that is already paired with this
            unit. It is good for a few minutes and one device.
        </p>
        <form
            class="space-y-4"
            onsubmit={(e) => {
                e.preventDefault();
                submit_code();
            }}
        >
            <label class="block">
                <span class="text-sm font-medium">Code</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 font-mono text-lg tracking-widest dark:border-gray-600 dark:bg-gray-800"
                    bind:value={code}
                    inputmode="numeric"
                    autocomplete="one-time-code"
                    placeholder="123 456"
                    readonly={from_code_link}
                    required
                />
            </label>
            <label class="block">
                <span class="text-sm font-medium">Name for this phone or computer (optional)</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    bind:value={device_name}
                    maxlength="64"
                />
            </label>
            {#if error}
                <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
            {/if}
            <button
                class="w-full rounded bg-blue-600 p-2 font-semibold text-white hover:bg-blue-700 disabled:opacity-50"
                type="submit"
                disabled={busy}
            >
                {busy ? 'Checking…' : 'Pair'}
            </button>
        </form>
        {#if status?.has_passphrase}
            <button
                class="mt-4 text-sm text-blue-700 underline dark:text-blue-300"
                onclick={() => (mode = 'passphrase')}
            >
                Use the owner passphrase instead
            </button>
        {/if}
    {:else if mode === 'press'}
        <h2 class="mb-1 text-xl font-semibold">Now press the button on the unit</h2>
        <p class="mb-4 text-sm text-gray-600 dark:text-gray-400">
            Any press will do. This page is waiting for it: {press_seconds} seconds left.
        </p>
        <button
            class="rounded border border-gray-400 px-3 py-1 text-sm"
            onclick={() => {
                stop_polling();
                mode = status ? decide_mode(status) : 'closed';
            }}
        >
            Cancel
        </button>
    {:else if mode === 'press-setup'}
        <h2 class="mb-1 text-xl font-semibold">Button pressed. This unit is yours to set up.</h2>
        <p class="mb-4 text-sm text-gray-600 dark:text-gray-400">
            Choose a passphrase. You will use it to add another phone or computer, or to get back in
            if you lose this one.
        </p>
        <form
            class="space-y-4"
            onsubmit={(e) => {
                e.preventDefault();
                submit_press_setup();
            }}
        >
            <label class="block">
                <span class="text-sm font-medium"
                    >Passphrase (at least {MIN_PASSPHRASE} characters)</span
                >
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    type="password"
                    bind:value={passphrase}
                    autocomplete="new-password"
                    minlength={MIN_PASSPHRASE}
                    required
                />
            </label>
            <label class="block">
                <span class="text-sm font-medium">Same passphrase again</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    type="password"
                    bind:value={confirm}
                    autocomplete="new-password"
                    required
                />
            </label>
            <label class="block">
                <span class="text-sm font-medium">Name for this phone or computer (optional)</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    bind:value={device_name}
                    maxlength="64"
                />
            </label>
            {#if error}
                <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
            {/if}
            <button
                class="w-full rounded bg-blue-600 p-2 font-semibold text-white hover:bg-blue-700 disabled:opacity-50"
                type="submit"
                disabled={busy}
            >
                {busy ? 'Setting up…' : 'Set up and pair this browser'}
            </button>
        </form>
    {:else if mode === 'passphrase'}
        <h2 class="mb-1 text-xl font-semibold">Pair this browser</h2>
        <p class="mb-4 text-sm text-gray-600 dark:text-gray-400">
            This unit already has an owner. Enter the owner passphrase to trust this browser. You
            will not be asked again on this device.
        </p>
        <form
            class="space-y-4"
            onsubmit={(e) => {
                e.preventDefault();
                submit_passphrase();
            }}
        >
            <label class="block">
                <span class="text-sm font-medium">Owner passphrase</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    type="password"
                    bind:value={passphrase}
                    autocomplete="current-password"
                    required
                />
            </label>
            <label class="block">
                <span class="text-sm font-medium">Name for this phone or computer (optional)</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    bind:value={device_name}
                    maxlength="64"
                />
            </label>
            {#if error}
                <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
            {/if}
            <button
                class="w-full rounded bg-blue-600 p-2 font-semibold text-white hover:bg-blue-700 disabled:opacity-50"
                type="submit"
                disabled={busy}
            >
                {busy ? 'Checking…' : 'Pair'}
            </button>
        </form>
        <div class="mt-4 flex flex-col gap-2">
            <button
                class="text-left text-sm text-blue-700 underline dark:text-blue-300"
                onclick={() => (mode = 'code')}
            >
                I have a code from a device that is already paired
            </button>
            {#if status?.has_accounts}
                <button
                    class="text-left text-sm text-blue-700 underline dark:text-blue-300"
                    onclick={() => (mode = 'account')}
                >
                    Sign in with a Rayhunter username and password instead
                </button>
            {/if}
        </div>
    {:else if mode === 'account'}
        <h2 class="mb-1 text-xl font-semibold">Sign in to pair this browser</h2>
        <p class="mb-4 text-sm text-gray-600 dark:text-gray-400">
            This unit has a Rayhunter account from before pairing existed. Sign in with it once;
            this browser is trusted from then on, and the account's password becomes the owner
            passphrase.
        </p>
        <form
            class="space-y-4"
            onsubmit={(e) => {
                e.preventDefault();
                submit_account();
            }}
        >
            <label class="block">
                <span class="text-sm font-medium">Username</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    bind:value={username}
                    autocomplete="username"
                    required
                />
            </label>
            <label class="block">
                <span class="text-sm font-medium">Password</span>
                <input
                    class="mt-1 w-full rounded border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800"
                    type="password"
                    bind:value={password}
                    autocomplete="current-password"
                    required
                />
            </label>
            {#if error}
                <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
            {/if}
            <button
                class="w-full rounded bg-blue-600 p-2 font-semibold text-white hover:bg-blue-700 disabled:opacity-50"
                type="submit"
                disabled={busy}
            >
                {busy ? 'Checking…' : 'Sign in and pair'}
            </button>
        </form>
    {:else}
        <h2 class="mb-1 text-xl font-semibold">This unit is waiting to be set up</h2>
        <p class="mb-4 text-sm text-gray-600 dark:text-gray-400">
            {#if status && !status.setup_complete}
                Nobody owns it yet and the setup code is not showing. Press the button on the unit:
                the code appears on its screen for ten minutes. Scan it, or open this page again and
                type it in.
            {:else}
                There is no way to pair from here right now.
            {/if}
        </p>
        {#if error}
            <p class="text-sm text-red-600 dark:text-red-400">{error}</p>
        {/if}
        <div class="flex flex-wrap gap-2">
            <button
                class="rounded border border-gray-400 px-3 py-1 text-sm"
                onclick={() => location.reload()}
            >
                Check again
            </button>
            {#if status && !status.setup_complete}
                <button
                    class="rounded bg-blue-600 px-3 py-1 text-sm font-semibold text-white"
                    onclick={request_press}
                    disabled={busy}
                >
                    Pair by pressing the button on the unit
                </button>
            {/if}
        </div>
    {/if}

    {#if fingerprint}
        <div
            class="mt-8 border-t border-gray-200 pt-4 text-xs text-gray-500 dark:border-gray-700 dark:text-gray-400"
        >
            <p class="mb-1">
                Your browser warned about this unit's certificate because the unit made it itself;
                nobody on the internet vouches for it, and nothing on the internet is involved. To
                check you are talking to your own unit, compare this fingerprint with the one your
                browser shows:
            </p>
            <code class="break-all">{fingerprint}</code>
        </div>
    {/if}
</main>
