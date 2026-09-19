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
  test: { include: ['src/**/*.test.ts'], environment: 'node', css: true },
});
