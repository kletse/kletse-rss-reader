#!/usr/bin/env python3
"""Answers CI's walk through Connect as real servers answered when they were run: FreshRSS 1.30.4, Miniflux 2.3.5 and
2.2.8, CommaFeed 7.4.1. Each server is a path, so that one port is all the walk needs. No test depends on what it does
beyond reaching a screen: the answers themselves are tested in src/detect_tests.rs, and those for feed lists in
src/server.rs. /empty-miniflux, /empty-freshrss and /integration are accounts with no feeds, for a config that names them:
it holds no state, so an import finds nothing new afterwards, which is a screen like any other.

/feeds-miniflux and /feeds-freshrss do hold state, as changing feeds is nothing but that, and POST /reset puts both back.
They keep to what those servers did: subscribing fails in the words each used, deleting a Miniflux category takes its feeds
with it, FreshRSS's rename-tag onto a name that's taken answers OK and does nothing, and it keeps its default category.

/readeck-approve, /readeck-deny, /readeck-elsewhere and /readeck-none are Readeck 0.23.5 at each end of connecting in a
browser, with a poll a second rather than one every five. The token is another thing a test can look for on disk."""

import json
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlsplit

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
    "/empty-miniflux/v1/categories": ("application/json", 200, b'[{"id":1,"title":"All","user_id":1}]'),
    "/empty-freshrss/api/greader.php" + READER + "/subscription/list": NO_FEEDS,
    "/empty-freshrss/api/greader.php" + READER + "/unread-count": NO_COUNTS,
    "/empty-freshrss/api/greader.php" + READER + "/stream/items/ids": NO_ITEMS,
    "/empty-freshrss/api/greader.php" + READER + "/subscription/export": EXPORT,
    "/empty-freshrss/api/greader.php" + READER + "/tag/list": (
        "application/json",
        200,
        b'{"tags":[{"id":"user/-/state/com.google/starred"},{"id":"user/-/label/Uncategorized","type":"folder"}]}',
    ),
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


def fresh():
    """Two categories with feeds, and one with none, on each. The empty category is what a list of feeds can't show."""
    return {
        "miniflux": {
            "categories": {1: "News", 2: "Tech", 3: "Empty"},
            "feeds": {10: ("Alpha", 1), 11: ("Beta", 1), 12: ("Gamma", 2)},
            "next": 20,
        },
        "freshrss": {
            "labels": ["News", "Tech", "Empty", "Uncategorized"],
            "feeds": {10: ("Alpha", "News"), 11: ("Beta", "News"), 12: ("Gamma", "Tech")},
            "next": 20,
        },
    }


ACCOUNTS = fresh()
JSON = "application/json"
TEXT = "text/plain"
OK = ("text/html", 200, b"OK")
TAKEN_FEEDS = "http://feeds.example/alpha.xml"
GONE = (JSON, 404, b'{"error_message":"resource not found"}')

READECK_INFO = (JSON, 200, b'{"version":{"canonical":"0.23.5","release":"0.23.5","build":""},"features":["oauth"]}')
READECK_PENDING = (JSON, 400, b'{"error":"authorization_pending"}')
READECK_DENIED = (JSON, 400, b'{"error":"access_denied"}')
READECK_TOKEN = (
    JSON,
    201,
    b'{"id":"tok","access_token":"the-readeck-token","token_type":"Bearer","scope":"bookmarks:write"}',
)
READECK_POLLS = {}


def readeck(name, route, verb):
    """A Readeck, by what's after its path. /readeck-approve says yes at the second poll, /readeck-deny no at the first."""
    if verb == "GET" and route == "/api/info":
        return (JSON, 200, b'{"features":[]}') if name == "none" else READECK_INFO
    if verb == "GET" and route == "/api/profile":
        return (JSON, 200, b'{"user":{"username":"me"}}')
    if verb != "POST" or name == "none":
        return NOT_FOUND
    if route == "/api/oauth/client":
        return (JSON, 201, b'{"client_id":"urn:uuid:4c1dc763","token_endpoint_auth_method":"none"}')
    if route == "/api/oauth/device":
        host = "http://example.com" if name == "elsewhere" else "http://127.0.0.1:18399/readeck-" + name
        code = {
            "device_code": "the-device-code",
            "user_code": "VSCZLVQJ",
            "verification_uri": host + "/device",
            "verification_uri_complete": host + "/device?user_code=VSCZLVQJ",
            "expires_in": 300,
            "interval": 1,
        }
        return (JSON, 200, json.dumps(code).encode())
    if route == "/api/oauth/token":
        READECK_POLLS[name] = READECK_POLLS.get(name, 0) + 1
        if name == "deny":
            return READECK_DENIED
        return READECK_TOKEN if name == "approve" and READECK_POLLS[name] > 1 else READECK_PENDING
    return NOT_FOUND


