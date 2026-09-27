/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: 'es2022' },
  // `css: true`: vitest otherwise turns every CSS import into an empty string, `?raw`
  // included, and tokens.test.ts reads tokens.css as text.
  //
  // Two projects. Almost everything runs in plain Node, where a `.svelte.ts` module is
  // compiled for Svelte's server runtime - and there `$effect` never runs, so what re-runs a
  // reaction cannot be observed at all: a test that an effect did *not* re-run passes
  // vacuously. A `*.client.test.ts` file gets the client runtime instead, for the few tests
  // that are about exactly that. Still Node, still no DOM.
  //
  // It takes both halves: `client-environment.ts` has modules transformed for the client
  // (so runes compile to the client runtime), and the `browser` condition makes `svelte`
  // itself resolve to its client entry - vitest resolves the client environment with Node's
  // conditions, which pick `index-server.js`, whose `flushSync` does nothing.
  test: {
    css: true,
    projects: [
      { extends: true, test: { name: 'node', include: ['src/**/*.test.ts'], exclude: ['src/**/*.client.test.ts'], environment: 'node' } },
      {
        extends: true,
        environments: { client: { resolve: { conditions: ['browser'] } } },
        test: { name: 'client', include: ['src/**/*.client.test.ts'], environment: './src/client-environment.ts' },
      },
    ],
  },
});
