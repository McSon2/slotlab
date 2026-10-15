import { defineConfig } from 'vite';

/** `pnpm build:cli` — the Analyze findings as a Node module for the CLI and the MCP server. */
export default defineConfig({
  build: {
    ssr: 'src/cli/report.ts',
    outDir: '../scripts',
    emptyOutDir: false,
    minify: false,
    target: 'node18',
    rollupOptions: { output: { format: 'es', entryFileNames: 'slotlab-report.mjs' } },
  },
});
