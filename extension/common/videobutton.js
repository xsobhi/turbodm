// TurboDM's video button, like IDM's: a small download icon over a video while the mouse is on it
// (and for a few seconds when one starts playing), faint until pointed at, when it opens into
// "Download ▾" and its ×. A click shows the video's qualities (videomenu.js) and the one picked
// goes to TurboDM; a plain file (an .mp4 playing from its own address) goes as a download of that
// file. It can be dragged anywhere on the video, and stays there on that site. When it shows, a
// running TurboDM starts looking at the video, so the qualities are ready by the click. Runs in
// every frame of every page; costs nothing until a video shows.
"use strict";

(() => {
  if (window.__turbodmVideoButton) return; // (injected once per frame)
  window.__turbodmVideoButton = true;

  const api = globalThis.browser ?? globalThis.chrome;
  const menu = globalThis.__turbodmMenu;
  const MIN_WIDTH = 240; // smaller videos (thumbnails, adverts) get no button
  const MIN_HEIGHT = 135;
  const LINGER_MS = 1500; // after the mouse leaves the video
  const ON_PLAY_MS = 4000; // when a video starts playing
  const ICON = 32; // the button's size, folded
  const EDGE = 12; // from the video's corner, where it goes first
  const SVG = "http://www.w3.org/2000/svg";
  const SITE = location.hostname.replace(/^www\./, "");

  let on = true;
  let places = {}; // where it was dragged, by site: {x, y}, the icon's centre as fractions of the video
  let host = null;
  let box = null;
  let pill = null;
  let label = null;
  let side = { end: true, up: false }; // which way it opens (from the video's right half: leftwards)
  let shownFor = null; // the video it's over
  let hideTimer = 0;
  let pending = false; // a mouse move waiting to be looked at
  let press = null; // a press that may become a drag
  let dragging = false;
  let dragged = false; // (the click that ends a drag isn't one)
  const dismissed = new Set(); // videos (their addresses) whose button was closed
  const peeked = new Set(); // videos TurboDM was told to look at

  api.storage.local.get({ videoButton: true, videoButtonPlaces: {} }).then((s) => {
    on = s.videoButton !== false;
    places = s.videoButtonPlaces || {};
  }, () => {});
  api.storage.onChanged.addListener((changes) => {
    if (changes.videoButton) {
      on = changes.videoButton.newValue !== false;
      if (!on) hide(true);
    }
    if (changes.videoButtonPlaces) places = changes.videoButtonPlaces.newValue || {};
  });

  const STYLE = `
    .box { position: relative; display: inline-block; }
    .pill {
      display: flex; align-items: center; height: ${ICON}px; border-radius: ${ICON / 2}px;
      background: #2e9a4f; color: #fff; box-shadow: 0 2px 8px rgba(0, 0, 0, 0.35);
      font: 600 13px/1 system-ui, "Segoe UI", sans-serif; cursor: pointer; user-select: none;
      touch-action: none; opacity: 0.5; transition: opacity 0.15s;
    }
    .box:hover .pill, .box.open .pill, .box.dragging .pill { opacity: 1; }
    .box.dragging .pill { cursor: grabbing; }
    .pill.rev, .rev .more { flex-direction: row-reverse; }
    .icon { width: ${ICON}px; height: ${ICON}px; display: grid; place-items: center; flex: none; }
    .icon svg { width: 16px; height: 16px; }
    .more { display: flex; align-items: center; max-width: 0; overflow: hidden; white-space: nowrap; transition: max-width 0.18s ease; }
    .box:hover .more, .box.open .more { max-width: 200px; }
    .text { padding: 0 8px 0 0; }
    .rev .text { padding: 0 0 0 8px; }
    .x { height: ${ICON}px; padding: 0 11px 0 9px; display: grid; place-items: center; font-size: 16px; font-weight: 400;
         border-left: 1px solid rgba(255, 255, 255, 0.3); }
    .rev .x { border-left: 0; border-right: 1px solid rgba(255, 255, 255, 0.3); padding: 0 9px 0 11px; }
    .x:hover { background: rgba(0, 0, 0, 0.15); }
    ${menu.STYLE}`;

  function make() {
    host = document.createElement("turbodm-video-button");
    host.style.cssText = "all: initial; position: fixed; z-index: 2147483647; display: none; top: 0; left: 0;";
    const root = host.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = STYLE;
    box = document.createElement("div");
    box.className = "box";
    pill = document.createElement("div");
    pill.className = "pill";
    pill.title = "Download this video with TurboDM";
    const icon = document.createElement("span");
    icon.className = "icon";
    icon.append(arrow());
    const more = document.createElement("span");
    more.className = "more";
    label = Object.assign(document.createElement("span"), { className: "text", textContent: "Download ▾" });
    const close = Object.assign(document.createElement("span"), { className: "x", textContent: "×" });
    close.title = "Hide this for this video";
    more.append(label, close);
    pill.append(icon, more);
    box.append(pill);
    root.append(style, box);
    // (the page mustn't see what happens on it: a player would pause or go full screen)
    for (const type of ["click", "mousedown", "mouseup", "dblclick", "pointerdown", "pointerup", "contextmenu"]) {
      box.addEventListener(type, (e) => e.stopPropagation());
    }
    box.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    box.addEventListener("mouseleave", () => later(LINGER_MS));
    pill.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      press = { x: e.clientX, y: e.clientY };
      try {
        pill.setPointerCapture(e.pointerId); // (the drag follows the mouse off the button)
      } catch {
        /* not a live pointer */
      }
    });
    pill.addEventListener("pointermove", drag);
    pill.addEventListener("pointerup", drop);
    pill.addEventListener("click", (e) => {
      if (dragged) return;
      if (e.composedPath().includes(close)) return dismiss();
      act();
    });
  }

  // A download arrow, drawn here (pages can't load the extension's own pictures).
  function arrow() {
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("viewBox", "0 0 16 16");
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", "M8 1.5v8.2M4.4 6.4 8 10l3.6-3.6M2.5 13.5h11");
    for (const [k, v] of [["fill", "none"], ["stroke", "currentColor"], ["stroke-width", "1.9"],
                          ["stroke-linecap", "round"], ["stroke-linejoin", "round"]]) path.setAttribute(k, v);
    svg.append(path);
    return svg;
  }

  function big(video) {
    const r = video && video.getBoundingClientRect();
    return r && r.width >= MIN_WIDTH && r.height >= MIN_HEIGHT ? r : null;
  }

  // The video under the mouse (players cover theirs with controls: go by where it is).
  function videoAt(x, y) {
    for (const video of document.querySelectorAll("video")) {
      const r = big(video);
      if (r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return video;
    }
    return null;
  }

  const clamp = (v, low, high) => Math.min(Math.max(v, low), Math.max(low, high));

  // Put the icon's centre at (x, y), inside the video and on screen. From the video's right half
  // it opens leftwards (anchored by its right edge), so the icon stays under the mouse.
  function put(x, y, r) {
    const half = ICON / 2 + 4;
    x = clamp(x, r.left + half, r.right - half);
    y = clamp(y, Math.max(r.top, 0) + half, Math.min(r.bottom, innerHeight) - half);
    side = { end: x > r.left + r.width / 2, up: y > innerHeight - 360 };
    pill.classList.toggle("rev", side.end);
    host.style.top = `${y - ICON / 2}px`;
    if (side.end) {
      host.style.left = "auto";
      host.style.right = `${document.documentElement.clientWidth - (x + ICON / 2)}px`;
    } else {
      host.style.right = "auto";
      host.style.left = `${x - ICON / 2}px`;
    }
  }

  // Where it goes on `video`: where it was dragged on this site, else by the top right corner.
  function place(video, r) {
    const p = places[SITE];
    if (p) put(r.left + p.x * r.width, r.top + p.y * r.height, r);
    else put(r.right - EDGE - ICON / 2, Math.max(r.top, 0) + EDGE + ICON / 2, r);
  }

  function show(video, ms) {
    if (!on || dragging) return;
    const r = big(video);
    if (!r) return;
    const what = message(video);
    if (dismissed.has(what.url)) return;
    lookAhead(what);
    if (!host) make();
    // in full screen only what's inside the full-screen element shows
    const parent = document.fullscreenElement || document.documentElement;
    if (host.parentNode !== parent) parent.append(host);
    if (shownFor !== video) menu.close(box);
    shownFor = video;
    host.style.display = "block";
    place(video, r);
    clearTimeout(hideTimer);
    if (ms) later(ms);
  }

  // Gone (but not while its menu is open, unless `now`).
  function hide(now) {
    if (!host || (!now && menu.isOpen(box))) return;
    clearTimeout(hideTimer);
    menu.close(box);
    box.classList.remove("open");
    host.style.display = "none";
    shownFor = null;
  }

  function later(ms) {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(hide, ms);
  }

  function moved(x, y) {
    const video = videoAt(x, y);
    if (video) show(video);
    else if (shownFor) later(LINGER_MS);
  }

  function drag(e) {
    if (!press || !shownFor) return;
    if (!dragging && Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return;
    if (!dragging) {
      dragging = true;
      box.classList.add("dragging");
      menu.close(box);
    }
    const r = big(shownFor);
    if (r) put(e.clientX, e.clientY, r);
  }

  function drop(e) {
    press = null;
    if (!dragging) return;
    dragging = false;
    box.classList.remove("dragging");
    dragged = true;
    setTimeout(() => (dragged = false), 0);
    const r = big(shownFor);
    if (!r) return;
    const x = clamp(e.clientX, r.left, r.right);
    const y = clamp(e.clientY, r.top, r.bottom);
    places = { ...places, [SITE]: { x: (x - r.left) / r.width, y: (y - r.top) / r.height } };
    api.storage.local.set({ videoButtonPlaces: places }).catch(() => {});
  }

  function dismiss() {
    if (shownFor) dismissed.add(message(shownFor).url);
    hide(true);
  }

  // A video's own page: where a feed links each post (Instagram, TikTok, Facebook, X, YouTube's
  // home page…). Feeds' own addresses aren't the video's, so look next to it for the post's link.
  const PERMALINK = /\/(watch\?(.*&)?v=|shorts\/|reels?\/|p\/|tv\/|video\/|videos\/|watch\/|status\/|posts\/)/i;

  function permalink(video) {
    if (PERMALINK.test(location.href)) return location.href; // its page already (YouTube, Bilibili…)
    let node = video.parentElement;
    for (let depth = 0; node && node !== document.body && depth < 8; depth++, node = node.parentElement) {
      for (const a of node.querySelectorAll("a[href]")) {
        if (PERMALINK.test(a.href) && new URL(a.href).host.endsWith(location.host.replace(/^www\./, ""))) {
          return a.href;
        }
      }
    }
    return location.href;
  }

  // What to send for `video`: a plain file (an .mp4 playing from its own address) goes to TurboDM
  // as a download of that file, on many connections; a streamed one (YouTube and most sites play
  // "blob:" streams) as the page for TurboDM to find its video and qualities.
  function message(video) {
    const src = (video && (video.currentSrc || video.src)) || "";
    if (/^https?:/i.test(src)) return { type: "media-file", url: src, referrer: location.href };
    const url = video ? permalink(video) : location.href; // (in a frame: the embedded player's)
    return { type: "video", url, referrer: document.referrer || null };
  }

  // TurboDM, if it's running, starts looking at the video now (it isn't started for this):
  // its qualities are ready by the time they're asked for. Once per video.
  function lookAhead(what) {
    if (what.type !== "video" || peeked.has(what.url)) return;
    peeked.add(what.url);
    try {
      api.runtime.sendMessage({ type: "video-peek", url: what.url, referrer: what.referrer }).catch(() => {});
    } catch {
      /* the extension was updated or turned off */
    }
  }

  // The button clicked: the qualities to pick from (a plain file just goes).
  function act() {
    if (!shownFor) return;
    if (menu.isOpen(box)) {
      menu.close(box);
      box.classList.remove("open");
      return;
    }
    const what = message(shownFor);
    if (what.type !== "video") return send(what);
    box.classList.add("open");
    clearTimeout(hideTimer);
    menu.open(box, what, side, (choice) => {
      box.classList.remove("open");
      send(choice ? { ...what, choice } : what);
    });
  }

  async function send(message) {
    let answer = null;
    try {
      answer = await api.runtime.sendMessage(message);
    } catch {
      /* the extension was updated or turned off: no answer */
    }
    label.textContent = answer && answer.ok ? "Sent to TurboDM" : "TurboDM isn't reachable";
    box.classList.add("open");
    setTimeout(() => {
      label.textContent = "Download ▾";
      box.classList.remove("open");
    }, 2500);
    later(2500);
  }

  // (looked at 20 times a second at most, where the mouse is by then)
  let mouse = null;
  document.addEventListener(
    "mousemove",
    (e) => {
      mouse = e;
      if (pending) return;
      pending = true;
      setTimeout(() => {
        pending = false;
        moved(mouse.clientX, mouse.clientY);
      }, 50);
    },
    { passive: true, capture: true },
  );
  document.addEventListener("play", (e) => e.target instanceof HTMLVideoElement && show(e.target, ON_PLAY_MS), true);
  document.addEventListener("scroll", () => shownFor && show(shownFor), { passive: true, capture: true });
  document.addEventListener("fullscreenchange", () => shownFor && show(shownFor, ON_PLAY_MS));
  // a click elsewhere, or Escape, closes the menu
  document.addEventListener("pointerdown", (e) => {
    if (host && menu.isOpen(box) && !e.composedPath().includes(host)) {
      menu.close(box);
      box.classList.remove("open");
      later(LINGER_MS);
    }
  }, true);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && host && menu.isOpen(box)) {
      menu.close(box);
      box.classList.remove("open");
    }
  }, true);
})();
