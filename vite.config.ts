import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri expects a fixed port, and a build with no external resources.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ['**/src-tauri/**', '**/target/**', '**/.dev/**'] } },
  build: { target: 'es2022', chunkSizeWarningLimit: 3000 },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
} as never);
