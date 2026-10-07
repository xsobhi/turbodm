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

// "/home/me/Pictures/cat.jpg" → "/home/me/Pictures" (null unless it's a full path)
function dirname(path) {
  const cut = (path || "").lastIndexOf("/");
  return path && path.startsWith("/") && cut > 0 ? path.slice(0, cut) : null;
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

async function handOff({ url, filename, directory, referrer, fileSize, cookies, userAgent, storeId }) {
  const reply = await sendNative({
    type: "download",
    url,
    filename: filename || null,
    directory: directory || null,
    referrer: referrer || null,
    cookies: (cookies ?? (await cookieHeader(url, storeId))) || null,
    userAgent: userAgent || navigator.userAgent,
    fileSize: fileSize > 0 ? fileSize : null,
  });
  return Boolean(reply && reply.ok);
}

function wanted(url, size, settings) {
  if (!settings.enabled || !/^https?:/i.test(url) || isSkippedHost(url, settings)) return false;
  return !(settings.minSizeKB > 0 && size > 0 && size < settings.minSizeKB * 1024);
}

async function discard(id) {
  try {
    await api.downloads.cancel(id);
    await api.downloads.erase({ id });
  } catch {
    /* the download may already be gone */
  }
}

// Chrome, Edge, Brave: decide while the browser is still choosing the file name, before it
// shows its "Save as" window; if TurboDM takes the download, that window never appears.
const BEFORE_SAVE_AS = Boolean(api.downloads.onDeterminingFilename);
if (BEFORE_SAVE_AS) {
  api.downloads.onDeterminingFilename.addListener((item, suggest) => {
    const url = item.finalUrl || item.url;
    if (!/^https?:/i.test(url)) return; // blob:, data:: the browser's own business
    (async () => {
      let accepted = false;
      try {
        if (wanted(url, item.totalBytes || item.fileSize, await getSettings())) {
          accepted = await handOff({ url, filename: basename(item.filename), referrer: item.referrer,
                                     fileSize: item.totalBytes || item.fileSize });
        }
      } catch (err) {
        console.warn("TurboDM is not reachable, keeping the browser download:", err);
      }
      if (accepted) await discard(item.id);
      else suggest(); // carry on as usual
    })();
    return true; // answering asynchronously
  });
}

// Firefox: pause, hand to TurboDM, and only then cancel the browser's copy. If TurboDM can't
// be reached, the browser simply continues. (Firefox catches most downloads earlier, from the
// response headers: see intercept.js.)
api.downloads.onCreated.addListener(async (item) => {
  if (BEFORE_SAVE_AS) return;
  const url = item.finalUrl || item.url;
  if (item.state !== "in_progress" || !wanted(url, item.totalBytes, await getSettings())) return;
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
      // the folder picked in the browser's "Save As" dialog (or its own download folder)
      directory: dirname(item.filename),
      referrer: item.referrer,
      fileSize: item.totalBytes || item.fileSize,
      storeId: item.cookieStoreId,
    });
  } catch (err) {
    console.warn("TurboDM is not reachable, keeping the browser download:", err);
  }
  if (accepted) {
    await discard(item.id);
  } else {
    api.downloads.resume(item.id).catch(() => {});
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
