# Putting the extension in the browsers' stores

Chrome, Edge, Brave and Vivaldi install an extension on their own only from a store. Once
TurboDM's extension is in one, installing TurboDM makes those browsers ask by themselves ("New
extension added: TurboDM Integration", then *Enable extension*), as they do for IDM's. Without a
store, users have to turn on Developer mode and load the folder by hand.

Firefox doesn't need a store: the Mozilla-signed `.xpi` ships with TurboDM, and Firefox asks
to add it the first time TurboDM starts.

| Store | Cost | Covers |
|---|---|---|
| [Chrome Web Store](https://chrome.google.com/webstore/devconsole) | US$5 once, for the account | Chrome, Brave, Vivaldi, Chromium; Edge and Opera too |
| [Microsoft Edge Add-ons](https://partner.microsoft.com/dashboard/microsoftedge/overview) | free | Edge (optional: Edge takes Chrome Web Store extensions) |

## Upload

Upload `turbodm-extension-<version>-for-stores.zip`. Each release's build attaches it as
*extension-for-stores*, and `tools/build-extension.sh` makes it in `dist/`. It's the Chrome
extension without the `key` field, because the stores pick the extension's ID themselves.

It also leaves YouTube out, because the Chrome Web Store doesn't allow extensions that download
YouTube's videos: no video button on YouTube's pages or players, and no *Download video* there
(`tools/store-manifest.py`, and the `STORE_COPY` check in `background.js` and `popup.js`). The copy
loaded by hand and Firefox's keep YouTube, and TurboDM's own *Download video…* takes any page.

What the store asks for:

- **Single purpose:** Sends the downloads you start in the browser to the TurboDM download
  manager on your computer.
- **Permissions:**
  - `downloads`: to see the downloads the browser starts and hand them to TurboDM instead.
  - `nativeMessaging`: to reach the TurboDM app on the computer.
  - `contextMenus`: for "Download with TurboDM" on links.
  - `cookies` and host permissions: to send the site's cookies with a download, so files that
    need you to be signed in download in TurboDM too.
  - `storage`: for the extension's own settings.
- **Remote code:** none.
- **Data use:** nothing is collected or sent off the computer.
- **Privacy policy:** <https://github.com/xsobhi/turbodm/blob/main/docs/privacy.md>
- **Icon and screenshot:** `extension/icons/icon-128.png` and `packaging/stores/screenshot.png`
  (1280×800).

## After it's published

Put the IDs the stores gave in `src/config.rs`:

```rust
pub const CHROME_STORE_ID: Option<&str> = Some("…32 letters…");
pub const EDGE_STORE_ID: Option<&str> = Some("…");   // only if it's in Edge Add-ons too
```

With an ID set, TurboDM:

- lets the store's extension talk to the app;
- lists it in the registry where the browsers look (`Software\Google\Chrome\Extensions\<id>`,
  which Brave and Vivaldi read too, and `Software\Microsoft\Edge\Extensions\<id>`), for this
  user and, from the installer, for everyone;
- removes that listing on uninstall, so the browsers remove the extension;
- opens the store page from *Add to your browser*.

Also put the store's ID in `allowed_origins` in `packaging/linux/native-messaging/chrome.json`, for
the Linux packages.
