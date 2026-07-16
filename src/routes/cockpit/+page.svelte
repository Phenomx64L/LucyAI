<script>
  /* Lucy 2.0 cockpit preview (Phase F2). Isolated route — additive, does not
     touch the main +page.svelte or the current UI. Navigate to /cockpit to see
     the two-lane shell. Delete this folder to revert entirely. */
  import CockpitShell from '$lib/cockpit/CockpitShell.svelte';
</script>

<svelte:head>
  <title>Lucy · Cockpit (preview F2)</title>
  <link rel="preconnect" href="https://fonts.googleapis.com" />
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="anonymous" />
  <link
    href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500&family=JetBrains+Mono:wght@400;500&display=swap"
    rel="stylesheet"
  />
</svelte:head>

<CockpitShell />

<style>
  :global(body) { margin: 0; }
</style>
