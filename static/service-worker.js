var cacheName = 'ledgyPWA-0.1';
var filesToCache = [
  "/",
  "/static/index.html",
  "/static/logo-128.png",
  "/static/logo-256.png",
  "/static/main.css",
  "/static/main.js",

  "/static/external/bootstrap.min.css",
  "/static/external/bootstrap.min.js",
  "/static/external/jquery-3.3.1.slim.min.js",
  "/static/external/jquery-dateformat.min.js",
  "/static/external/popper.min.js",
];

var externalFilesToCache = [
];

self.addEventListener('install', function(e) {
  console.log('[ServiceWorker] Install');
  e.waitUntil(
    caches.open(cacheName).then(function(cache) {
      console.log('[ServiceWorker] Caching app shell');
      return cache.addAll(filesToCache);
    })
  );
});

self.addEventListener('activate', function(e) {
  console.log('[ServiceWorker] Activate');
  e.waitUntil(
    caches.keys().then(function(keyList) {
      return Promise.all(keyList.map(function(key) {
        if (key !== cacheName) {
          console.log('[ServiceWorker] Removing old cache', key);
          return caches.delete(key);
        }
      }));
    })
  );
  return self.clients.claim();
});

self.addEventListener('fetch', function(e) {
  console.log('[ServiceWorker] Fetch', e.request.url);
  e.respondWith(
    caches.match(e.request).then(function(response) {
      return response || fetch(e.request);
    })
  );
});
