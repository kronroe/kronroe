import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright config for kronroe.dev consent + analytics tests.
 *
 * The webServer block starts a Python static server against the source `site/`
 * directory (not `site/dist/`) on port 5180 so tests run against the live
 * source, mirroring the existing `site-static` launch profile in
 * .claude/launch.json.
 */
export default defineConfig({
  testDir: './specs',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: [['list'], ['html', { open: 'never' }]],

  use: {
    baseURL: 'http://127.0.0.1:5180',
    trace: 'on-first-retry',
    // Tests run as if a real EU/UK visitor — we never want geolocation
    // fast-paths to skip the banner.
    locale: 'en-GB',
    timezoneId: 'Europe/London',
  },

  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],

  webServer: {
    command: 'python3 -m http.server 5180 --bind 127.0.0.1 --directory ../../site',
    url: 'http://127.0.0.1:5180',
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
