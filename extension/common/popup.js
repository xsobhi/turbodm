"use strict";

const api = globalThis.browser ?? globalThis.chrome;
const HOST = "com.xsobhi.turbodm";
const DEFAULTS = { enabled: true, videoButton: true, minSizeKB: 0, skipHosts: "", sortFolders: true };
const $ = (id) => document.getElementById(id);

async function showStatus() {
  const status = $("status");
  try {
    const reply = await api.runtime.sendNativeMessage(HOST, { type: "ping" });
    status.textContent = reply.running
      ? `TurboDM ${reply.version} is running`
      : "Ready — TurboDM starts when a download arrives";
    status.className = "status ok";
  } catch (err) {
    status.textContent = "TurboDM app not found — run install.sh from the TurboDM folder";
    status.className = "status error";
  }
}

async function load() {
  const settings = { ...DEFAULTS, ...(await api.storage.local.get(DEFAULTS)) };
  $("enabled").checked = settings.enabled;
  $("videoButton").checked = settings.videoButton;
  $("sortFolders").checked = settings.sortFolders;
  // (only Chrome, Edge and Brave let it choose where the browser saves a file)
  $("sortRow").hidden = !(api.downloads && api.downloads.onDeterminingFilename);
  $("minSizeKB").value = settings.minSizeKB;
  $("skipHosts").value = settings.skipHosts;
}

function save() {
  api.storage.local.set({
    enabled: $("enabled").checked,
    videoButton: $("videoButton").checked,
    sortFolders: $("sortFolders").checked,
    minSizeKB: Math.max(0, parseInt($("minSizeKB").value, 10) || 0),
    skipHosts: $("skipHosts").value.trim(),
  });
}

for (const id of ["enabled", "videoButton", "sortFolders", "minSizeKB", "skipHosts"]) {
  $(id).addEventListener("change", save);
}

$("open").addEventListener("click", async () => {
  try {
    await api.runtime.sendNativeMessage(HOST, { type: "show" });
    window.close();
  } catch {
    $("status").textContent = "Couldn't start TurboDM";
  }
});

// "Download video on this page", for web pages
let page = null;
api.tabs.query({ active: true, currentWindow: true }).then(async ([tab]) => {
  // (not on YouTube for the Chrome Web Store's copy: its rules don't allow it; background.js)
  const storeCopy = api.runtime.id === "enaaoegoakgmcdbhepcoflfmpeodipob";
  const youtube = /^https?:\/\/([^/]*\.)?(youtube\.com|youtube-nocookie\.com|youtu\.be)(\/|$)/i.test((tab && tab.url) || "");
  // (nor where TurboDM's full extension is on too: that one does it)
  const answer = await api.runtime.sendMessage({ type: "resting?" }).catch(() => null);
  if (answer && answer.resting) {
    $("resting").hidden = false;
    return;
  }
  if (tab && /^https?:/i.test(tab.url || "") && !(storeCopy && youtube)) {
    page = tab;
    $("video").hidden = false;
  }
});

$("video").addEventListener("click", async () => {
  const answer = await api.runtime.sendMessage({ type: "video", url: page.url, storeId: page.cookieStoreId });
  if (answer && answer.ok) {
    window.close();
  } else {
    $("status").textContent = "This TurboDM can't download videos yet: update it";
    $("status").className = "status error";
  }
});

load();
showStatus();
