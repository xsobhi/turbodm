// TurboDM browser integration: hand downloads to the TurboDM app (with the page's
// cookies, referrer and user-agent, so logged-in / protected links keep working).
// Works as a Firefox MV2 background page and a Chrome/Brave MV3 service worker.
"use strict";

const api = globalThis.browser ?? globalThis.chrome;
const HOST = "com.xsobhi.turbodm";
const DEFAULTS = { enabled: true, minSizeKB: 0, skipHosts: "" };

async function getSettings() {
  return { ...DEFAULTS, ...(await api.storage.local.get(DEFAULTS)) };
}

function hostOf(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return "";
  }
}

function isSkippedHost(url, settings) {
  const host = hostOf(url);
  return settings.skipHosts
    .split(/[\s,]+/)
    .filter(Boolean)
    .some((h) => host === h || host.endsWith("." + h));
}

function basename(path) {
  return (path || "").split(/[\\/]/).pop();
}

// Cookies for the URL as a "Cookie:" header value (the app sends it with every connection).
// storeId picks the private-window / container cookie jar in Firefox.
async function cookieHeader(url, storeId) {
  try {
    const cookies = await api.cookies.getAll(storeId ? { url, storeId } : { url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

function sendNative(message) {
  return api.runtime.sendNativeMessage(HOST, message);
}

async function handOff({ url, filename, referrer, fileSize, cookies, userAgent, storeId }) {
  const reply = await sendNative({
    type: "download",
    url,
    filename: filename || null,
    referrer: referrer || null,
    cookies: (cookies ?? (await cookieHeader(url, storeId))) || null,
    userAgent: userAgent || navigator.userAgent,
    fileSize: fileSize > 0 ? fileSize : null,
  });
  return Boolean(reply && reply.ok);
}

// Catch browser downloads: pause, hand to TurboDM, and only then cancel the browser's
// copy. If TurboDM can't be reached, the browser simply continues the download.
// (Firefox catches most downloads earlier, from the response headers: see intercept.js.)
api.downloads.onCreated.addListener(async (item) => {
  const url = item.finalUrl || item.url;
  const settings = await getSettings();
  if (!settings.enabled || !/^https?:/i.test(url) || item.state !== "in_progress") return;
  if (isSkippedHost(url, settings)) return;
  if (settings.minSizeKB > 0 && item.totalBytes > 0 && item.totalBytes < settings.minSizeKB * 1024) return;
  try {
    await api.downloads.pause(item.id);
  } catch {
    /* not pausable yet: we'll cancel it if the hand-off works */
  }
  let accepted = false;
  try {
    accepted = await handOff({
      url,
      filename: basename(item.filename),
      referrer: item.referrer,
      fileSize: item.totalBytes || item.fileSize,
      storeId: item.cookieStoreId,
    });
  } catch (err) {
    console.warn("TurboDM is not reachable, keeping the browser download:", err);
  }
  try {
    if (accepted) {
      await api.downloads.cancel(item.id);
      await api.downloads.erase({ id: item.id });
    } else {
      await api.downloads.resume(item.id);
    }
  } catch {
    /* the download may already be gone */
  }
});

function createMenus() {
  api.contextMenus.removeAll(() => {
    api.contextMenus.create({ id: "tdm-link", title: "Download with TurboDM", contexts: ["link"] });
    api.contextMenus.create({
      id: "tdm-media",
      title: "Download media with TurboDM",
      contexts: ["video", "audio", "image"],
    });
  });
}

api.runtime.onInstalled.addListener(createMenus);
createMenus(); // Firefox recreates menus each time the background page loads

api.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl;
  if (url && /^https?:/i.test(url)) {
    handOff({ url, referrer: info.pageUrl || (tab && tab.url) }).catch((err) =>
      console.warn("TurboDM is not reachable:", err),
    );
  }
});
