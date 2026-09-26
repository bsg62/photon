/** The viewer's video controls, apart from the markup that draws them so the behaviour can be
 *  tested against a fake element; `VideoControls.svelte` only renders this state and forwards
 *  pointer events to it.
 *
 *  photon draws its own controls rather than the webview's `controls` attribute: those look
 *  different in WebKitGTK, WKWebView and WebView2, ignore the viewer's styling, and sit in a
 *  strip of their own above photon's bar. */

/** Shift+←/→ moves this far. Plain arrows stay the viewer's, moving between items. */
export const SEEK_STEP_S = 5;

/** What the element the player drives must offer: a subset of `HTMLVideoElement`. */
export interface PlayerMedia {
  paused: boolean;
  currentTime: number;
  duration: number;
  volume: number;
  muted: boolean;
  loop: boolean;
  play(): Promise<void>;
  pause(): void;
  addEventListener(type: string, listener: () => void): void;
  removeEventListener(type: string, listener: () => void): void;
}

/** Settings that follow the user from one video to the next. Kept for as long as photon is
 *  open and saved nowhere: a video opening with no sound because of something done to another
 *  one a week ago would read as a fault. */
export interface VideoPrefs {
  loop: boolean;
  muted: boolean;
  volume: number;
}

/** The one set the viewer shares across the videos it opens. */
export const sessionPrefs: VideoPrefs = { loop: false, muted: false, volume: 1 };

const EVENTS = ['play', 'pause', 'timeupdate', 'durationchange', 'loadedmetadata', 'volumechange'] as const;

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

export function createVideoPlayer(prefs: VideoPrefs = sessionPrefs) {
  let media: PlayerMedia | null = null;
  let playing = $state(false);
  let time = $state(0);
  let duration = $state(Number.NaN);
  let loop = $state(prefs.loop);
  let muted = $state(prefs.muted);
  let volume = $state(prefs.volume);
  let scrubbing = $state(false);
  /** Where a scrub began, so Escape can put the video back there. */
  let scrubFrom = 0;
  /** Whether it was playing when the scrub began: it pauses while dragged, so each seek shows
   *  its frame instead of playing on from it, and resumes when let go. */
  let resumeAfterScrub = false;

  function sync() {
    if (!media) return;
    playing = !media.paused;
    time = media.currentTime;
    duration = media.duration;
    muted = media.muted;
    volume = media.volume;
  }

  /** The length, or null while it is unknown (no metadata yet, or a stream without one):
   *  seeking needs a length to clamp to. */
  function knownDuration(): number | null {
    return media && Number.isFinite(media.duration) && media.duration > 0 ? media.duration : null;
  }

  function seekTo(fraction: number) {
    const length = knownDuration();
    if (!media || length === null) return;
    media.currentTime = clamp(fraction, 0, 1) * length;
    time = media.currentTime;
  }

  function seekBy(seconds: number) {
    const length = knownDuration();
    if (!media || length === null) return;
    media.currentTime = clamp(media.currentTime + seconds, 0, length);
    time = media.currentTime;
  }

  async function toggle() {
    if (!media) return;
    if (media.paused) {
      // `play()` rejects when the webview aborts it (a quick double press) or refuses it
      // (autoplay policy); neither is something to report, but an uncaught rejection would
      // surface as an unhandled promise.
      await media.play().catch(() => {});
    } else {
      media.pause();
    }
  }

  function toggleLoop() {
    loop = !loop;
    prefs.loop = loop;
    if (media) media.loop = loop;
  }

  function toggleMute() {
    muted = !muted;
    prefs.muted = muted;
    if (media) media.muted = muted;
  }

  function setVolume(value: number) {
    volume = clamp(value, 0, 1);
    prefs.volume = volume;
    if (media) media.volume = volume;
    // Turning the sound up is asking to hear it: a muted player that moves its slider and
    // stays silent looks broken.
    if (muted && volume > 0) toggleMute();
  }

  return {
    get playing() { return playing; },
    get time() { return time; },
    get duration() { return duration; },
    get loop() { return loop; },
    get muted() { return muted; },
    get volume() { return volume; },
    get scrubbing() { return scrubbing; },

    /** Drives `element`, handing it the carried-over settings, and stops listening to the
     *  one before. Returns the detach, for an effect's teardown. */
    attach(element: PlayerMedia) {
      const previous = media;
      if (previous) EVENTS.forEach((type) => previous.removeEventListener(type, sync));
      media = element;
      element.loop = prefs.loop;
      element.muted = prefs.muted;
      element.volume = prefs.volume;
      loop = prefs.loop;
      EVENTS.forEach((type) => element.addEventListener(type, sync));
      sync();
      return () => {
        if (media !== element) return;
        EVENTS.forEach((type) => element.removeEventListener(type, sync));
        media = null;
      };
    },

    toggle,
    seekTo,
    seekBy,
    toggleLoop,
    toggleMute,
    setVolume,

    beginScrub() {
      if (!media) return;
      scrubbing = true;
      scrubFrom = media.currentTime;
      resumeAfterScrub = !media.paused;
      if (resumeAfterScrub) media.pause();
    },
    scrubTo(fraction: number) {
      if (scrubbing) seekTo(fraction);
    },
    endScrub() {
      if (!scrubbing) return;
      scrubbing = false;
      if (resumeAfterScrub) void toggle();
    },
    /** Escape during a drag: back to where it began, as if the drag had not happened. */
    abandonScrub() {
      if (!scrubbing || !media) return;
      media.currentTime = scrubFrom;
      time = scrubFrom;
      scrubbing = false;
      if (resumeAfterScrub) void toggle();
    },

    /** The keys a video answers: Space, L (loop), M (mute) and Shift+←/→ (seek). Returns
     *  whether it took the key. Plain arrows are left to the viewer, which moves between
     *  items with them; a modifier other than Shift means the key belongs to the webview. */
    handleKey(e: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'ctrlKey' | 'metaKey' | 'altKey'>): boolean {
      if (e.ctrlKey || e.metaKey || e.altKey) return false;
      if (e.shiftKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
        seekBy(e.key === 'ArrowLeft' ? -SEEK_STEP_S : SEEK_STEP_S);
        return true;
      }
      switch (e.key) {
        case ' ':
          void toggle();
          return true;
        case 'l':
        case 'L':
          toggleLoop();
          return true;
        case 'm':
        case 'M':
          toggleMute();
          return true;
        default:
          return false;
      }
    },
  };
}

export type VideoPlayer = ReturnType<typeof createVideoPlayer>;

/** Where along a bar of `rect` a pointer at `clientX` lies, from 0 to 1. */
export function fractionAt(clientX: number, rect: { left: number; width: number }): number {
  return rect.width > 0 ? clamp((clientX - rect.left) / rect.width, 0, 1) : 0;
}
