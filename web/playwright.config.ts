import { defineConfig, devices } from '@playwright/test'

const port = Number(process.env.DOCUMOSA_E2E_PORT ?? 5175)
const baseURL = `http://127.0.0.1:${port}`

export default defineConfig({
  testDir: './tests',
  reporter: 'list',
  use: {
    baseURL,
    trace: 'on-first-retry',
  },
  webServer: {
    command: `npm run dev -- --host 127.0.0.1 --port ${port} --strictPort`,
    url: baseURL,
    reuseExistingServer: false,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
})
