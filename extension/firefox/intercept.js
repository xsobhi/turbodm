// Firefox only: recognise downloads from the server's response headers and hand them to
// TurboDM *before* Firefox starts saving anything — no partial copy, no entry in Firefox's
// download list. Uses the exact cookies, referrer and user-agent Firefox sent with the
// request (right cookie jar for private windows and containers). Anything missed here is
// still caught by downloads.onCreated in background.js.
"use strict";

const FILTER = { urls: ["http://*/*", "https://*/*"], types: ["main_frame", "sub_frame"] };
const sentHeaders = new Map(); // requestId → headers Firefox sent (latest hop of redirects)

// Types Firefox shows or plays itself rather than downloading
const SHOWN_INLINE = new RegExp(
  "^(text/|multipart/|image/(png|jpe?g|gif|webp|avif|apng|bmp|svg\\+xml|x-icon|vnd\\.microsoft\\.icon)$" +
    "|(audio|video)/(mp4|webm|ogg|mpeg|wave?|x-wav|flac|aac|x-m4a)$" +
    "|application/(xhtml\\+xml|xml|json|pdf|javascript|x-javascript|ecmascript|[\\w.-]+\\+(json|xml))$)",
);
// Types Firefox must handle itself (add-on and certificate installs)
const BROWSER_ONLY = /^application\/(x-xpinstall|x-x509-[\w-]+|pkix-cert|x-pkcs7-[\w-]+)$/;

function headerOf(headers, name) {
  const found = (headers || []).find((h) => h.name.toLowerCase() === name);
  return found ? found.value : "";
}

// "inline" isn't enough: servers (Moodle, for one) mark Word files and the like inline too,
// and Firefox downloads what it can't show anyway. Only what it really shows stays in the tab.
function isDownload(disposition, contentType) {
  const type = contentType.split(";")[0].trim().toLowerCase();
  if (BROWSER_ONLY.test(type)) return false;
  if (/^\s*attachment/i.test(disposition)) return true;
  if (!type) return false;
  return !SHOWN_INLINE.test(type);
}

// filename*=UTF-8''name%20x.zip (RFC 6266) or filename="name.zip"
function dispositionFilename(disposition) {
  const extended = /filename\*\s*=\s*[\w-]+'[^']*'([^;]+)/i.exec(disposition);
  if (extended) {
    try {
      return decodeURIComponent(extended[1].trim().replace(/^"|"$/g, ""));
    } catch {
      /* malformed: fall through to the plain form */
    }
  }
  const plain = /filename\s*=\s*("([^"]*)"|[^;]+)/i.exec(disposition);
  return plain ? (plain[2] ?? plain[1]).trim() : null;
}

// A tab opened only for this download (target=_blank link, middle-click) would stay blank:
// close it, as Firefox does with its own downloads.
async function closeIfBlank(tabId) {
  try {
    const tab = await browser.tabs.get(tabId);
    if (tab.url === "about:blank") await browser.tabs.remove(tabId);
  } catch {
    /* already closed */
  }
}

browser.webRequest.onSendHeaders.addListener(
  (d) => {
    const h = d.requestHeaders;
    sentHeaders.set(d.requestId, {
      cookies: headerOf(h, "cookie"),
      referrer: headerOf(h, "referer"),
      userAgent: headerOf(h, "user-agent"),
    });
  },
  FILTER,
  ["requestHeaders"],
);

const forget = (d) => sentHeaders.delete(d.requestId);
browser.webRequest.onCompleted.addListener(forget, FILTER);
browser.webRequest.onErrorOccurred.addListener(forget, FILTER);

browser.webRequest.onHeadersReceived.addListener(
  async (d) => {
    if (d.method !== "GET" || d.statusCode !== 200) return {};
    const disposition = headerOf(d.responseHeaders, "content-disposition");
    if (!isDownload(disposition, headerOf(d.responseHeaders, "content-type"))) return {};
    const settings = await getSettings();
    const size = parseInt(headerOf(d.responseHeaders, "content-length"), 10) || 0;
    if (!settings.enabled || isSkippedHost(d.url, settings)) return {};
    if (settings.minSizeKB > 0 && size > 0 && size < settings.minSizeKB * 1024) return {};
    const sent = sentHeaders.get(d.requestId) || {};
    try {
      const accepted = await handOff({
        url: d.url,
        filename: dispositionFilename(disposition),
        referrer: sent.referrer || d.originUrl,
        cookies: sent.cookies,
        userAgent: sent.userAgent,
        fileSize: size,
        storeId: d.cookieStoreId,
      });
      if (accepted) {
        if (d.type === "main_frame" && d.tabId >= 0) setTimeout(() => closeIfBlank(d.tabId), 100);
        return { cancel: true };
      }
    } catch (err) {
      console.warn("TurboDM is not reachable, leaving the download to Firefox:", err);
    }
    return {};
  },
  FILTER,
  ["blocking", "responseHeaders"],
);
