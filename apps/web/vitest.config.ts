import { defineConfig } from 'vitest/config'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { svelteTesting } from '@testing-library/svelte/vite'
import { resolve } from 'path'

export default defineConfig({
  plugins: [svelte({ hot: !process.env.VITEST }), svelteTesting()],
  resolve: {
    alias: {
      '$wasm': resolve(__dirname, './src/lib/wasm-stub.js'),
      '$lib': resolve(__dirname, './src/lib'),
      '@': resolve(__dirname, './src'),
    },
  },
  test: {
    globals: true,
    environment: 'jsdom',
    include: ['src/**/*.{test,spec}.{js,ts}'],
    setupFiles: ['./src/test/setup.ts'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'json'],
      include: ['src/**/*.{ts,svelte}'],
      exclude: [
        'src/**/*.test.ts',
        'src/**/*.spec.ts',
        'src/**/*.d.ts',
        'src/test/**',
        'src/lib/test/**',
        'src/lib/backend/generated/**',
        'src/lib/backend/serde_json/**',
        'src/lib/backend/wasmWorkerNew.ts',
        'src/lib/components/ui/**',
        'src/lib/wasm/**',
        'src/main.ts',
      ],
    },
  },
})
