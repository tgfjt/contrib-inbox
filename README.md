# contrib-inbox

External contribution inbox for me. A PWA built with `gpui-web` (unofficial `gpui` web backend).

3 panes — left: external PR / Issue list · middle: title / repo / updated / state / CI ·
right: body, latest comments, reviews.

## Run

```sh
trunk serve
# open http://127.0.0.1:8080
```

`trunk serve` sends the required isolation headers (see below).
Production build: `trunk build --release`. Needs the stable toolchain with the
wasm target (`rust-toolchain.toml` pins it).

## Sign in (OAuth Device Flow)

No tokens are typed anywhere. GitHub's OAuth endpoints send no CORS headers,
so the PWA cannot call them directly — `trunk serve` relays same-origin
`/gh-oauth/*` → `https://github.com/*` (see `[[proxy]]` in `Trunk.toml`).
Device Flow needs no `client_secret`, only the public OAuth App client_id.

One-time setup (2 min):

1. github.com → Settings → Developer settings → OAuth Apps → New OAuth App.
   Name/homepage can be anything local (e.g. `http://127.0.0.1:8080`);
   the callback URL field is required but unused by Device Flow.
2. Check **Enable Device Flow**, create, copy the **Client ID**
   (the secret is never needed).
3. In the app press `s`, paste the Client ID (stored in `localStorage`).
   The app shows a one-time code — enter it at `github.com/login/device`
   (a tab opens automatically; a clickable link is also shown).

The issued token (scope `repo`) is stored in `localStorage` and sent only to
`api.github.com`. `s` again signs out.

## Keys

| key   | action                              |
| ----- | ----------------------------------- |
| j/k, ↑/↓ | move selection                   |
| enter | open on GitHub (new tab, `noopener`) |
| r     | refresh                             |
| c     | comment on selected item            |
| x     | close selected item (open only)     |
| s     | sign in / out (OAuth Device Flow)   |
| 1-4   | filter: open / merged / closed / stale |

List query: `author:<you> archived:false` (default excludes archived repos),
then client-side exclusion of `MEMBER` / `OWNER` / `COLLABORATOR`
(`author_association`), so only true external contributions remain.
Stale = open and not updated for 30 days. Items updated since you last looked
at them are highlighted; selecting an item marks it seen (persisted in
`localStorage`).

## Headers (COOP/COEP)

`Trunk.toml` sets, for both dev serve and as documentation for hosting:

```toml
[serve.headers]
Cross-Origin-Opener-Policy = "same-origin"
Cross-Origin-Embedder-Policy = "require-corp"
```

Notes:

- The build is **single-threaded** (`WebPlatform::new(false)`, `gpui-web` with
  `default-features = false`), so it runs on the stable toolchain and does not
  need `SharedArrayBuffer`. The headers are kept anyway as defense-in-depth and
  so a future multithreaded build keeps working. (Multithreaded gpui-web needs
  nightly + atomics + these exact headers.)
- `require-corp` is safe here because everything is same-origin (bundled
  fonts, no CDN). GitHub API calls use CORS-mode `fetch`, whose
  `Access-Control-Allow-Origin` responses pass COEP.
- `same-origin` severs the `window.opener` link for cross-origin popups, but
  `enter` opens GitHub with `noopener` anyway, so nothing breaks.

## PWA

`public/`: `manifest.webmanifest`, `icon.svg`, `sw.js` (copied to `dist/` by
trunk). The service worker caches the app shell same-origin and never caches
`api.github.com`. Install prompt needs a 192px+ PNG icon (only SVG is bundled
for now — installability is best-effort).

## Gotchas learned with gpui-web 1.21

- `Application::run` holds the `AppCell` borrow for the whole launch callback:
  **no `AsyncApp` read/update/spawn inside it** (panics with
  `RefCell already borrowed`). Defer with `cx.spawn` (tasks run after release).
  Same hazard applies anywhere a borrow may be held — all key/click handlers
  here defer work via `cx.spawn` and only mutate inside the task's updates.
- `std::time::SystemTime` panics on `wasm32-unknown-unknown`
  ("time not implemented on this platform"). Wall clock goes through
  `js_sys::Date::now()`.
- `overflow_y_scroll` requires a stateful (`id`-ed) div.
- The bundled font lacks `●`/emoji glyphs; status dots are drawn divs.
  Emoji in bodies may show tofu — accepted for now.
