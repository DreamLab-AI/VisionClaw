import path from 'node:path';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  root: __dirname,
  cacheDir: path.resolve(__dirname, '../../node_modules/.vite/codescape'),
  resolve: {
    alias: { '@cs': path.resolve(__dirname, 'src') },
  },
  test: {
    environment: 'jsdom',
    globals: false,
    env: { NODE_ENV: 'test' },
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
  },
});
