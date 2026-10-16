// Which video the video button (videobutton.js) is for, as an address TurboDM's yt-dlp can open:
// the page's own when it's a video's page; in a feed (Instagram, TikTok, Facebook, X, YouTube's
// home page…) the post's link found next to the video. Facebook keeps its posts' links empty
// ("#") until the mouse is on them: they're pointed at so it fills them in, or else the video the
// page has just been streaming is taken (Facebook's video addresses carry the video's number).
// A plain file playing from its own address goes as that file.
"use strict";

globalThis.__turbodmPost ??= (() => {
  const PERMALINK = /\/(watch\/?\?(.*&)?v=|shorts\/|reels?\/|p\/|tv\/|video\/|videos\/|share\/[vr]\/|status\/|posts\/|permalink\.php|story\.php)/i;
  const STRONG = /\/(watch\/?\?(.*&)?v=|shorts\/|reels?\/|p\/|tv\/|video\/|videos\/|share\/[vr]\/)/i; // (a video's, over a post's)
  const site = (host) => host.split(".").slice(-2).join("."); // (www., web., m. are the same site)
  const FACEBOOK = site(location.hostname) === "facebook.com";

  // Facebook: the videos the page streamed lately, by their addresses' "efg" (JSON, in base64,
  // with the video's number), newest last.
  const streamed = [];
  if (FACEBOOK && typeof PerformanceObserver === "function") {
    try {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          const id = videoNumberIn(entry.name);
          if (id) {
            streamed.push({ id, at: performance.now() });
            if (streamed.length > 40) streamed.shift();
          }
        }
      }).observe({ type: "resource", buffered: true });
    } catch {
      /* not offered here */
    }
  }

  function videoNumberIn(address) {
    if (!/fbcdn\.net/.test(address) || !/[?&]efg=/.test(address)) return null;
    try {
      const efg = new URL(address).searchParams.get("efg").replace(/-/g, "+").replace(/_/g, "/");
      const said = JSON.parse(atob(efg));
      const id = said.video_id ?? said.videoId;
      return id ? String(id) : null;
    } catch {
      return null;
    }
  }

  // The links to a post in `node` (on this site) that are `video`'s: none closer to another of
  // the videos there (out as far as a feed, the next post's link isn't this one's).
  function postLinks(node, video) {
    const others = [...node.querySelectorAll("video")].filter((v) => v !== video);
    return [...node.querySelectorAll("a[href]")].filter((a) => {
      try {
        if (!PERMALINK.test(a.href) || site(new URL(a.href).host) !== site(location.host)) return false;
      } catch {
        return false;
      }
      const ours = depthOf(shared(a, video));
      return !others.some((v) => depthOf(shared(a, v)) > ours);
    });
  }

  // The innermost element holding both; how deep an element is.
  function shared(a, b) {
    let node = a;
    while (node && !node.contains(b)) node = node.parentElement;
    return node;
  }

  function depthOf(node) {
    let depth = 0;
    for (; node; node = node.parentElement) depth++;
    return depth;
  }

  // From the video outwards, the first links to a post (Instagram and Facebook nest their videos
  // deep inside a post, so far enough out to reach it); the page's own address if none.
  function permalink(video) {
    if (PERMALINK.test(location.pathname + location.search)) return location.href; // its page already
    let node = video.parentElement;
    for (let depth = 0; node && node !== document.body && depth < 30; depth++, node = node.parentElement) {
      const links = postLinks(node, video);
      if (links.length) return tidy((links.find((a) => STRONG.test(a.href)) || links[0]).href);
    }
    return location.href;
  }

  // Facebook's and Instagram's links carry their tracking ("?__cft__[0]=…", "igsh"): left out.
  function tidy(href) {
    const url = new URL(href);
    for (const key of [...url.searchParams.keys()]) {
      if (key.startsWith("__") || key === "igsh") url.searchParams.delete(key);
    }
    return url.href;
  }

  // Facebook: its post's empty links pointed at (it fills them in), then looked at again; or else
  // the video it streamed last (a minute at most ago). Null: no telling which video this is.
  async function onFacebook(video) {
    const empty = [];
    let node = video.parentElement;
    for (let depth = 0; node && node !== document.body && depth < 30; depth++, node = node.parentElement) {
      for (const a of node.querySelectorAll('a[href="#"], a[role="link"]:not([href])')) {
        if (empty.length < 40 && !empty.includes(a)) empty.push(a);
      }
      if (node.querySelectorAll("video").length > 1) break; // (past its post: others' links)
    }
    for (const a of empty) {
      a.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      a.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    }
    if (empty.length) await new Promise((done) => setTimeout(done, 250));
    const found = permalink(video);
    if (found !== location.href) return found;
    const last = streamed.at(-1);
    return last && performance.now() - last.at < 60000 ? `https://www.facebook.com/watch/?v=${last.id}` : null;
  }

  // What to send for `video` (at once: for the early look): a plain file as itself; a streamed
  // one (YouTube and most sites play "blob:" streams) as the page for TurboDM to look at.
  function message(video) {
    const src = (video && (video.currentSrc || video.src)) || "";
    if (/^https?:/i.test(src)) return { type: "media-file", url: src, referrer: location.href };
    const url = video ? permalink(video) : location.href; // (in a frame: the embedded player's)
    return { type: "video", url, referrer: document.referrer || null };
  }

  // A feed's own address in place of a post's: the video isn't known (yet).
  function unsure(what) {
    return what.type === "video" && FACEBOOK && what.url === location.href && !PERMALINK.test(location.pathname + location.search);
  }

  // What to send, when the button is clicked: Facebook's posts looked at harder. `unknown` set:
  // no telling which video this is.
  async function resolve(video) {
    const what = message(video);
    if (!unsure(what)) return what;
    const url = await onFacebook(video);
    return url ? { ...what, url } : { ...what, unknown: true };
  }

  return { message, resolve, unsure };
})();
