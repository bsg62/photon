<script lang="ts">
  import { ICONS, type IconName } from '../lib/icons';

  let { name, size = 16, filled = false }: { name: IconName; size?: number; filled?: boolean } = $props();
</script>

<!-- Decoration: the button around it carries the aria-label. `{@html}` is safe here
     because ICONS is a compile-time constant, never user data. -->
<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={filled ? 'currentColor' : 'none'}
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {@html ICONS[name]}
</svg>

<style>
  /* An inline SVG sits on the text baseline and drags its line taller; as a block-level
     flex item it takes exactly its own box. */
  svg { display: block; flex: none; }
</style>
