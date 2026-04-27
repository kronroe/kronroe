import { test, expect, type Page } from '@playwright/test';

/**
 * Compliance harness for the Kronroe consent banner.
 *
 * Covers the seven core flows from the build plan:
 *   1. First visit  — banner appears, no cookies, GA in 'denied' default
 *   2. Accept all   — cookie + GA granted + LinkedIn injected
 *   3. Reject all   — cookie denied + GA still denied + no LinkedIn
 *   4. Customise    — partial consent persists exactly as toggled
 *   5. Withdraw     — footer link → reject → GA flips back to denied
 *   6. Expiry       — expired cookie is treated as no record (re-prompt)
 *   7. Re-open      — focus management + a11y dialog invariants
 *
 * Plus two integrity checks:
 *   • Policy version bump invalidates stored consent
 *   • aria-modal + focus trap when modal is open
 *
 * All tests are independent: each starts on a fresh `/` with no cookies.
 *
 * Note on third-party network: tests verify the SHAPE of the consent contract
 * (cookie + dataLayer + script-tag presence). They do NOT assert on real GA4 /
 * LinkedIn beacons firing — those are external services we don't control and
 * shouldn't depend on for CI stability.
 */

const COOKIE_NAME = 'kronroe_consent';
const POLICY_VERSION = 1;
const SCHEMA_VERSION = 1;

// ─── Helpers ──────────────────────────────────────────────────

async function getConsentCookie(page: Page) {
  const cookies = await page.context().cookies();
  return cookies.find((c) => c.name === COOKIE_NAME);
}

async function getConsentRecord(page: Page) {
  return page.evaluate(() => (window as any).kronroeConsent.getRecord());
}

async function getLastConsentUpdate(page: Page) {
  return page.evaluate(() => {
    const dl = (window as any).dataLayer || [];
    const updates = dl.filter(
      (a: any) => a[0] === 'consent' && a[1] === 'update',
    );
    return updates.length
      ? Array.from(updates[updates.length - 1] as ArrayLike<any>)
      : null;
  });
}

async function bannerVisible(page: Page) {
  return page.locator('#kr-consent-banner').isVisible();
}

async function modalVisible(page: Page) {
  return page.locator('#kr-consent-backdrop').isVisible();
}

async function linkedInLoaded(page: Page) {
  return page.evaluate(
    () => !!document.querySelector('script[src*="snap.licdn.com"]'),
  );
}

// ─── 1. First visit ───────────────────────────────────────────

test('first visit shows banner with no cookies and consent denied by default', async ({
  page,
}) => {
  await page.goto('/');

  expect(await bannerVisible(page)).toBe(true);
  expect(await getConsentCookie(page)).toBeUndefined();
  expect(await getConsentRecord(page)).toBeNull();

  // Consent default must be the very first dataLayer entry.
  const firstEntry = await page.evaluate(() =>
    Array.from((window as any).dataLayer[0] as ArrayLike<any>),
  );
  expect(firstEntry[0]).toBe('consent');
  expect(firstEntry[1]).toBe('default');
  expect(firstEntry[2]).toMatchObject({
    analytics_storage: 'denied',
    ad_storage: 'denied',
    ad_user_data: 'denied',
    ad_personalization: 'denied',
  });

  // No LinkedIn script before consent.
  expect(await linkedInLoaded(page)).toBe(false);

  // Banner is announced as a region with the right label (a11y).
  await expect(page.getByRole('region', { name: /cookie consent/i })).toBeVisible();
});

// ─── 2. Accept all ────────────────────────────────────────────

test('accept all writes record, grants all signals, loads LinkedIn', async ({
  page,
}) => {
  await page.goto('/');
  await page.locator('#kr-consent-banner button[data-kr="accept"]').click();

  expect(await bannerVisible(page)).toBe(false);

  const record = await getConsentRecord(page);
  expect(record).toMatchObject({
    schemaVersion: SCHEMA_VERSION,
    policyVersion: POLICY_VERSION,
    choices: { necessary: true, analytics: true, marketing: true },
  });
  expect(record.recordedAt).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  expect(record.expiresAt).toMatch(/^\d{4}-\d{2}-\d{2}T/);

  // Expiry must be ~12 months in the future (allow ±1 day for clock skew).
  const recorded = Date.parse(record.recordedAt);
  const expires = Date.parse(record.expiresAt);
  const days = (expires - recorded) / (1000 * 60 * 60 * 24);
  expect(days).toBeGreaterThan(364);
  expect(days).toBeLessThan(367);

  const lastUpdate = await getLastConsentUpdate(page);
  expect(lastUpdate?.[2]).toMatchObject({
    analytics_storage: 'granted',
    ad_storage: 'granted',
    ad_user_data: 'granted',
    ad_personalization: 'granted',
  });

  // Wait briefly for async LinkedIn injection.
  await expect
    .poll(() => linkedInLoaded(page), { timeout: 2000 })
    .toBe(true);
});