def as_json(value, status=200):
    return (JSON, status, json.dumps(value).encode())


def refused(status, reason):
    return as_json({"error_message": reason}, status)


def miniflux_get(route):
    account = ACCOUNTS["miniflux"]
    if route == "/v1/me":
        return as_json({"id": 1, "username": "me"})
    if route == "/v1/feeds":
        cat = account["categories"]
        return as_json(
            [
                {"id": i, "title": t, "category": {"id": c, "title": cat[c]}}
                for i, (t, c) in account["feeds"].items()
            ]
        )
    if route == "/v1/feeds/counters":
        return as_json({"reads": {}, "unreads": {str(i): 3 for i in account["feeds"]}})
    if route == "/v1/categories":
        return as_json([{"id": i, "title": t, "user_id": 1} for i, t in account["categories"].items()])
    if route == "/v1/entries":
        return as_json({"total": 0, "entries": []})
    return NOT_FOUND


def miniflux_change(verb, route, body):
    account = ACCOUNTS["miniflux"]
    found = route.split("/")
    if verb == "POST" and route == "/v1/feeds":
        url = json.loads(body)["feed_url"]
        if url == TAKEN_FEEDS:
            return refused(400, "This feed already exists.")
        if "://" not in url:
            return refused(400, "Invalid feed URL.")
        if url.endswith(".html"):
            return refused(500, "parser: unable to detect feed format")
        if ".invalid" in url:
            return refused(500, 'fetcher: Get "%s": dial tcp: lookup nohost.invalid: no such host' % url)
        feed, category = account["next"], json.loads(body).get("category_id", min(account["categories"]))
        account["next"] += 1
        account["feeds"][feed] = ("New feed", category)
        return as_json({"feed_id": feed}, 201)
    if route == "/v1/categories" and verb == "POST":
        title = json.loads(body)["title"]
        if title in account["categories"].values():
            return refused(400, "This category already exists.")
        category, account["next"] = account["next"], account["next"] + 1
        account["categories"][category] = title
        return as_json({"id": category, "title": title}, 201)
    if route == "/v1/feeds/refresh":
        return (JSON, 204, b"")
    if len(found) != 4 or not found[3].isdigit():
        return NOT_FOUND
    which = int(found[3])
    if found[2] == "feeds" and which in account["feeds"]:
        if verb == "DELETE":
            del account["feeds"][which]
            return (JSON, 204, b"")
        title, category = account["feeds"][which]
        change = json.loads(body)
        account["feeds"][which] = (change.get("title", title), change.get("category_id", category))
        return as_json({"id": which}, 201)
    if found[2] == "categories" and which in account["categories"]:
        if verb == "DELETE":
            del account["categories"][which]
            account["feeds"] = {i: f for i, f in account["feeds"].items() if f[1] != which}
            return (JSON, 204, b"")
        account["categories"][which] = json.loads(body)["title"]
        return as_json({"id": which, "title": account["categories"][which]}, 201)
    return GONE


def freshrss_get(route):
    account = ACCOUNTS["freshrss"]
    label = lambda name: {"id": "user/-/label/" + name, "label": name}
    if route == "/subscription/list":
        return as_json(
            {
                "subscriptions": [
                    {"id": "feed/%d" % i, "title": t, "url": "", "categories": [label(c)]}
                    for i, (t, c) in account["feeds"].items()
                ]
            }
        )
    if route == "/unread-count":
        return as_json({"max": 3, "unreadcounts": [{"id": "feed/%d" % i, "count": 3} for i in account["feeds"]]})
    if route == "/stream/items/ids":
        return as_json({"itemRefs": []})
    if route == "/tag/list":
        state = [{"id": "user/-/state/com.google/starred"}]
        return as_json({"tags": state + [{"id": "user/-/label/" + n, "type": "folder"} for n in account["labels"]]})
    if route == "/token":
        return (TEXT, 200, b"token\n")
    return NOT_FOUND


