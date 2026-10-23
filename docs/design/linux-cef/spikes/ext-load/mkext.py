#!/usr/bin/env python3
"""Lay out a test extension the way vsesvit-core installs one: an immutable directory
<exts>/<id>/<version>/ whose manifest.json carries the "key" (base64 SPKI DER), so the
Chromium id is derived from the key, not from the path.

  mkext.py reporter NAME VERSION KEYNAME [JSON-manifest-overrides]
  mkext.py agent VERSION
  mkext.py fixture NAME            ($VSESVIT_REPO/tests/fixtures/extensions/NAME, key keys/NAME.pem)
  mkext.py id KEYNAME
Prints "<id> <dir>".
"""
import base64, hashlib, json, os, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
E = os.path.join(os.environ.get("VSESVIT_SCRATCH") or os.path.expanduser("~/vsesvit-scratch"), "extload-env")
EXTS = os.environ.get("EXTS", E + "/exts")
FIXTURES = os.path.join(os.environ["VSESVIT_REPO"], "tests/fixtures/extensions") if os.environ.get("VSESVIT_REPO") else None

def der(keyname):
    return subprocess.run(["openssl", "pkey", "-in", f"{E}/keys/{keyname}.pem", "-pubout", "-outform", "DER"],
                          check=True, capture_output=True).stdout

def ext_id(d):
    return "".join(chr(ord("a") + int(c, 16)) for c in hashlib.sha256(d).hexdigest()[:32])

def write(src_dir, manifest, keyname):
    d = der(keyname)
    manifest["key"] = base64.b64encode(d).decode()
    i = ext_id(d)
    out = f"{EXTS}/{i}/{manifest['version']}"
    if os.path.exists(out):
        shutil.rmtree(out)
    shutil.copytree(src_dir, out, ignore=shutil.ignore_patterns("manifest.json", "_metadata"))
    with open(f"{out}/manifest.json", "w") as f:
        json.dump(manifest, f, indent=2)
    print(i, out)

cmd = sys.argv[1]
if cmd == "reporter":
    name, version, keyname = sys.argv[2:5]
    m = {
        "manifest_version": 3, "name": f"Vsesvit Reporter {name}", "version": version,
        "description": "Reports what the extension sees to the spike's report server.",
        "permissions": ["storage"], "host_permissions": ["http://127.0.0.1/*"],
        "background": {"service_worker": "background.js"},
        "content_scripts": [{"matches": ["http://127.0.0.1/*"], "js": ["content.js"], "run_at": "document_end"}],
        "commands": {"rep-cmd": {"suggested_key": {"default": "Alt+Shift+Y"}, "description": f"Reporter {name} command"}},
        "action": {"default_title": f"Reporter {name}"},
    }
    if len(sys.argv) > 5:
        m.update(json.loads(sys.argv[5]))
    write(f"{HERE}/exts-src/reporter", m, keyname)
elif cmd == "agent":
    m = {"manifest_version": 3, "name": "Vsesvit Agent", "version": sys.argv[2],
         "description": "Vsesvit's own helper: chrome.management on behalf of the shell.",
         "permissions": ["management", "storage"], "host_permissions": ["http://127.0.0.1/*"],
         "background": {"service_worker": "background.js"}}
    write(f"{HERE}/exts-src/agent", m, "agent")
elif cmd == "fixture":
    name = sys.argv[2]
    src = f"{FIXTURES}/{name}"
    m = json.load(open(f"{src}/manifest.json"))
    m.pop("browser_specific_settings", None)
    write(src, m, name)
elif cmd == "id":
    print(ext_id(der(sys.argv[2])))
