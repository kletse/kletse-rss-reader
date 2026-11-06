#!/usr/bin/env python3
"""Answers CI's walk through Connect as real servers answered when they were run: FreshRSS 1.30.4, Miniflux 2.3.5 and
2.2.8, CommaFeed 7.4.1. Each server is a path, so that one port is all the walk needs. No test depends on what it does
beyond reaching a screen: the answers themselves are tested in src/detect_tests.rs, and those for feed lists in
src/server.rs. /empty-miniflux, /empty-freshrss and /integration are accounts with no feeds, for a config that names them:
it holds no state, so an import finds nothing new afterwards, which is a screen like any other."""

from http.server import BaseHTTPRequestHandler, HTTPServer

CHECK = "/api/greader.php/check/compatibility"
UNAUTHORIZED = ("application/json", 401, b'{"error_message":"access unauthorized"}')
SIGN_IN = ("text/html", 200, b'<title>Sign In - Miniflux</title><input type="password" name="password">')
NOT_FOUND = ("text/html", 404, b"<h1>404 Not Found</h1>")
COMMAFEED = b'{"version":"7.4.1","allowRegistrations":false}'
FAIL = b"FAIL get HTTP Authorization header! Wrong Web server configuration."
LOGIN = ("text/plain", 200, b"SID=me/token\nLSID=null\nAuth=me/token\n")
EXPORT = (
    "text/xml",
    200,
    b'<?xml version="1.0" encoding="UTF-8"?><opml version="2.0"><head><title>Miniflux</title></head><body>'
    b'<outline text="News"><outline title="Alpha" text="Alpha" xmlUrl="http://feeds.example/alpha.xml" '
    b'htmlUrl="http://feeds.example/alpha/" type="rss"></outline></outline></body></opml>',
)
READER = "/reader/api/0"
NO_FEEDS = ("application/json", 200, b'{"subscriptions":[]}')
NO_COUNTS = ("application/json", 200, b'{"max":150,"unreadcounts":[]}')
NO_ITEMS = ("application/json", 200, b'{"itemRefs":[]}')

GET = {
    "/miniflux/": SIGN_IN,
    "/miniflux/v1/me": UNAUTHORIZED,
    "/old/": SIGN_IN,
    "/old/v1/me": UNAUTHORIZED,
    "/no-passwords/": ("text/html", 200, b"<title>Sign In - Miniflux</title>"),
    "/no-passwords/v1/me": UNAUTHORIZED,
    "/freshrss" + CHECK: ("text/plain", 200, b"PASS"),
    "/freshrss-off" + CHECK: ("text/plain", 503, b"Service Unavailable!"),
    "/freshrss-stripped" + CHECK: ("text/plain", 200, FAIL),
    "/commafeed/rest/server/get": ("application/json", 200, COMMAFEED),
    "/empty-miniflux/v1/me": ("application/json", 200, b'{"id":1,"username":"me"}'),
    "/empty-miniflux/v1/feeds": ("application/json", 200, b"[]"),
    "/empty-miniflux/v1/feeds/counters": ("application/json", 200, b'{"reads":{},"unreads":{}}'),
    "/empty-miniflux/v1/entries": ("application/json", 200, b'{"total":0,"entries":[]}'),
    "/empty-miniflux/v1/export": EXPORT,
    "/empty-freshrss/api/greader.php" + READER + "/subscription/list": NO_FEEDS,
    "/empty-freshrss/api/greader.php" + READER + "/unread-count": NO_COUNTS,
    "/empty-freshrss/api/greader.php" + READER + "/stream/items/ids": NO_ITEMS,
    "/empty-freshrss/api/greader.php" + READER + "/subscription/export": EXPORT,
    "/integration" + READER + "/subscription/list": NO_FEEDS,
    "/integration" + READER + "/unread-count": ("application/json", 200, b"[]"),
    "/integration" + READER + "/stream/items/ids": NO_ITEMS,
    # A false success: nothing may go by this.
    "/integration" + READER + "/subscription/export": ("application/json", 200, b"[]"),
}
POST = {
    "/miniflux/v1/api-keys": UNAUTHORIZED,
    "/old/v1/api-keys": ("text/plain", 404, b"404 page not found\n"),
    "/empty-miniflux/v1/import": ("application/json", 201, b'{"message":"Feeds imported successfully"}'),
    "/empty-freshrss/api/greader.php/accounts/ClientLogin": LOGIN,
    "/empty-freshrss/api/greader.php" + READER + "/subscription/import": ("text/html", 200, b"OK"),
    "/integration/accounts/ClientLogin": LOGIN,
    "/integration" + READER + "/subscription/import": ("text/plain", 401, b"Unauthorized"),
}
PUT = {
    "/empty-miniflux/v1/feeds/refresh": ("application/json", 204, b""),
}


class Fake(BaseHTTPRequestHandler):
    def answer(self, routes):
        if self.path.startswith("/gate/"):
            self.send_response(302)
            self.send_header("Location", "http://localhost:18399/sign-in")
            self.end_headers()
            return
        kind, status, body = routes.get(self.path.split("?")[0], NOT_FOUND)
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self.answer(GET)

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        self.answer(POST)

    def do_PUT(self):
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        self.answer(PUT)

    def log_message(self, *_):
        pass


HTTPServer(("127.0.0.1", 18399), Fake).serve_forever()