def freshrss_change(route, query, body):
    account = ACCOUNTS["freshrss"]
    form = {**parse_qs(query), **parse_qs(body.decode())}
    one = lambda key: form.get(key, [""])[0]
    name = lambda key: one(key).rsplit("/label/", 1)[-1]
    feed = int(one("s").split("/")[1]) if one("s").startswith("feed/") else None
    if route == "/subscription/quickadd":
        url = one("quickadd")
        if url == TAKEN_FEEDS:
            return as_json({"numResults": 0, "error": "Already subscribed! " + url})
        if "://" not in url:
            return as_json({"numResults": 0, "error": "`%s` is not a valid URL" % url})
        if url.endswith(".html"):
            reason = "A feed could not be found at `%s`; the status code is `200` and content-type is `text/html` [%s]"
            return as_json({"numResults": 0, "error": reason % (url, url)})
        if ".invalid" in url:
            return as_json({"numResults": 0, "error": "cURL error 6: Could not resolve host: nohost.invalid [%s]" % url})
        new, account["next"] = account["next"], account["next"] + 1
        account["feeds"][new] = ("New feed", "Uncategorized")
        return as_json({"numResults": 1, "query": url, "streamId": "feed/%d" % new, "streamName": "New feed"})
    if route == "/subscription/edit":
        if one("ac") == "unsubscribe" and feed in account["feeds"]:
            del account["feeds"][feed]
        elif one("ac") == "edit" and feed in account["feeds"]:
            title, label = account["feeds"][feed]
            if one("a"):
                label = name("a")
                if label not in account["labels"]:
                    account["labels"].append(label)
            account["feeds"][feed] = (one("t") or title, label)
        return OK
    if route == "/rename-tag":
        old, new = name("s"), name("dest")
        if old not in account["labels"]:
            return (TEXT, 400, b"Bad Request!")
        if new not in account["labels"] and old != "Uncategorized":
            account["labels"][account["labels"].index(old)] = new
            account["feeds"] = {i: (t, new if c == old else c) for i, (t, c) in account["feeds"].items()}
        return OK
    if route == "/disable-tag":
        old = name("s")
        if old not in account["labels"]:
            return (TEXT, 400, b"Bad Request!")
        if old != "Uncategorized":
            account["labels"].remove(old)
            account["feeds"] = {i: (t, "Uncategorized" if c == old else c) for i, (t, c) in account["feeds"].items()}
        return OK
    return NOT_FOUND


class Fake(BaseHTTPRequestHandler):
    def stateful(self, verb, body):
        """The accounts that hold state, or `None` for any other path."""
        path = urlsplit(self.path)
        if path.path == "/reset":
            ACCOUNTS.update(fresh())
            READECK_POLLS.clear()
            return OK
        if path.path.startswith("/readeck-"):
            name, _, route = path.path[len("/readeck-"):].partition("/")
            return readeck(name, "/" + route, verb)
        if path.path.startswith("/feeds-miniflux"):
            route = path.path[len("/feeds-miniflux"):]
            return miniflux_get(route) if verb == "GET" else miniflux_change(verb, route, body)
        reader = "/feeds-freshrss/api/greader.php"
        if path.path == reader + "/accounts/ClientLogin":
            return LOGIN
        if path.path.startswith(reader + READER):
            route = path.path[len(reader + READER):]
            return freshrss_get(route) if verb == "GET" else freshrss_change(route, path.query, body)
        return None

    def respond(self, kind, status, body):
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def answer(self, routes, verb="GET", body=b""):
        held = self.stateful(verb, body)
        if held is not None:
            return self.respond(*held)
        if self.path.startswith("/gate/"):
            self.send_response(302)
            self.send_header("Location", "http://localhost:18399/sign-in")
            self.end_headers()
            return
        self.respond(*routes.get(self.path.split("?")[0], NOT_FOUND))

    def do_GET(self):
        self.answer(GET)

    def do_POST(self):
        self.answer(POST, "POST", self.rfile.read(int(self.headers.get("Content-Length", 0))))

    def do_PUT(self):
        self.answer(PUT, "PUT", self.rfile.read(int(self.headers.get("Content-Length", 0))))

    def do_DELETE(self):
        self.answer({}, "DELETE")

    def log_message(self, *_):
        pass


HTTPServer(("127.0.0.1", 18399), Fake).serve_forever()
