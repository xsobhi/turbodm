// TurboDM browser integration: hand downloads to the TurboDM app (with the page's
// cookies, referrer and user-agent, so logged-in / protected links keep working).
// Works as a Firefox MV2 background page and a Chrome/Brave MV3 service worker.
"use strict";

const api = globalThis.browser ?? globalThis.chrome;
const HOST = "com.xsobhi.turbodm";
const DEFAULTS = { enabled: true, minSizeKB: 0, skipHosts: "", sortFolders: true };

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

// A video on a web page (YouTube and the many sites TurboDM's yt-dlp knows): TurboDM looks at the
// page and offers its qualities. An older TurboDM doesn't know this message ("bad message"): the
// button says so instead of failing silently.
async function handOffVideo({ url, referrer, storeId, choice }) {
  const reply = await sendNative({
    type: "video",
    url,
    choice: choice || null, // the quality picked in the page's menu (videomenu.js); else TurboDM asks
    referrer: referrer || null,
    cookies: (await cookieHeader(url, storeId)) || null,
    userAgent: navigator.userAgent,
  });
  if (reply && reply.ok) return true;
  if (/bad message/.test((reply && reply.error) || "")) {
    const button = api.action ?? api.browserAction;
    button.setBadgeText({ text: "!" });
    button.setTitle({ title: "Update TurboDM to download videos (this version can't yet)" });
  }
  return false;
}

// What "Download video with TurboDM" means where it was clicked: a link to a video's page, the
// embedded player a video plays in, or the page itself.
function videoPage(info, tab) {
  return info.linkUrl || info.frameUrl || info.pageUrl || (tab && tab.url);
}

// Where a download came from: the browser's referrer, or else the page in front (the one the
// user clicked the link on), for TurboDM's "Refresh download address" to open later.
async function pageOf(item) {
  if (item.referrer) return item.referrer;
  try {
    const [tab] = await api.tabs.query({ active: true, lastFocusedWindow: true });
    return tab && /^https?:/i.test(tab.url || "") ? tab.url : null;
  } catch {
    return null;
  }
}

