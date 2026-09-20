<script lang="ts">
  import { library } from '../lib/library.svelte';
  import { scanStatus } from '../lib/status';

  /** One entry per running scan, in watched-folder order, each with a bar. */
  const scans = $derived(
    library.folders.watched
      .filter((w) => library.isScanning(w.id))
      .map((w) => scanStatus(w, library.scans[w.id], library.expected[w.id])),
  );
</script>

<footer class="status">
  <span class="notices">
    {#if library.anyDegraded}
      <span>Live updates limited — photon will re-check these folders periodically.</span>
    {/if}
    {#if library.exporting}
      <span class="scan" role="status">
        <span>Exporting {library.exporting.done.toLocaleString()} / {library.exporting.total.toLocaleString()}…</span>
        <progress aria-label="Export progress" value={library.exporting.done} max={library.exporting.total}></progress>
      </span>
    {/if}
    {#each scans as scan (scan.watchedId)}
      <span class="scan" role="status">
        <span>{scan.label}</span>
        <!-- A `<progress>` with no value is the browser's own indeterminate bar, which is
             exactly what a first scan is: the walk cannot know its total ahead of time. -->
        {#if scan.fraction === null}
          <progress aria-label="Scan progress"></progress>
        {:else}
          <progress aria-label="Scan progress" value={scan.fraction} max="1"></progress>
        {/if}
      </span>
    {/each}
  </span>
  <span>
    {#if library.selectionCount > 1}<span class="selected">{library.selectionCount.toLocaleString()} selected</span> · {/if}{library.info.len.toLocaleString()} photos
  </span>
</footer>

<style>
  .status {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-1) var(--s-3);
    background: var(--chrome);
    color: var(--text-dim);
    font-size: var(--t-2);
    border-top: 1px solid var(--line);
  }
  .notices { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 16px; min-width: 0; }
  .scan { display: inline-flex; align-items: center; gap: var(--s-2); white-space: nowrap; }
  /* Brighter than the footer's --text-dim: the count is the only thing here that changes
     under the user's hand, and it answers "what will the menu act on". */
  .selected { color: var(--text); }
  progress {
    width: 120px;
    height: 6px;
    appearance: none;
    border: 0;
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }
  progress::-webkit-progress-bar { background: var(--line); border-radius: 3px; }
  progress::-webkit-progress-value { background: var(--accent); border-radius: 3px; transition: width 200ms ease-out; }
  progress::-moz-progress-bar { background: var(--accent); border-radius: 3px; }
</style>
