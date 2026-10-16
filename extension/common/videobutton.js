// TurboDM's "Download this video" button, like IDM's: it shows over a video while the mouse is
// on it (and for a few seconds when one starts playing). A click sends the page, or the frame
// that plays the video (an embedded YouTube player, say), to TurboDM, which offers its qualities
// and formats (MP4, MP3…). When it shows, TurboDM (if running) starts looking at the video, so the
// qualities are ready by the click. Its × hides it for that video. Runs in every frame of every
// page; costs nothing until a video shows.
"use strict";

(() => {
  if (window.__turbodmVideoButton) return; // (injected once per frame)
  window.__turbodmVideoButton = true;

  const api = globalThis.browser ?? globalThis.chrome;
  const MIN_WIDTH = 240; // smaller videos (thumbnails, adverts) get no button
  const MIN_HEIGHT = 135;
  const LINGER_MS = 1500; // after the mouse leaves the video
  const ON_PLAY_MS = 4000; // when a video starts playing
  const SVG = "http://www.w3.org/2000/svg";

  let on = true;
  let host = null;
  let button = null;
  let label = null;
  let shownFor = null; // the video it's over
  const dismissed = new Set(); // videos (their addresses) whose button was closed
  const peeked = new Set(); // videos TurboDM was told to look at
  let hideTimer = 0;
  let pending = false; // a mouse move waiting for the next frame

  api.storage.local.get({ videoButton: true }).then((s) => (on = s.videoButton), () => {});
  api.storage.onChanged.addListener((changes) => {
    if (changes.videoButton) {
      on = changes.videoButton.newValue !== false;
      if (!on) hide();
    }
  });

  function make() {
    host = document.createElement("turbodm-video-button");
    host.style.cssText = "all: initial; position: fixed; z-index: 2147483647; display: none; top: 0; left: 0;";
    const root = host.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = `
      .pill {
        display: flex; align-items: stretch; border-radius: 18px; overflow: hidden;
        box-shadow: 0 2px 8px rgba(0, 0, 0, 0.35); opacity: 0.94;
      }
      .pill:hover { opacity: 1; }
      button {
        display: flex; align-items: center; gap: 7px; border: 0; margin: 0;
        font: 600 13px/1 system-ui, "Segoe UI", sans-serif; color: #fff;
        background: #2e9a4f; cursor: pointer; white-space: nowrap;
      }
      button:hover { background: #26843f; }
      .go { padding: 7px 11px 7px 10px; }
      .x { padding: 0 10px 0 8px; font-size: 16px; font-weight: 400; border-left: 1px solid rgba(255, 255, 255, 0.3); }
      svg { width: 16px; height: 16px; flex: none; }`;
    button = document.createElement("div");
    button.className = "pill";
    const go = document.createElement("button");
    go.className = "go";
    go.title = "Download this video with TurboDM";
    go.append(arrow());
    label = document.createElement("span");
    label.textContent = "Download this video";
    go.append(label);
    go.addEventListener("click", send);
    const close = document.createElement("button");
    close.className = "x";
    close.title = "Hide this for this video";
    close.textContent = "\u00d7";
    close.addEventListener("click", () => {
      if (shownFor) dismissed.add(message(shownFor).url);
      hide();
    });
    button.append(go, close);
    // (the page mustn't see these clicks: a player would pause or go full screen)
    for (const type of ["click", "mousedown", "mouseup", "dblclick", "pointerdown", "pointerup"]) {
      button.addEventListener(type, (e) => e.stopPropagation());
    }
    button.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    button.addEventListener("mouseleave", () => later(LINGER_MS));
    root.append(style, button);
  }

  // A download arrow, drawn here (pages can't load the extension's own pictures).
  function arrow() {
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("viewBox", "0 0 16 16");
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", "M8 1.5v8.2M4.4 6.4 8 10l3.6-3.6M2.5 13.5h11");
    path.setAttribute("fill", "none");
    path.setAttribute("stroke", "currentColor");
    path.setAttribute("stroke-width", "1.9");
    path.setAttribute("stroke-linecap", "round");
    path.setAttribute("stroke-linejoin", "round");
    svg.append(path);
    return svg;
  }

  function big(video) {
    const r = video.getBoundingClientRect();
    return r.width >= MIN_WIDTH && r.height >= MIN_HEIGHT ? r : null;
  }

  // The video under the mouse (players cover theirs with controls: go by where it is).
  function videoAt(x, y) {
    for (const video of document.querySelectorAll("video")) {
      const r = big(video);
      if (r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return video;
    }
    return null;
  }

  function show(video, ms) {
    if (!on) return;
    const r = big(video);
    if (!r) return;
    const what = message(video);
    if (dismissed.has(what.url)) return;
    lookAhead(what);
    if (!host) make();
    // in full screen only what's inside the full-screen element shows
    const parent = document.fullscreenElement || document.documentElement;
    if (host.parentNode !== parent) parent.append(host);
    shownFor = video;
    host.style.display = "block";
    const width = host.offsetWidth || 170;
    host.style.left = `${Math.max(r.left + 8, r.right - width - 12)}px`;
    host.style.top = `${Math.max(r.top, 0) + 12}px`;
    clearTimeout(hideTimer);
    if (ms) later(ms);
  }

  function hide() {
    clearTimeout(hideTimer);
    if (host) host.style.display = "none";
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

  async function send() {
    const video = shownFor;
    let answer = null;
    try {
      answer = await api.runtime.sendMessage(message(video));
    } catch {
      /* the extension was updated or turned off: no answer */
    }
    label.textContent = answer && answer.ok ? "Sent to TurboDM" : "TurboDM isn't reachable";
    setTimeout(() => (label.textContent = "Download this video"), 2500);
    later(2500);
  }
})();
