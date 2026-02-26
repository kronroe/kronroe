import { chromium } from "playwright";

function requireUrl() {
  const fromArg = process.argv[2];
  const fromEnv = process.env.PLAYGROUND_URL;
  const url = fromArg || fromEnv;
  if (!url) {
    throw new Error("Missing target URL. Pass PLAYGROUND_URL or argv[2].");
  }
  return url;
}

async function run() {
  const targetUrl = requireUrl();
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();

  const subject = `ci-user-${Date.now().toString(36).slice(-6)}`;
  const predicate = "works_at";
  const object = "AcmeCI";

  try {
    await page.goto(targetUrl, { waitUntil: "networkidle", timeout: 120_000 });

    await page.waitForSelector("#subject", { timeout: 60_000 });
    await page.locator("#subject").fill(subject);
    await page.locator("#predicate").fill(predicate);
    await page.locator("#object").fill(object);
    await page.locator("#obj-type").selectOption("Entity");
    await page.locator("#assert-btn").click();

    await page.waitForFunction(
      () => {
        const el = document.querySelector("#assert-status");
        return !!el && el.textContent?.includes("✓");
      },
      undefined,
      { timeout: 30_000 }
    );

    await page.locator("#query-entity").fill(subject);
    await page.locator("#query-pred").fill(predicate);
    await page.locator("#query-btn").click();

    await page.waitForFunction(
      () => {
        const el = document.querySelector("#query-status");
        return !!el && /result/i.test(el.textContent || "");
      },
      undefined,
      { timeout: 30_000 }
    );

    const row = page.locator("#stream-body .fact-row").first();
    await row.locator(".btn-invalidate").click();

    await page.waitForFunction(
      () => {
        const el = document.querySelector("#query-status");
        return !!el && /retracted:/i.test(el.textContent || "");
      },
      undefined,
      { timeout: 30_000 }
    );

    const queryStatus = (await page.locator("#query-status").textContent()) || "";
    const assertStatus = (await page.locator("#assert-status").textContent()) || "";

    console.log(
      JSON.stringify(
        {
          ok: true,
          url: targetUrl,
          subject,
          predicate,
          object,
          assertStatus: assertStatus.trim(),
          queryStatus: queryStatus.trim(),
        },
        null,
        2
      )
    );
  } finally {
    await browser.close();
  }
}

run().catch((error) => {
  console.error("Post-deploy playground smoke failed:", error);
  process.exit(1);
});
