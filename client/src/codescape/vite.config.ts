import path from 'node:path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Codescape is its own Vite root under client/src/codescape (decision record §9).
// root, outDir and the alias are explicit because Vite would otherwise resolve
// them relative to the process cwd (client/) rather than this file.
const isolationHeaders = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
};

export default defineConfig({
  root: __dirname,
  cacheDir: path.resolve(__dirname, '../../node_modules/.vite/codescape'),
  base: './',
  plugins: [react()],
  resolve: {
    alias: { '@cs': path.resolve(__dirname, 'src') },
  },
  worker: { format: 'es' },
  build: {
    outDir: process.env.CODESCAPE_DIST ?? path.resolve(__dirname, '../../../target/codescape-dist'),
    emptyOutDir: true,
    target: 'es2022',
    sourcemap: true,
    chunkSizeWarningLimit: 1500,
  },
  server: { headers: isolationHeaders, fs: { allow: [path.resolve(__dirname, '..', '..')] } },
  preview: { headers: isolationHeaders },
});
