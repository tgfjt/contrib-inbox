// contrib-inbox service worker: app-shell offline, API always network.
// Bump VERSION on every shipped UI change so clients drop the old shell.
const VERSION = "v0.2.0";
const SHELL = "inbox-shell-" + VERSION;

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(SHELL).then((cache) => cache.addAll(["/", "/index.html"])).then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((k) => k !== SHELL).map((k) => caches.delete(k))),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return; // api.github.com etc: network only
  event.respondWith(
    caches.match(event.request).then(
      (hit) =>
        hit ||
        fetch(event.request).then((res) => {
          const copy = res.clone();
          caches.open(SHELL).then((cache) => cache.put(event.request, copy));
          return res;
        }),
    ),
  );
});
