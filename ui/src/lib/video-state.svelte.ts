/** Set once, in `App.svelte`'s `onMount`: where videos are served, and whether this webview
 *  can play them. Until both are known nothing creates a `<video>`. */
export const videoState = $state<{ base: string | null; supported: boolean }>({ base: null, supported: false });
