// The menu of a video's qualities under TurboDM's video button (videobutton.js), as IDM's:
// TurboDM says which qualities the video comes in (it has usually looked already, while the
// video played), and a choice opens TurboDM's Download video window with it, already downloading.
"use strict";

globalThis.__turbodmMenu ??= (() => {
  const api = globalThis.browser ?? globalThis.chrome;

  const STYLE = `
    .menu {
      position: absolute; top: calc(100% + 6px); min-width: 220px; max-height: 330px; overflow-y: auto;
      padding: 5px 0; border-radius: 10px; background: #fff; color: #222;
      box-shadow: 0 6px 22px rgba(0, 0, 0, 0.35); font: 13px/1.25 system-ui, "Segoe UI", sans-serif;
    }
    .menu.up { top: auto; bottom: calc(100% + 6px); }
    .menu.end { right: 0; }
    .menu.start { left: 0; }
    .row { display: flex; justify-content: space-between; gap: 18px; padding: 7px 14px; cursor: pointer; white-space: nowrap; }
    .row:hover { background: #e4f3e9; }
    .size { color: #777; }
    .title { padding: 4px 14px 6px; font-weight: 600; max-width: 280px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .note { padding: 7px 14px; color: #555; max-width: 280px; }
    .line { height: 1px; margin: 4px 0; background: #e3e3e3; }
    @media (prefers-color-scheme: dark) {
      .menu { background: #2b2b2b; color: #eee; }
      .row:hover { background: #2c4a37; }
      .size, .note { color: #aaa; }
      .line { background: #444; }
    }`;

  function size(bytes) {
    if (!bytes) return "";
    const units = ["B", "KB", "MB", "GB"];
    let n = bytes;
    let i = 0;
    for (; n >= 1024 && i < units.length - 1; i++) n /= 1024;
    return `${n.toFixed(i === 0 || n >= 100 ? 0 : 1)} ${units[i]}`;
  }

  function element(className, text) {
    const e = document.createElement("div");
    e.className = className;
    if (text) e.textContent = text;
    return e;
  }

  // A line to pick: `label` (and its size) → `then()`.
  function row(label, bytes, then) {
    const r = element("row");
    r.append(Object.assign(document.createElement("span"), { textContent: label }));
    if (bytes) r.append(Object.assign(document.createElement("span"), { className: "size", textContent: size(bytes) }));
    r.addEventListener("click", then);
    return r;
  }

  function close(box) {
    box.querySelector(".menu")?.remove();
  }

  function isOpen(box) {
    return Boolean(box.querySelector(".menu"));
  }

  // A menu with only a note in it (`place` as for `open`).
  function say(box, place, text) {
    close(box);
    const menu = element(`menu ${place.end ? "end" : "start"}${place.up ? " up" : ""}`);
    menu.addEventListener("click", (e) => e.stopPropagation());
    menu.append(element("note", text));
    box.append(menu);
  }

  // Open the menu in `box` for `what` ({url, referrer}); `place`: {end, up} (which side it lines
  // up with, and whether it opens upwards); `chosen(id)`: a quality picked (null: "More choices",
  // TurboDM's own window).
  async function open(box, what, place, chosen) {
    close(box);
    const menu = element(`menu ${place.end ? "end" : "start"}${place.up ? " up" : ""}`);
    for (const type of ["click", "mousedown", "mouseup", "pointerdown", "pointerup", "wheel"]) {
      menu.addEventListener(type, (e) => e.stopPropagation(), { passive: type === "wheel" });
    }
    menu.append(element("note", "Looking for its qualities…"));
    box.append(menu);
    const pick = (id) => () => {
      close(box);
      chosen(id);
    };
    // a look taking long: said, with TurboDM's own window as a way on (it goes on looking)
    const slow = setTimeout(() => {
      if (!menu.isConnected) return;
      menu.replaceChildren(element("note", "Still looking\u2026 this one takes a while"), element("line"),
                           row("Open in TurboDM\u2026", 0, pick(null)));
    }, 10000);
    let reply = null;
    try {
      reply = await api.runtime.sendMessage({ type: "video-qualities", url: what.url, referrer: what.referrer });
    } catch {
      /* the extension was updated or turned off */
    }
    clearTimeout(slow);
    if (!menu.isConnected) return; // (closed meanwhile)
    menu.replaceChildren();
    if (!reply || !reply.ok) {
      const error = (reply && reply.error) || "TurboDM isn't reachable";
      const old = /bad message|unknown variant/i.test(error);
      menu.append(element("note", old ? "Update TurboDM to choose the quality here" : error));
      menu.append(element("line"), row("Open in TurboDM…", 0, pick(null)));
      return;
    }
    if (reply.title) menu.append(element("title", reply.title));
    const choices = reply.choices || [];
    const videos = choices.filter((c) => !c.audio);
    const sounds = choices.filter((c) => c.audio);
    for (const c of videos) menu.append(row(c.label, c.size, pick(c.id)));
    if (videos.length && sounds.length) menu.append(element("line"));
    for (const c of sounds) menu.append(row(c.label, c.size, pick(c.id)));
    menu.append(element("line"), row("More choices…", 0, pick(null)));
  }

  return { STYLE, open, close, isOpen, say };
})();
