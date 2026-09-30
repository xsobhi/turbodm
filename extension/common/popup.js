"use strict";

const api = globalThis.browser ?? globalThis.chrome;
const HOST = "com.xsobhi.turbodm";
const DEFAULTS = { enabled: true, minSizeKB: 0, skipHosts: "" };
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
  $("minSizeKB").value = settings.minSizeKB;
  $("skipHosts").value = settings.skipHosts;
}

function save() {
  api.storage.local.set({
    enabled: $("enabled").checked,
    minSizeKB: Math.max(0, parseInt($("minSizeKB").value, 10) || 0),
    skipHosts: $("skipHosts").value.trim(),
  });
}

for (const id of ["enabled", "minSizeKB", "skipHosts"]) {
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

load();
showStatus();