// ─── 3. Reject all ────────────────────────────────────────────

test('reject all writes denied record, no GA grant, no LinkedIn', async ({
  page,
}) => {
  await page.goto('/');
  await page.locator('#kr-consent-banner button[data-kr="reject"]').click();

  expect(await bannerVisible(page)).toBe(false);

  const record = await getConsentRecord(page);
  expect(record.choices).toEqual({
    necessary: true,
    analytics: false,
    marketing: false,
  });

  const lastUpdate = await getLastConsentUpdate(page);
  expect(lastUpdate?.[2]).toMatchObject({
    analytics_storage: 'denied',
    ad_storage: 'denied',
    ad_user_data: 'denied',
    ad_personalization: 'denied',
  });

  // LinkedIn must not be present even if we wait.
  await page.waitForTimeout(500);
  expect(await linkedInLoaded(page)).toBe(false);
});

// ─── 4. Customise — analytics only ────────────────────────────

test('customise with analytics only persists exactly that combination', async ({
  page,
}) => {
  await page.goto('/');
  await page.locator('#kr-consent-banner button[data-kr="customise"]').click();

  await expect(page.locator('#kr-consent-backdrop')).toBeVisible();

  // Toggle analytics on, leave marketing off.
  // The checkbox input is visually hidden (opacity:0) under a custom
  // slider — real users click the label, which forwards to the input.
  await page
    .locator('#kr-consent-modal label.kr-toggle')
    .filter({ has: page.locator('input[data-cat="analytics"]') })
    .click();

  await page
    .locator('#kr-consent-backdrop button[data-kr="save"]')
    .click();

  expect(await modalVisible(page)).toBe(false);

  const record = await getConsentRecord(page);
  expect(record.choices).toEqual({
    necessary: true,
    analytics: true,
    marketing: false,
  });

  const lastUpdate = await getLastConsentUpdate(page);
  expect(lastUpdate?.[2]).toMatchObject({
    analytics_storage: 'granted',
    ad_storage: 'denied',
    ad_user_data: 'denied',
    ad_personalization: 'denied',
  });

  expect(await linkedInLoaded(page)).toBe(false);
});

// ─── 5. Withdraw via footer link ──────────────────────────────

test('withdraw via footer link flips GA signals back to denied', async ({
  page,
}) => {
  await page.goto('/');
  // Start by accepting all.
  await page.locator('#kr-consent-banner button[data-kr="accept"]').click();

  const linkedInWasLoaded = await linkedInLoaded(page);

  // Open the modal via the footer "Cookie preferences" link.
  await page.getByRole('link', { name: /cookie preferences/i }).click();
  await expect(page.locator('#kr-consent-backdrop')).toBeVisible();

  // Reject all from inside the modal.
  await page
    .locator('#kr-consent-backdrop button[data-kr="reject"]')
    .click();

  expect(await modalVisible(page)).toBe(false);

  const record = await getConsentRecord(page);
  expect(record.choices.analytics).toBe(false);
  expect(record.choices.marketing).toBe(false);

  const lastUpdate = await getLastConsentUpdate(page);
  expect(lastUpdate?.[2]).toMatchObject({
    analytics_storage: 'denied',
    ad_storage: 'denied',
    ad_user_data: 'denied',
    ad_personalization: 'denied',
  });

  // Per design: withdraw stops FUTURE collection, not the current
  // session's loaded scripts. LinkedIn script remains in the DOM but
  // the loader guard prevents re-injection on subsequent navigations.
  expect(await linkedInLoaded(page)).toBe(linkedInWasLoaded);
});

// ─── 6. Expiry ────────────────────────────────────────────────

test('expired consent record is treated as no record and re-prompts', async ({
  page,
  context,
}) => {
  // Seed a consent cookie that has already expired.
  const expiredAt = new Date(Date.now() - 1000).toISOString();
  const recordedAt = new Date(Date.now() - 1000 * 60 * 60 * 24 * 400).toISOString();
  const stale = {
    schemaVersion: SCHEMA_VERSION,
    policyVersion: POLICY_VERSION,
    recordedAt,
    expiresAt: expiredAt,
    choices: { necessary: true, analytics: true, marketing: true },
  };

  await context.addCookies([
    {
      name: COOKIE_NAME,
      value: encodeURIComponent(JSON.stringify(stale)),
      domain: '127.0.0.1',
      path: '/',
      expires: Math.floor(Date.now() / 1000) + 60, // browser-level still valid; app-level expired
      httpOnly: false,
      secure: false,
      sameSite: 'Lax',
    },
  ]);

  await page.goto('/');

  // Banner must reappear because the in-record expiresAt is stale.
  expect(await bannerVisible(page)).toBe(true);
  expect(await getConsentRecord(page)).toBeNull();
});

