"""ids.py DIR1,DIR2,...: print {"<dir name>": {"id": ..., "path": ...}} for --load-extension dirs.
The id comes from the manifest "key" (SPKI DER, base64) when there is one, else from the absolute
path, as Chromium derives it (crx_file::id_util). harness/cdp.mjs reads the result as ids.json."""
import base64, hashlib, json, os, sys

def to_id(data):
    return "".join(chr(ord("a") + int(c, 16)) for c in hashlib.sha256(data).hexdigest()[:32])

out = {}
for d in sys.argv[1].split(","):
    d = os.path.abspath(d)
    key = json.load(open(os.path.join(d, "manifest.json"))).get("key")
    out[os.path.basename(d)] = {"id": to_id(base64.b64decode(key) if key else d.encode()), "path": d}
print(json.dumps(out, indent=1))
