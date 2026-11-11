import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: process.env.TLAB_UI_BASE_URL ?? 'http://localhost:5173',
    trace: 'off',
    screenshot: 'off',
    video: 'off',
  },
})
