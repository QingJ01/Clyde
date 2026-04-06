import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import path from 'path';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: process.env.TAURI_ENV_PLATFORM == 'windows'
      ? 'chrome105'
      : process.env.TAURI_ENV_PLATFORM == 'macos'
      ? 'safari13'
      : ['es2021', 'chrome100'],
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rollupOptions: {
      input: {
        pet:    path.resolve(__dirname, 'src/windows/pet/index.html'),
        hit:    path.resolve(__dirname, 'src/windows/hit/index.html'),
        bubble: path.resolve(__dirname, 'src/windows/bubble/index.html'),
        menu:   path.resolve(__dirname, 'src/windows/menu/index.html'),
        tasks:  path.resolve(__dirname, 'src/windows/tasks/index.html'),
      },
    },
  },
});
