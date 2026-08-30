<script lang="ts">
    import '../app.css';
    import { theme } from '$lib/theme.svelte';
    let { children } = $props();

    $effect(() => {
        theme.init();
    });
</script>

{@render children()}
