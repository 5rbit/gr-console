import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// 분리 SPA — gr-console 백엔드를 소비한다.
// - dev: `/api`를 백엔드(기본 127.0.0.1:8090)로 프록시 → 같은 origin이라 CORS 불요.
//   SSE(`/api/status/stream` 등)도 프록시가 그대로 스트리밍한다.
// - build: `dist/`로 산출 → 백엔드가 정적 서빙(prod 같은 origin).
const BACKEND = process.env.GR_BACKEND ?? 'http://127.0.0.1:8090'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      '/api': { target: BACKEND, changeOrigin: true },
    },
  },
  build: { outDir: 'dist', emptyOutDir: true },
})
