<script lang="ts">
    // Stopping the certificate warning for good, for people who want to.
    //
    // The unit has its own certificate authority. Installing that on a
    // phone or computer makes the browser treat the unit's certificate as
    // genuine from then on: no warning, a padlock, and later on the things
    // browsers only allow on trusted pages. It is optional, and it is one
    // device at a time, so the steps are laid out per operating system with
    // the reader's own first.
    import type { TlsInfo } from '../utils.svelte';

    let { tls }: { tls: TlsInfo } = $props();

    type Os = 'iphone' | 'android' | 'mac' | 'windows' | 'linux';
    const ALL: { id: Os; label: string }[] = [
        { id: 'iphone', label: 'iPhone or iPad' },
        { id: 'android', label: 'Android' },
        { id: 'mac', label: 'Mac' },
        { id: 'windows', label: 'Windows' },
        { id: 'linux', label: 'Linux' },
    ];

    function guess(): Os {
        const ua = navigator.userAgent;
        if (/iPhone|iPad|iPod/.test(ua)) return 'iphone';
        if (/Android/.test(ua)) return 'android';
        if (/Mac OS|Macintosh/.test(ua)) return 'mac';
        if (/Windows/.test(ua)) return 'windows';
        return 'linux';
    }

    let os = $state<Os>(guess());
    let open = $state(false);
</script>

<div class="mt-4 rounded-md border border-gray-200 p-3 text-sm dark:border-gray-700">
    <button
        type="button"
        class="flex w-full items-center justify-between text-left font-medium"
        onclick={() => (open = !open)}
        aria-expanded={open}
    >
        <span>Stop the certificate warning for good (optional)</span>
        <span class="text-gray-500">{open ? '−' : '+'}</span>
    </button>
    {#if open}
        <p class="mt-2 text-gray-600 dark:text-gray-400">
            This unit signs its own certificate, and that is all your browser objects to. Tell this
            phone or computer to trust the unit's certificate authority, "{tls.ca_name}", and the
            warning does not come back on it. Each device is done separately; the unit never asks
            for it.
        </p>
        <div class="mt-2 flex flex-wrap gap-1">
            {#each ALL as o (o.id)}
                <button
                    type="button"
                    onclick={() => (os = o.id)}
                    class="rounded-full border px-2 py-0.5 text-xs {os === o.id
                        ? 'border-blue-600 bg-blue-600 text-white'
                        : 'border-gray-300 dark:border-gray-600'}"
                >
                    {o.label}
                </button>
            {/each}
        </div>
        <ol class="mt-3 list-decimal space-y-1 pl-5">
            {#if os === 'iphone'}
                <li>
                    <a class="underline" href="/api/ca.mobileconfig">Download the profile</a>.
                    Safari says a profile was downloaded; allow it.
                </li>
                <li>
                    Open <strong>Settings</strong>. Near the top, tap
                    <strong>Profile Downloaded</strong>, then <strong>Install</strong>, and enter
                    your passcode.
                </li>
                <li>
                    Go to <strong>Settings → General → About → Certificate Trust Settings</strong>
                    and turn on full trust for "{tls.ca_name}".
                </li>
                <li>Reopen this page. The warning is gone.</li>
            {:else if os === 'android'}
                <li><a class="underline" href="/api/ca.crt">Download the certificate</a>.</li>
                <li>
                    Open <strong>Settings</strong> and search for <strong>CA certificate</strong>
                    (under Security, Encryption &amp; credentials, Install a certificate). Choose
                    <strong>CA certificate</strong>, accept the warning that says installing one is
                    risky, and pick the downloaded file.
                </li>
                <li>
                    Reopen this page. Chrome and most browsers now trust the unit; Firefox needs
                    "Use third party CA certificates" turned on in its own settings.
                </li>
            {:else if os === 'mac'}
                <li>
                    <a class="underline" href="/api/ca.mobileconfig">Download the profile</a> and open
                    it. macOS says the profile must be installed from System Settings.
                </li>
                <li>
                    Open <strong>System Settings → Privacy &amp; Security → Profiles</strong>,
                    select "{tls.ca_name}" and install it.
                </li>
                <li>
                    Open <strong>Keychain Access</strong>, find "{tls.ca_name}" under login or
                    System, open it, expand <strong>Trust</strong>, and set "When using this
                    certificate" to
                    <strong>Always Trust</strong>.
                </li>
                <li>
                    Reopen this page in Safari or Chrome. Firefox needs its own import (see Linux).
                </li>
            {:else if os === 'windows'}
                <li>
                    <a class="underline" href="/api/ca.crt">Download the certificate</a> and open it.
                </li>
                <li>
                    Choose <strong>Install Certificate</strong>, then <strong>Current User</strong>,
                    then
                    <strong>Place all certificates in the following store</strong> and pick
                    <strong>Trusted Root Certification Authorities</strong>. Confirm the warning.
                </li>
                <li>
                    Reopen this page in Edge or Chrome. Firefox needs its own import (see Linux).
                </li>
            {:else}
                <li><a class="underline" href="/api/ca.pem">Download the certificate</a>.</li>
                <li>
                    Chrome: open <code>chrome://settings/certificates</code>, the
                    <strong>Authorities</strong> tab, <strong>Import</strong>, and tick "Trust this
                    certificate for identifying websites".
                </li>
                <li>
                    Firefox: <strong
                        >Settings → Privacy &amp; Security → View Certificates → Authorities →
                        Import</strong
                    >, and tick the same.
                </li>
                <li>Reopen this page.</li>
            {/if}
        </ol>
        <p class="mt-3 text-xs text-gray-500 dark:text-gray-400">
            To check you are installing your own unit's authority and not somebody else's, its
            fingerprint is
            <code class="break-all">{tls.ca_fingerprint_sha256}</code>. It can be removed the same
            way it was installed. A device that trusts it trusts only this one unit.
        </p>
    {/if}
</div>
