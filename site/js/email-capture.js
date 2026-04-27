/**
 * Kronroe email capture
 * ─────────────────────────────────────────────────────────────
 * Hand-rolled, zero-dependency newsletter signup form handler.
 *
 * How it works:
 *   1. Each form on the page with `data-kr-subscribe` is enhanced.
 *   2. Submission is intercepted; the email is POSTed to the form's
 *      `action` URL as application/x-www-form-urlencoded.
 *   3. UI feedback (success / error / loading) is shown inline.
 *   4. On success, a `generate_lead` event is pushed to GA4's dataLayer
 *      (only fires if the user accepted analytics consent — gtag handles
 *      that gating automatically).
 *
 * Why no library: we already proved (with the consent banner) that we
 * don't need 25 KB of dep to handle a textbox + a fetch. This adds 80
 * lines for a feature we'll touch on every blog post.
 *
 * Switching providers (Buttondown → Kit → Listmonk → custom):
 *   Just change the `action="..."` attribute on the <form>. The body
 *   shape (email=...) is the same across all major providers.
 *
 * Buttondown setup reference:
 *   action="https://buttondown.com/api/emails/embed-subscribe/<USERNAME>"
 *   <input name="email">
 *
 * Kit (ConvertKit) setup reference:
 *   action="https://app.kit.com/forms/<FORM_ID>/subscriptions"
 *   <input name="email_address">
 */
(function () {
  'use strict';

  function enhanceForm(form) {
    if (form.dataset.krEnhanced === '1') return;
    form.dataset.krEnhanced = '1';

    var input = form.querySelector('input[type="email"]');
    var statusEl = form.querySelector('[data-kr-status]');
    var submitBtn = form.querySelector('button[type="submit"]');

    if (!input || !submitBtn) {
      console.warn('[kronroe] subscribe form missing email input or submit button');
      return;
    }

    form.addEventListener('submit', function (e) {
      e.preventDefault();
      var email = (input.value || '').trim();
      if (!email) return;

      submitBtn.disabled = true;
      submitBtn.textContent = 'Subscribing…';
      if (statusEl) {
        statusEl.textContent = '';
        statusEl.removeAttribute('data-state');
      }

      var body = new URLSearchParams();
      // Form body shape is provider-specific; we send under multiple
      // common keys so a single config works across Buttondown, Kit, etc.
      body.set('email', email);
      body.set('email_address', email);

      fetch(form.action, {
        method: 'POST',
        mode: 'no-cors', // most providers don't return CORS headers; success is implicit
        headers: {
          'Content-Type': 'application/x-www-form-urlencoded',
        },
        body: body.toString(),
      })
        .then(function () {
          submitBtn.disabled = false;
          submitBtn.textContent = 'Subscribed ✓';
          input.value = '';
          if (statusEl) {
            statusEl.textContent = 'Thanks — check your inbox to confirm.';
            statusEl.setAttribute('data-state', 'success');
          }

          // GA4 conversion event — gated by consent automatically.
          if (typeof window.gtag === 'function') {
            window.gtag('event', 'generate_lead', {
              method: 'newsletter',
              currency: 'USD',
              value: 0,
            });
          }

          // Reset button label after a moment so the form feels reusable.
          setTimeout(function () {
            submitBtn.textContent = submitBtn.dataset.label || 'Subscribe';
          }, 4000);
        })
        .catch(function (_err) {
          submitBtn.disabled = false;
          submitBtn.textContent = submitBtn.dataset.label || 'Subscribe';
          if (statusEl) {
            statusEl.textContent =
              "Something went wrong — please email rebekah@kindlyroe.com instead.";
            statusEl.setAttribute('data-state', 'error');
          }
        });
    });

    // Cache original button label for the post-submit reset.
    submitBtn.dataset.label = submitBtn.textContent.trim();
  }

  function init() {
    document.querySelectorAll('form[data-kr-subscribe]').forEach(enhanceForm);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