// ─── 7. Re-open + focus management ────────────────────────────

test('footer link reopens modal, Escape closes it, focus returns to link', async ({
  page,
}) => {
  await page.goto('/');
  await page.locator('#kr-consent-banner button[data-kr="accept"]').click();

  const link = page.getByRole('link', { name: /cookie preferences/i });
  await link.scrollIntoViewIfNeeded();
  await link.focus();
  await link.click();

  await expect(page.locator('#kr-consent-backdrop')).toBeVisible();

  // Prior choices (accept-all) should be prefilled.
  await expect(
    page.locator('#kr-consent-modal input[data-cat="analytics"]'),
  ).toBeChecked();
  await expect(
    page.locator('#kr-consent-modal input[data-cat="marketing"]'),
  ).toBeChecked();

  // Close via Escape.
  await page.keyboard.press('Escape');
  await expect(page.locator('#kr-consent-backdrop')).toHaveCount(0);

  // Focus returns to the triggering link.
  const focusReturned = await page.evaluate(
    () =>
      document.activeElement?.textContent?.includes('Cookie preferences') ?? false,
  );
  expect(focusReturned).toBe(true);
});

// ─── Integrity check: policy version bump invalidates stored consent ──

test('policy version mismatch invalidates stored consent', async ({
  page,
  context,
}) => {
  const futureRecord = {
    schemaVersion: SCHEMA_VERSION,
    policyVersion: 999, // higher than current — simulates user holds old cookie after a policy bump
    recordedAt: new Date().toISOString(),
    expiresAt: new Date(Date.now() + 1000 * 60 * 60 * 24 * 365).toISOString(),
    choices: { necessary: true, analytics: true, marketing: true },
  };

  await context.addCookies([
    {
      name: COOKIE_NAME,
      value: encodeURIComponent(JSON.stringify(futureRecord)),
      domain: '127.0.0.1',
      path: '/',
      expires: Math.floor(Date.now() / 1000) + 60 * 60 * 24,
      httpOnly: false,
      secure: false,
      sameSite: 'Lax',
    },
  ]);

  await page.goto('/');

  // Even though the cookie hasn't expired, the policyVersion mismatch
  // forces a fresh prompt.
  expect(await bannerVisible(page)).toBe(true);
  expect(await getConsentRecord(page)).toBeNull();
});

// ─── Integrity check: ARIA dialog invariants while modal is open ──

test('modal is a properly-structured WAI-ARIA dialog with focus trap', async ({
  page,
}) => {
  await page.goto('/');
  await page.locator('#kr-consent-banner button[data-kr="customise"]').click();

  const inner = page.locator('#kr-consent-modal');
  await expect(inner).toHaveAttribute('role', 'dialog');
  await expect(inner).toHaveAttribute('aria-modal', 'true');
  await expect(inner).toHaveAttribute('aria-labelledby', 'kr-consent-h');

  // Background should be marked inert.
  const inertStates = await page.evaluate(() => ({
    main: !!document.querySelector('main')?.hasAttribute('inert'),
    header: !!document.querySelector('header')?.hasAttribute('inert'),
    footer: !!document.querySelector('footer')?.hasAttribute('inert'),
  }));
  // Not every page has all three semantic landmarks — but if they exist,
  // they must be inert.
  for (const [tag, isInert] of Object.entries(inertStates)) {
    const exists = await page.locator(tag).count();
    if (exists > 0) {
      expect(isInert, `<${tag}> should be inert while modal is open`).toBe(true);
    }
  }

  // Focus trap: tab from the last focusable wraps to the first.
  const focusables = page.locator(
    '#kr-consent-modal button, #kr-consent-modal input:not([disabled])',
  );
  const count = await focusables.count();
  expect(count).toBeGreaterThan(0);

  await focusables.last().focus();
  await page.keyboard.press('Tab');

  const firstFocused = await page.evaluate(() => {
    const first = document.querySelector(
      '#kr-consent-modal button, #kr-consent-modal input:not([disabled])',
    );
    return document.activeElement === first;
  });
  expect(firstFocused).toBe(true);
});
