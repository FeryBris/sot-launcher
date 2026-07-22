import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5174,
    strictPort: true,
    watch: {
      // Never watch the Rust build output — cargo/tauri already watches
      // src-tauri's *source*, and target/ (especially an AppImage bundle's
      // copied system libraries) can contain enough files to blow past
      // Linux's inotify watch limit on its own.
      ignored: ['**/src-tauri/**'],
    },
  },
})