// The folders TurboDM sorts its downloads into (src/categories.rs). What the browser saves itself
// (files a page makes, like WhatsApp Web's; or when TurboDM can't be reached) goes into the same
// ones, inside the browser's download folder: Chrome, Edge and Brave only (popup: sortFolders).
const CATEGORIES = {
  Compressed: ["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "lz", "iso", "img"],
  Documents: ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "txt", "rtf", "epub", "csv",
              "md", "djvu"],
  Music: ["mp3", "flac", "wav", "aac", "ogg", "opus", "m4a", "wma", "alac", "aiff"],
  Video: ["mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "ts", "mpg", "mpeg", "3gp"],
  Programs: ["deb", "rpm", "appimage", "flatpakref", "snap", "exe", "msi", "sh", "run", "apk", "dmg", "pkg", "jar"],
  Images: ["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "tif", "tiff", "psd"],
};

// "Images/cat.png" for a file the browser keeps, or nothing (no such folder for it, or turned off)
function sortedName(filename, settings) {
  const name = basename(filename);
  const ext = (name.match(/\.([^.]+)$/) || [])[1];
  const folder = ext && Object.keys(CATEGORIES).find((c) => CATEGORIES[c].includes(ext.toLowerCase()));
  return settings.sortFolders && folder ? { filename: `${folder}/${name}`, conflictAction: "uniquify" } : null;
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
    (async () => {
      let accepted = false;
      const settings = await getSettings();
      const rest = await resting(); // (the full extension decides, and sorts)
      try {
        // (blob:, data:: made by the page, not a link TurboDM can download: the browser's to save)
        if (/^https?:/i.test(url) && !rest && wanted(url, item.totalBytes || item.fileSize, settings)) {
          accepted = await handOff({ url, filename: basename(item.filename), referrer: await pageOf(item),
                                     fileSize: item.totalBytes || item.fileSize });
        }
      } catch (err) {
        console.warn("TurboDM is not reachable, keeping the browser download:", err);
      }
      if (accepted) {
        await discard(item.id);
        return;
      }
      // the browser keeps it: into the folder for its kind, as TurboDM would, or as it is
      const sorted = rest ? null : sortedName(item.filename, settings);
      if (sorted) suggest(sorted);
      else suggest();
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
      referrer: await pageOf(item),
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

// The copy installed from the Chrome Web Store, whose rules don't allow downloading YouTube's
// videos: no video button there (its manifest leaves YouTube out), no "Download video" on its
// pages, and its videos not sent. (TurboDM itself still takes them, from Download video….)
const STORE_ID = "enaaoegoakgmcdbhepcoflfmpeodipob";
const STORE_COPY = api.runtime.id === STORE_ID;
const YOUTUBE = /^https?:\/\/([^/]*\.)?(youtube\.com|youtube-nocookie\.com|youtu\.be)(\/|$)/i;

function storeLeavesOut(url) {
  return STORE_COPY && YOUTUBE.test(url || "");
}

// (the store copy: "Download video with TurboDM" hidden on YouTube's pages)
function showVideoMenuFor(url) {
  if (STORE_COPY) api.contextMenus.update("tdm-video", { visible: !storeLeavesOut(url) }, () => void api.runtime.lastError);
}

if (STORE_COPY) {
  api.tabs.onActivated.addListener(({ tabId }) => {
    resting();
    api.tabs.get(tabId).then((tab) => showVideoMenuFor(tab.url), () => {});
  });
  api.tabs.onUpdated.addListener((_, change, tab) => change.url !== undefined && tab.active && showVideoMenuFor(change.url));
}

// TurboDM's full extension, the one added by hand (it has YouTube; TurboDM's "YouTube in your
// browser" window shows how). Where it's on too, the store's copy rests: no downloads sent twice,
// no second button on videos, no second menu.
const FULL_ID = "edlkglikdjabdhlailjmdopnlocdegbe";
let full = { at: 0, here: false };

async function resting() {
  if (!STORE_COPY) return false;
  if (Date.now() - full.at < 10000) return full.here;
  let here = false;
  try {
    here = Boolean((await api.runtime.sendMessage(FULL_ID, { type: "turbodm-full?" }))?.full);
  } catch {
    /* not added, or turned off */
  }
  const changed = full.at > 0 && here !== full.here;
  full = { at: Date.now(), here };
  if (changed) createMenus();
  return here;
}

if (!STORE_COPY && api.runtime.onMessageExternal) {
  api.runtime.onMessageExternal.addListener((message, sender, reply) => {
    if (message && message.type === "turbodm-full?" && sender.id === STORE_ID) reply({ full: true });
  });
}

function createMenus() {
  api.contextMenus.removeAll(async () => {
    if (await resting()) return; // (the full extension has them)
    api.contextMenus.create({ id: "tdm-link", title: "Download with TurboDM", contexts: ["link"] });
    api.contextMenus.create({
      id: "tdm-media",
      title: "Download media with TurboDM",
      contexts: ["video", "audio", "image"],
    });
    api.contextMenus.create({
      id: "tdm-video",
      title: "Download video with TurboDM",
      contexts: ["page", "frame", "link", "video"],
    });
  });
}

api.runtime.onInstalled.addListener(createMenus);
createMenus(); // Firefox recreates menus each time the background page loads

api.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === "tdm-video") {
    const url = videoPage(info, tab);
    if (url && /^https?:/i.test(url) && !storeLeavesOut(url)) {
      handOffVideo({ url, referrer: info.pageUrl, storeId: tab && tab.cookieStoreId }).catch((err) =>
        console.warn("TurboDM is not reachable:", err),
      );
    }
    return;
  }
  const url = info.linkUrl || info.srcUrl;
  // magnet links too: TurboDM downloads torrents
  if (url && /^(https?|magnet):/i.test(url)) {
    handOff({ url, referrer: info.pageUrl || (tab && tab.url) }).catch((err) =>
      console.warn("TurboDM is not reachable:", err),
    );
  }
});

// The popup's "Download video on this page" (the popup can't wait for an answer once it closes),
// and the "Download this video" button on videos (videobutton.js).
// A plain video file playing on a page ("media-file") goes as an ordinary download.
// "video-peek": a video playing, for a running TurboDM to look at early (it isn't started for it).
// "video-qualities": what the video button's menu offers, as TurboDM says.
api.runtime.onMessage.addListener((message, sender, reply) => {
  if (message && message.type === "resting?") {
    resting().then((rest) => reply({ resting: rest }));
    return true; // answering asynchronously
  }
  if (message && ["video", "video-qualities", "video-peek"].includes(message.type) && storeLeavesOut(message.url)) {
    reply({ ok: false, error: "Videos from YouTube can't be downloaded with this browser's store add-on" });
    return false;
  }
  if (message && message.type === "video-qualities") {
    const storeId = sender.tab && sender.tab.cookieStoreId;
    cookieHeader(message.url, storeId)
      .then((cookies) => sendNative({ type: "videoQualities", url: message.url, referrer: message.referrer || null,
                                      cookies: cookies || null, userAgent: navigator.userAgent }))
      .then((answer) => reply(answer || { ok: false }))
      .catch((err) => reply({ ok: false, error: String(err) }));
    return true; // answering asynchronously
  }
  if (message && message.type === "video-peek") {
    const storeId = sender.tab && sender.tab.cookieStoreId;
    cookieHeader(message.url, storeId).then((cookies) =>
      sendNative({ type: "videoPeek", url: message.url, referrer: message.referrer || null,
                   cookies: cookies || null, userAgent: navigator.userAgent }).catch(() => {}),
    );
    return false;
  }
  if (!message || !["video", "media-file"].includes(message.type)) return false;
  const storeId = message.storeId ?? (sender.tab && sender.tab.cookieStoreId);
  const hand = message.type === "video" ? handOffVideo : handOff;
  hand({ url: message.url, referrer: message.referrer, storeId, choice: message.choice })
    .then((ok) => reply({ ok }))
    .catch((err) => reply({ ok: false, error: String(err) }));
  return true; // answering asynchronously
});
