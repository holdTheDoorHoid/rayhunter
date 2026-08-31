<script lang="ts">
    import '../app.css';
    import { theme } from '$lib/theme.svelte';
    import { help } from '$lib/helpVisibility.svelte';
    let { children } = $props();

    $effect(() => {
        theme.init();
        help.init();
    });
</script>

{@render children()}
