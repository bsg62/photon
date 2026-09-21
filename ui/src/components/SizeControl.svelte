<script lang="ts">
  import { gridSize } from '../lib/app-grid-size.svelte';
  import type { TileSize } from '../lib/layout';

  const SIZES: { value: TileSize; label: string }[] = [
    { value: 'small', label: 'Small' },
    { value: 'medium', label: 'Medium' },
    { value: 'large', label: 'Large' },
  ];
</script>

<!-- A group of independent toggle buttons, each its own Tab stop with Enter/Space to
     activate - the same pattern as the theme control in Settings, and not the APG
     radiogroup pattern, because role="radio" without roving-tabindex key handling lies to
     a screen reader. -->
<div class="segmented" role="group" aria-label="Photo size">
  {#each SIZES as option (option.value)}
    <button
      aria-pressed={gridSize.size === option.value}
      class:checked={gridSize.size === option.value}
      onclick={() => gridSize.set(option.value)}
    >
      {option.label}
    </button>
  {/each}
</div>

<style>
  /* The `.segmented`/`.segmented button` rules are copied from Settings.svelte. Svelte scopes
     component styles, so this component does not inherit that file's generic `button` rule
     the way its theme control does - `border`, `cursor` and `transition` below are restated
     from that rule for exactly that reason, so the two controls compute to the same
     appearance. Do not delete them as redundant; nothing else here supplies them. */
  .segmented { display: inline-flex; gap: 2px; padding: 2px; border-radius: var(--r-3); background: var(--field); }
  .segmented button {
    padding: 4px 14px;
    border: 0;
    border-radius: var(--r-2);
    background: none;
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  .segmented button:hover:not(:disabled) { background: var(--hover); }
  /* After the hover rule and spelled as long, so the chosen segment keeps its accent under
     the pointer by specificity rather than by source order alone. */
  .segmented button.checked, .segmented button.checked:hover:not(:disabled) { background: var(--accent); color: var(--on-accent); }
  @media (prefers-reduced-motion: reduce) { .segmented button { transition: none; } }
</style>
