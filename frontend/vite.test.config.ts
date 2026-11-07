import { defineConfig } from 'vite';

/** `pnpm test` — the pure lib/ tests bundled for Node, then run by `node --test` (no extra dependency). */
export default defineConfig({
  build: {
    ssr: 'src/lib/muted.test.ts',
    outDir: '.test-dist',
    emptyOutDir: true,
    minify: false,
    target: 'node18',
    rollupOptions: { output: { format: 'es', entryFileNames: '[name].mjs' } },
  },
});
