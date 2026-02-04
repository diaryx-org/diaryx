import { defineConfig, devices } from '@playwright/test'

const webHost = process.env.PW_WEB_HOST ?? '127.0.0.1'
const webPort = process.env.PW_WEB_PORT ?? '5174'
const webServerUrl = `http://${webHost}:${webPort}`
const appBaseUrl = process.env.PW_BASE_URL ?? `http://localhost:${webPort}`

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 1,
  workers: process.env.CI ? 1 : undefined,
  // Use list reporter for real-time output, HTML for detailed reports
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: appBaseUrl,
    trace: 'on-first-retry',
    video: 'on-first-retry',
    screenshot: 'only-on-failure',
    actionTimeout: 10000,
    navigationTimeout: 30000,
  },
  // Global timeout for each test
  timeout: 60000,
  // Expect timeout for assertions
  expect: {
    timeout: 10000,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'webkit',
      use: { ...devices['Desktop Safari'] },
    },
  ],
  webServer: {
    command: `bun run dev -- --host ${webHost} --port ${webPort} --strictPort`,
    url: webServerUrl,
    reuseExistingServer: !process.env.CI,
    timeout: 120000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
})
