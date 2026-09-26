<script lang="ts">
  import { fractionAt, SEEK_STEP_S, type VideoPlayer } from '../lib/video-player.svelte';
  import { formatDuration } from '../lib/video';
  import Icon from './Icon.svelte';

  let { player }: { player: VideoPlayer } = $props();

  const known = $derived(Number.isFinite(player.duration) && player.duration > 0);
  const progress = $derived(known ? Math.min(1, player.time / player.duration) : 0);
  /** The pointer a scrub belongs to, so a second finger or pen cannot steer or end it. */
  let pointer: number | null = null;

  // A drag has three endings, not two (CLAUDE.md): `pointerup` finishes it, Escape abandons
  // it (the viewer's key handler calls `abandonScrub`), and `pointercancel` - a browser
  // claiming the gesture, a touchscreen pan - abandons it too, since the user did not choose
  // where it stopped.
  function onpointerdown(e: PointerEvent & { currentTarget: HTMLElement }) {
    if (e.button !== 0 || !known) return;
    pointer = e.pointerId;
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      // A pointer the browser does not know (a synthetic one) cannot be captured; the drag
      // still works while it stays over the bar.
    }
    player.beginScrub();
    player.scrubTo(fractionAt(e.clientX, e.currentTarget.getBoundingClientRect()));
  }

  function onpointermove(e: PointerEvent & { currentTarget: HTMLElement }) {
    if (e.pointerId !== pointer || !player.scrubbing) return;
    player.scrubTo(fractionAt(e.clientX, e.currentTarget.getBoundingClientRect()));
  }

  function onpointerup(e: PointerEvent) {
    if (e.pointerId !== pointer) return;
    pointer = null;
    player.endScrub();
  }

  function onpointercancel(e: PointerEvent) {
    if (e.pointerId !== pointer) return;
    pointer = null;
    player.abandonScrub();
  }

  // An Escape handled by the viewer ends the scrub without a pointer event reaching here;
  // forget the pointer too, or its later `pointerup` would end a scrub that is not running.
  $effect(() => {
    if (!player.scrubbing) pointer = null;
  });

  /** A range input keeps focus after it is used, and the viewer leaves every key alone while
   *  an input has it - so Space would stop playing and pausing until the user clicked
   *  elsewhere. Let go of it the moment the drag ends. */
  function release(e: Event & { currentTarget: HTMLInputElement }) {
    e.currentTarget.blur();
  }
</script>

<div class="video-controls">
  <button
    class="tool"
    onclick={() => player.toggle()}
    aria-label={player.playing ? 'Pause' : 'Play'}
    title={player.playing ? 'Pause (Space)' : 'Play (Space)'}
  >
    <Icon name={player.playing ? 'pause' : 'play'} size={16} />
  </button>
  <span class="time">{formatDuration(player.time * 1000)}</span>
  <!-- Not a range input: those keep focus and swallow the arrow keys, which the viewer needs
       for moving between items. Seeking by keyboard is Shift+←/→ on the viewer. -->
  <div
    class="seek"
    class:disabled={!known}
    role="slider"
    tabindex="-1"
    aria-label="Position"
    aria-valuemin={0}
    aria-valuemax={known ? Math.round(player.duration) : 0}
    aria-valuenow={Math.round(player.time)}
    aria-valuetext={formatDuration(player.time * 1000)}
    title={`Drag to move through the video (Shift+←/→: ${SEEK_STEP_S} s)`}
    {onpointerdown}
    {onpointermove}
    {onpointerup}
    {onpointercancel}
  >
    <div class="track"><div class="fill" style:width="{progress * 100}%"></div></div>
  </div>
  <span class="time">{known ? formatDuration(player.duration * 1000) : '-:--'}</span>
  <span class="sep" aria-hidden="true"></span>
  <button
    class="tool"
    onclick={() => player.toggleMute()}
    aria-pressed={player.muted}
    aria-label={player.muted ? 'Unmute' : 'Mute'}
    title={player.muted ? 'Unmute (M)' : 'Mute (M)'}
  >
    <Icon name={player.muted ? 'volume-x' : 'volume-2'} size={16} />
  </button>
  <input
    class="volume"
    type="range"
    min="0"
    max="1"
    step="0.05"
    aria-label="Volume"
    value={player.muted ? 0 : player.volume}
    oninput={(e) => player.setVolume(Number(e.currentTarget.value))}
    onchange={release}
  />
  <button
    class="tool"
    onclick={() => player.toggleLoop()}
    aria-pressed={player.loop}
    aria-label="Loop"
    title={player.loop ? 'Stop looping (L)' : 'Loop (L)'}
  >
    <Icon name="repeat" size={16} />
  </button>
</div>

<style>
  /* Above the viewer's bar and centred like it, in the same glass (see the bar's own comment
     in Viewer.svelte for why 90% opaque); the video is inset to clear both. */
  .video-controls {
    position: absolute; bottom: 64px; left: 50%; transform: translateX(-50%);
    display: flex; align-items: center; gap: var(--s-2);
    width: min(720px, calc(100% - 428px)); box-sizing: border-box;
    padding: var(--s-1) var(--s-2); border-radius: var(--r-4);
    background: var(--glass);
    box-shadow: 0 0 0 1px var(--glass-line), var(--shadow-menu);
    -webkit-backdrop-filter: blur(18px);
    backdrop-filter: blur(18px);
  }
  .tool { display: grid; place-items: center; flex: none; width: 30px; height: 30px; padding: 0; border: 0; border-radius: var(--r-3); background: none; color: var(--text-dim); line-height: 1; cursor: pointer; transition: background-color 120ms ease-out; }
  .tool:hover { color: var(--text); background: var(--hover); }
  .tool[aria-pressed='true'], .tool[aria-pressed='true']:hover { background: var(--accent); color: var(--on-accent); }
  .time { flex: none; min-width: 3.2em; color: var(--text-dim); font-size: var(--t-2); font-variant-numeric: tabular-nums; text-align: center; }
  /* The hit area is the full height of the strip; the drawn track is a thin line inside it. */
  .seek { flex: 1 1 auto; display: flex; align-items: center; height: 30px; cursor: pointer; touch-action: none; }
  .seek.disabled { cursor: default; opacity: 0.4; }
  .track { flex: 1; height: 4px; border-radius: 2px; background: var(--glass-line); overflow: hidden; }
  .fill { height: 100%; background: var(--accent); }
  .sep { flex: none; width: 1px; height: 18px; background: var(--glass-line); }
  .volume { flex: none; width: 80px; accent-color: var(--accent); }
  @media (prefers-reduced-motion: reduce) { .tool { transition: none; } }
</style>
