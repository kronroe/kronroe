/**
 * Kronroe analytics + cookie consent
 * ─────────────────────────────────────────────────────────────
 * Hand-built, zero-dependency consent orchestrator for kronroe.dev.
 *
 * Design principles:
 *   • Single orchestrator — one code path gates GA4 and LinkedIn.
 *   • Consent Mode v2 — GA4 loads with all signals denied by default.
 *   • Versioned consent record — policyVersion + recordedAt + expiresAt.
 *     Bump POLICY_VERSION to force re-prompt when tracking materially changes.
 *   • Withdraw = stop future, not unload — denying consent mid-session flips
 *     gtag to 'denied' (no more cookies, no identifiable hits).
 *   • WAI-ARIA dialog: focus trap, focus return, `inert` background when
 *     preferences modal is open.
 *   • No geolocation — banner shows for everyone (EEA/UK posture globally).
 *   • No cookies set before a choice is made (necessary cookies only).
 *
 * Public API:
 *   window.kronroeConsent.open()      → reopen preferences modal
 *   window.kronroeConsent.getRecord() → read current consent record
 *   window.kronroeConsent.reset()     → clear record + reshow banner
 */
(function () {
  'use strict';

  var GA4_ID = 'G-QC2EK11KHY';
  var LINKEDIN_PARTNER_ID = '8986058';
  var COOKIE_NAME = 'kronroe_consent';
  var POLICY_VERSION = 1;
  var SCHEMA_VERSION = 1;
  var EXPIRY_MONTHS = 12;

  function readRecord() {
    var match = document.cookie.match(
      new RegExp('(?:^|; )' + COOKIE_NAME + '=([^;]+)')
    );
    if (!match) return null;
    try {
      var parsed = JSON.parse(decodeURIComponent(match[1]));
      if (parsed.policyVersion !== POLICY_VERSION) return null;
      if (parsed.schemaVersion !== SCHEMA_VERSION) return null;
      if (parsed.expiresAt && Date.parse(parsed.expiresAt) < Date.now()) return null;
      return parsed;
    } catch (_e) {
      return null;
    }
  }

  function writeRecord(choices) {
    var now = new Date();
    var expires = new Date(now);
    expires.setMonth(expires.getMonth() + EXPIRY_MONTHS);
    var record = {
      schemaVersion: SCHEMA_VERSION,
      policyVersion: POLICY_VERSION,
      recordedAt: now.toISOString(),
      expiresAt: expires.toISOString(),
      choices: {
        necessary: true,
        analytics: !!choices.analytics,
        marketing: !!choices.marketing
      }
    };
    document.cookie =
      COOKIE_NAME + '=' + encodeURIComponent(JSON.stringify(record)) +
      '; path=/; expires=' + expires.toUTCString() +
      '; SameSite=Lax; Secure';
    return record;
  }

  function clearRecord() {
    document.cookie = COOKIE_NAME + '=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
  }

  window.dataLayer = window.dataLayer || [];
  function gtag() { window.dataLayer.push(arguments); }
  window.gtag = gtag;

  gtag('consent', 'default', {
    analytics_storage: 'denied',
    ad_storage: 'denied',
    ad_user_data: 'denied',
    ad_personalization: 'denied',
    wait_for_update: 500
  });

  var gtagScript = document.createElement('script');
  gtagScript.async = true;
  gtagScript.src = 'https://www.googletagmanager.com/gtag/js?id=' + GA4_ID;
  document.head.appendChild(gtagScript);

  gtag('js', new Date());
  gtag('config', GA4_ID, { send_page_view: true });

  var linkedInLoaded = false;
  function loadLinkedIn() {
    if (linkedInLoaded) return;
    linkedInLoaded = true;
    window._linkedin_data_partner_ids = window._linkedin_data_partner_ids || [];
    window._linkedin_data_partner_ids.push(LINKEDIN_PARTNER_ID);
    var s = document.createElement('script');
    s.async = true;
    s.src = 'https://snap.licdn.com/li.lms-analytics/insight.min.js';
    document.head.appendChild(s);
  }

  function applyChoices(choices) {
    gtag('consent', 'update', {
      analytics_storage: choices.analytics ? 'granted' : 'denied',
      ad_storage: choices.marketing ? 'granted' : 'denied',
      ad_user_data: choices.marketing ? 'granted' : 'denied',
      ad_personalization: choices.marketing ? 'granted' : 'denied'
    });
    if (choices.marketing) loadLinkedIn();
  }

  var STYLES = [
    '#kr-consent-root *{box-sizing:border-box;font-family:"Plus Jakarta Sans",system-ui,sans-serif}',
    '#kr-consent-banner{position:fixed;left:0;right:0;bottom:0;z-index:9999;background:#FBFAFF;border-top:1px solid #DDD9E8;box-shadow:0 -4px 24px rgba(45,41,64,.08);padding:1.25rem 1.5rem;display:flex;gap:1.5rem;align-items:center;flex-wrap:wrap;animation:krSlide .25s ease-out}',
    '@keyframes krSlide{from{transform:translateY(100%)}to{transform:translateY(0)}}',
    '#kr-consent-banner::before{content:"";position:absolute;top:0;left:0;right:0;height:3px;background:linear-gradient(90deg,#7C5CFC 0 25%,#E87D4A 25% 50%,#3EC9C9 50% 75%,#8BBF20 75% 100%)}',
    '#kr-consent-banner .kr-text{flex:1 1 320px;min-width:0}',
    '#kr-consent-banner .kr-title{font-size:.95rem;font-weight:600;color:#2D2940;margin:0 0 .25rem}',
    '#kr-consent-banner .kr-body{font-size:.85rem;line-height:1.5;color:#4A4559;margin:0}',
    '#kr-consent-banner .kr-body a{color:#7C5CFC;text-decoration:underline}',
    '#kr-consent-banner .kr-btns{display:flex;gap:.5rem;flex-wrap:wrap}',
    '#kr-consent-root button{font-family:inherit;font-size:.85rem;font-weight:600;padding:.6rem 1rem;border-radius:8px;cursor:pointer;transition:background .15s,color .15s;border:1.5px solid transparent}',
    '#kr-consent-root button.kr-primary{background:#7C5CFC;color:#fff;border-color:#7C5CFC}',
    '#kr-consent-root button.kr-primary:hover{background:#6344E0;border-color:#6344E0}',
    '#kr-consent-root button.kr-secondary{background:transparent;color:#4A4559;border-color:#C4BFD1}',
    '#kr-consent-root button.kr-secondary:hover{background:#F4F2FA;color:#2D2940}',
    '#kr-consent-root button:focus-visible{outline:2px solid #7C5CFC;outline-offset:2px}',
    '#kr-consent-backdrop{position:fixed;inset:0;z-index:10000;background:rgba(25,23,38,.4);display:flex;align-items:center;justify-content:center;padding:1rem;animation:krFade .2s ease-out}',
    '@keyframes krFade{from{opacity:0}to{opacity:1}}',
    '#kr-consent-modal{background:#FBFAFF;border-radius:14px;max-width:540px;width:100%;max-height:calc(100vh - 2rem);overflow-y:auto;box-shadow:0 12px 40px rgba(45,41,64,.24);position:relative}',
    '#kr-consent-modal::before{content:"";position:absolute;top:0;left:0;right:0;height:4px;background:linear-gradient(90deg,#7C5CFC 0 25%,#E87D4A 25% 50%,#3EC9C9 50% 75%,#8BBF20 75% 100%);border-radius:14px 14px 0 0}',
    '#kr-consent-modal .kr-head{padding:1.75rem 1.75rem 1rem}',
    '#kr-consent-modal h2{font-size:1.15rem;font-weight:700;color:#2D2940;margin:0 0 .5rem}',
    '#kr-consent-modal .kr-lede{font-size:.88rem;line-height:1.55;color:#4A4559;margin:0}',
    '#kr-consent-modal .kr-cats{padding:0 1.75rem;display:flex;flex-direction:column;gap:.75rem}',
    '#kr-consent-modal .kr-cat{background:#F4F2FA;border:1px solid #DDD9E8;border-radius:10px;padding:1rem 1.15rem}',
    '#kr-consent-modal .kr-cat-head{display:flex;justify-content:space-between;align-items:flex-start;gap:1rem}',
    '#kr-consent-modal .kr-cat-title{font-size:.9rem;font-weight:600;color:#2D2940;margin:0}',
    '#kr-consent-modal .kr-cat-desc{font-size:.8rem;line-height:1.5;color:#6E6980;margin:.35rem 0 0}',
    '#kr-consent-modal .kr-toggle{position:relative;width:40px;height:22px;flex-shrink:0;margin-top:2px}',
    '#kr-consent-modal .kr-toggle input{opacity:0;width:0;height:0;position:absolute}',
    '#kr-consent-modal .kr-toggle .kr-slider{position:absolute;inset:0;background:#C4BFD1;border-radius:22px;transition:background .15s;cursor:pointer}',
    '#kr-consent-modal .kr-toggle .kr-slider::after{content:"";position:absolute;left:2px;top:2px;width:18px;height:18px;background:#fff;border-radius:50%;transition:transform .15s;box-shadow:0 1px 3px rgba(0,0,0,.15)}',
    '#kr-consent-modal .kr-toggle input:checked+.kr-slider{background:#7C5CFC}',
    '#kr-consent-modal .kr-toggle input:checked+.kr-slider::after{transform:translateX(18px)}',
    '#kr-consent-modal .kr-toggle input:disabled+.kr-slider{background:#A895FD;cursor:not-allowed}',
    '#kr-consent-modal .kr-toggle input:focus-visible+.kr-slider{outline:2px solid #7C5CFC;outline-offset:2px}',
    '#kr-consent-modal .kr-details{margin-top:.75rem;border-top:1px dashed #DDD9E8;padding-top:.75rem;font-family:"JetBrains Mono",ui-monospace,monospace;font-size:.72rem;color:#6E6980;line-height:1.5;word-break:break-word}',
    '#kr-consent-modal .kr-details strong{color:#4A4559;font-weight:600}',
    '#kr-consent-modal .kr-foot{padding:1.25rem 1.75rem 1.75rem;display:flex;gap:.5rem;flex-wrap:wrap;justify-content:flex-end;border-top:1px solid #DDD9E8;margin-top:1rem}',
    // Mobile: stack the banner vertically and override `.kr-text`'s
    // `flex: 1 1 320px` (which is correct for row layout but in column
    // flex turns the 320px into a forced *height* — was producing a
    // huge empty gap between the body text and the buttons, leaving
    // the banner at ~440px = 54% of viewport on a 375x812 phone).
    // Padding also tightened so the banner stays under ~25% of viewport
    // on the smallest phones.
    '@media (max-width:520px){'
      + '#kr-consent-banner{flex-direction:column;align-items:stretch;padding:.95rem 1.15rem;gap:.75rem}'
      + '#kr-consent-banner .kr-text{flex:0 0 auto}'
      + '#kr-consent-banner .kr-body{font-size:.82rem;line-height:1.45}'
      + '#kr-consent-banner .kr-btns{justify-content:stretch}'
      + '#kr-consent-banner .kr-btns button{flex:1;font-size:.82rem;padding:.55rem .6rem}'
      + '}'
  ].join('');

  function injectStyles() {
    if (document.getElementById('kr-consent-styles')) return;
    var s = document.createElement('style');
    s.id = 'kr-consent-styles';
    s.textContent = STYLES;
    document.head.appendChild(s);
  }

  function ensureRoot() {
    var r = document.getElementById('kr-consent-root');
    if (r) return r;
    r = document.createElement('div');
    r.id = 'kr-consent-root';
    document.body.appendChild(r);
    return r;
  }

  var bannerEl = null;
  var lastFocusBeforeModal = null;

  function showBanner() {
    if (bannerEl) return;
    injectStyles();
    var root = ensureRoot();
    bannerEl = document.createElement('div');
    bannerEl.id = 'kr-consent-banner';
    bannerEl.setAttribute('role', 'region');
    bannerEl.setAttribute('aria-label', 'Cookie consent');
    bannerEl.innerHTML =
      '<div class="kr-text">' +
        '<p class="kr-title">We use cookies</p>' +
        '<p class="kr-body">We use cookies to understand how you found us and improve your experience. You can accept all, reject all, or <a href="#" data-kr="customise">customise</a>.</p>' +
      '</div>' +
      '<div class="kr-btns">' +
        '<button type="button" class="kr-secondary" data-kr="reject">Reject all</button>' +
        '<button type="button" class="kr-secondary" data-kr="customise">Customise</button>' +
        '<button type="button" class="kr-primary" data-kr="accept">Accept all</button>' +
      '</div>';
    root.appendChild(bannerEl);

    bannerEl.addEventListener('click', function (e) {
      var action = e.target.getAttribute && e.target.getAttribute('data-kr');
      if (!action) return;
      e.preventDefault();
      if (action === 'accept') commit({ analytics: true, marketing: true });
      else if (action === 'reject') commit({ analytics: false, marketing: false });
      else if (action === 'customise') openModal();
    });
  }

  function hideBanner() {
    if (!bannerEl) return;
    bannerEl.remove();
    bannerEl = null;
  }

  var modalEl = null;
  var keyHandler = null;

  function setInert(flag) {
    ['main', 'header', 'footer'].forEach(function (sel) {
      document.querySelectorAll(sel).forEach(function (el) {
        if (flag) el.setAttribute('inert', '');
        else el.removeAttribute('inert');
      });
    });
  }

  function openModal() {
    injectStyles();
    if (modalEl) return;
    lastFocusBeforeModal = document.activeElement;
    var existing = readRecord();
    var prefill = existing ? existing.choices : { analytics: false, marketing: false };

    var root = ensureRoot();
    modalEl = document.createElement('div');
    modalEl.id = 'kr-consent-backdrop';
    modalEl.innerHTML =
      '<div id="kr-consent-modal" role="dialog" aria-modal="true" aria-labelledby="kr-consent-h" tabindex="-1">' +
        '<div class="kr-head">' +
          '<h2 id="kr-consent-h">Cookie preferences</h2>' +
          '<p class="kr-lede">Kronroe is a bi-temporal database — it tracks <em>when facts are true</em> and <em>when we recorded them</em>. Your consent choices are recorded the same way. Change them any time from the footer.</p>' +
        '</div>' +
        '<div class="kr-cats">' +
          buildCat('necessary', 'Necessary', 'Essential cookies for site functionality. These cannot be disabled.', true, true) +
          buildCat('analytics', 'Analytics', 'Google Analytics 4 helps us understand traffic sources, popular pages, and how visitors use the site. No personal data is shared with third parties.', prefill.analytics, false) +
          buildCat('marketing', 'Marketing', 'LinkedIn Insight Tag provides anonymous professional demographic data (job titles, industries) about visitors, helping us understand our audience.', prefill.marketing, false) +
          buildDetails(existing) +
        '</div>' +
        '<div class="kr-foot">' +
          '<button type="button" class="kr-secondary" data-kr="reject">Reject all</button>' +
          '<button type="button" class="kr-secondary" data-kr="save">Save preferences</button>' +
          '<button type="button" class="kr-primary" data-kr="accept">Accept all</button>' +
        '</div>' +
      '</div>';
    root.appendChild(modalEl);

    modalEl.addEventListener('click', function (e) {
      if (e.target === modalEl) { closeModal(); return; }
      var action = e.target.getAttribute && e.target.getAttribute('data-kr');
      if (!action) return;
      if (action === 'accept') commit({ analytics: true, marketing: true });
      else if (action === 'reject') commit({ analytics: false, marketing: false });
      else if (action === 'save') {
        var a = modalEl.querySelector('input[data-cat="analytics"]').checked;
        var m = modalEl.querySelector('input[data-cat="marketing"]').checked;
        commit({ analytics: a, marketing: m });
      }
    });

    setInert(true);

    var modalInner = modalEl.querySelector('#kr-consent-modal');
    modalInner.focus();
    keyHandler = function (e) {
      if (e.key === 'Escape') { e.preventDefault(); closeModal(); return; }
      if (e.key !== 'Tab') return;
      var focusables = modalEl.querySelectorAll('button, input:not([disabled]), [tabindex="0"]');
      if (!focusables.length) return;
      var first = focusables[0];
      var last = focusables[focusables.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    };
    document.addEventListener('keydown', keyHandler);
  }

  function closeModal() {
    if (!modalEl) return;
    modalEl.remove();
    modalEl = null;
    setInert(false);
    if (keyHandler) {
      document.removeEventListener('keydown', keyHandler);
      keyHandler = null;
    }
    if (lastFocusBeforeModal && lastFocusBeforeModal.focus) {
      lastFocusBeforeModal.focus();
    }
  }

  function buildCat(id, title, desc, checked, readOnly) {
    return '<div class="kr-cat">' +
      '<div class="kr-cat-head">' +
        '<div><p class="kr-cat-title">' + title + '</p><p class="kr-cat-desc">' + desc + '</p></div>' +
        '<label class="kr-toggle"><input type="checkbox" data-cat="' + id + '"' +
          (checked ? ' checked' : '') + (readOnly ? ' disabled' : '') +
          ' aria-label="' + title + '"><span class="kr-slider"></span></label>' +
      '</div>' +
    '</div>';
  }

  function buildDetails(record) {
    if (!record) {
      return '<div class="kr-cat"><div class="kr-details">' +
        '<strong>Consent record:</strong> none yet — your choice will be stored with a timestamp and a 12-month expiry.' +
      '</div></div>';
    }
    return '<div class="kr-cat"><div class="kr-details">' +
      '<strong>Recorded at:</strong> ' + record.recordedAt + '<br>' +
      '<strong>Expires at:</strong> ' + record.expiresAt + '<br>' +
      '<strong>Policy version:</strong> ' + record.policyVersion +
    '</div></div>';
  }

  function commit(choices) {
    var record = writeRecord(choices);
    applyChoices(record.choices);
    hideBanner();
    closeModal();
  }

  window.kronroeConsent = {
    open: function () {
      if (!document.getElementById('kr-consent-styles')) injectStyles();
      openModal();
    },
    getRecord: readRecord,
    reset: function () {
      clearRecord();
      gtag('consent', 'update', {
        analytics_storage: 'denied',
        ad_storage: 'denied',
        ad_user_data: 'denied',
        ad_personalization: 'denied'
      });
      showBanner();
    }
  };

  function init() {
    var record = readRecord();
    if (record) applyChoices(record.choices);
    else showBanner();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
