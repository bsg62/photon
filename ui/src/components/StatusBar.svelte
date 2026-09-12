<script lang="ts">
  import { library } from '../lib/library.svelte';

  const scanning = $derived(
    library.folders.watched
      .filter((w) => library.isScanning(w.id))
      .map((w) => `Scanning ${w.path.split(/[\\/]/).pop()}… ${library.scans[w.id].filesSeen.toLocaleString()} files`),
  );
  const notices = $derived(
    library.anyDegraded
      ? ['Live updates limited — photon will re-check these folders periodically.', ...scanning]
      : scanning,
  );
</script>

<footer class="status">
  <span>{notices.join(' · ')}</span>
  <span>{library.info.len.toLocaleString()} photos</span>
</footer>

<style>
  .status {
    display: flex;
    justify-content: space-between;
    padding: 4px 12px;
    background: var(--panel);
    color: var(--muted);
    font-size: 12px;
    border-top: 1px solid #0003;
  }
</style>
