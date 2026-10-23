"""Fixture server for spike-ext-api: serves tests/fixtures/site, logs every request path
(hits.log), and collects JSON POSTed to /report (reports.log). Needs VSESVIT_REPO and SPIKE_ENV
(source ../env.sh). Missing *.png -> a 1x1 PNG
(dnr.html asks for /dnr/<run>/<kind>.png). Run with python3 -I."""
import http.server, json, os, sys, time, threading
SITE = os.path.join(os.environ["VSESVIT_REPO"], "tests/fixtures/site")
OUT = os.environ["SPIKE_ENV"]
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8793
PNG = open(os.path.join(SITE, "allowed.png"), "rb").read()
lock = threading.Lock()
def log(name, line):
    with lock, open(os.path.join(OUT, name), "a") as f:
        f.write(f"{time.time():.3f} {line}\n")
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k):
        super().__init__(*a, directory=SITE, **k)
    def log_message(self, fmt, *args):
        pass
    def do_GET(self):
        log("hits.log", f"GET {self.path} host={self.headers.get('Host')} origin={self.headers.get('Origin')}")
        path = self.path.split("?")[0]
        if path == "/set-cookie":
            self.send_response(200); self.send_header("Set-Cookie", "served=1; Path=/"); self.send_header("Content-Length", "2"); self.end_headers(); self.wfile.write(b"ok"); return
        if path == "/auth":  # identity.launchWebAuthFlow target: redirects to the extension's chromiumapp.org URL
            q = self.path.split("?", 1)[1] if "?" in self.path else ""
            params = dict(p.split("=", 1) for p in q.split("&") if "=" in p)
            body = f"<html><head><title>auth page</title></head><body><a id=go href='{params.get('redirect_uri','')}#token=abc'>go</a></body></html>".encode()
            self.send_response(200); self.send_header("Content-Type", "text/html"); self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body); return
        if path.endswith(".png") and not os.path.exists(os.path.join(SITE, path.lstrip("/"))):
            self.send_response(200); self.send_header("Content-Type", "image/png"); self.send_header("Content-Length", str(len(PNG))); self.end_headers(); self.wfile.write(PNG); return
        return super().do_GET()
    def do_POST(self):
        n = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(n).decode("utf-8", "replace")
        log("reports.log", f"POST {self.path} origin={self.headers.get('Origin')} {body}")
        self.send_response(204); self.send_header("Access-Control-Allow-Origin", "*"); self.end_headers()
    def do_OPTIONS(self):
        self.send_response(204); self.send_header("Access-Control-Allow-Origin", "*"); self.send_header("Access-Control-Allow-Headers", "*"); self.end_headers()
http.server.ThreadingHTTPServer.allow_reuse_address = True
http.server.ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
