#!/usr/bin/env python3
"""Answers CI's walk through Connect as real servers answered when they were run: FreshRSS 1.30.4, Miniflux 2.3.5 and
2.2.8, CommaFeed 7.4.1. Each server is a path, so that one port is all the walk needs. No test depends on what it does
beyond reaching a screen: the answers themselves are tested in src/detect_tests.rs."""

from http.server import BaseHTTPRequestHandler, HTTPServer

CHECK = "/api/greader.php/check/compatibility"
UNAUTHORIZED = ("application/json", 401, b'{"error_message":"access unauthorized"}')
SIGN_IN = ("text/html", 200, b'<title>Sign In - Miniflux</title><input type="password" name="password">')
NOT_FOUND = ("text/html", 404, b"<h1>404 Not Found</h1>")
COMMAFEED = b'{"version":"7.4.1","allowRegistrations":false}'
FAIL = b"FAIL get HTTP Authorization header! Wrong Web server configuration."

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
}
POST = {
    "/miniflux/v1/api-keys": UNAUTHORIZED,
    "/old/v1/api-keys": ("text/plain", 404, b"404 page not found\n"),
}


class Fake(BaseHTTPRequestHandler):
    def answer(self, routes):
        if self.path.startswith("/gate/"):
            self.send_response(302)
            self.send_header("Location", "http://localhost:18399/sign-in")
            self.end_headers()
            return
        kind, status, body = routes.get(self.path, NOT_FOUND)
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

    def log_message(self, *_):
        pass


HTTPServer(("127.0.0.1", 18399), Fake).serve_forever()
