/** The `client` test project's environment (see `vite.config.ts`): Node in every way but
 *  one - modules are transformed for the client, so Svelte's client runtime is the one that
 *  runs and effects actually re-run. There is still no DOM. */
export default {
  name: 'svelte-client',
  viteEnvironment: 'client',
  setup() {
    return { teardown() {} };
  },
};
