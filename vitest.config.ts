import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [vue()],
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/tests/frontend/support/setup.ts'],
    include: ['src/tests/frontend/**/*.test.ts'],
    restoreMocks: true,
  },
})
