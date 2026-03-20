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

function createFailure(error, details) {
  const message = error instanceof Error ? error.message : String(error);
  return {
    ok: false,
    ...details,
    error: message,
  };
}

async function run() {
  const targetUrl = requireUrl();
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  const pageErrors = [];
  const consoleErrors = [];

  const subject = `ci-user-${Date.now().toString(36).slice(-6)}`;
  const predicate = "works_at";
  const object = "AcmeCI";

  try {
    page.on("pageerror", (error) => {
      pageErrors.push(error.message);
    });
    page.on("console", (message) => {
      if (message.type() === "error") {
        consoleErrors.push(message.text());
      }
    });

    // Suppress the first-visit auto-demo — it races with smoke test inputs
    await page.addInitScript(() => {
      localStorage.setItem("kronroe_has_visited", "1");
    });

    await page.goto(targetUrl, { waitUntil: "networkidle", timeout: 120_000 });

    // Wait for WASM engine to finish loading (overlay gets .hidden class)
    await page.locator("#loading.hidden").waitFor({ timeout: 60_000 });
    await page.waitForSelector("#subject", { timeout: 60_000 });
    await page.locator("#subject").fill(subject);
    await page.locator("#predicate").fill(predicate);
    await page.locator("#object").fill(object);
    await page.locator("#obj-type").selectOption("Entity");
    await page.locator("#assert-btn").click();

    const assertSuccess = page.locator("#assert-status", { hasText: "✓" }).waitFor({ timeout: 30_000 });
    const assertFailure = page.locator("#assert-status", { hasText: "Error:" }).waitFor({ timeout: 30_000 });
    const assertOutcome = await Promise.race([
      assertSuccess.then(() => "success"),
      assertFailure.then(() => "failure"),
    ]);

    const assertStatus = ((await page.locator("#assert-status").textContent()) || "").trim();

    if (assertOutcome === "failure") {
      throw createFailure("assert flow failed", {
        url: targetUrl,
        subject,
        predicate,
        object,
        assertStatus,
        queryStatus: "",
        pageErrors,
        consoleErrors,
      });
    }

    await page.locator("#query-entity").fill(subject);
    await page.locator("#query-pred").fill(predicate);
    await page.locator("#query-btn").click();

    await page.locator("#query-status", { hasText: /result/i }).waitFor({ timeout: 30_000 });

    const row = page.locator("#stream-body .fact-row").first();
    await row.locator(".btn-invalidate").click();

    await page.locator("#query-status", { hasText: /retracted:/i }).waitFor({ timeout: 30_000 });

    const queryStatus = ((await page.locator("#query-status").textContent()) || "").trim();

    console.log(
      JSON.stringify(
        {
          ok: true,
          url: targetUrl,
          subject,
          predicate,
          object,
          assertStatus,
          queryStatus,
          pageErrors,
          consoleErrors,
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
  const details =
    error && typeof error === "object" && "ok" in error
      ? error
      : createFailure(error, {
          url: requireUrl(),
          assertStatus: "",
          queryStatus: "",
          pageErrors: [],
          consoleErrors: [],
        });
  console.log(JSON.stringify(details, null, 2));
  process.exit(1);
});
