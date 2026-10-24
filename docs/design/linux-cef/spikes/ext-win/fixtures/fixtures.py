#!/usr/bin/env python3
"""Build every WX.0 test extension, CRX and fixture site file. Run in WSL with -I:

  python3 -I fixtures.py <spike-dir> <out-dir> <mkcrx-binary>

Keys are throwaway RSA-2048 (made once, never committed). Output in <out-dir>:
  keys/<K>.pem, exts/<name>/<version>/ (unpacked, manifest "key" set), crx/<name>-<version>.crx,
  site/ (fixture pages + updates/D-<v>.xml manifests), ids.json, ids.env
Extensions (names are the keys):
  H  Vsesvit Helper 1.0            management; bridge.html (design §5.3)
  C  Reporter C 1.0, 2.0           plain packed extension (no update_url)
  D  Reporter D 1.0, 2.0, 3.0      update_url http://upd.vsesvit.test/updates/D.xml; 3.0 adds tabs+history (permission increase)
  E  Reporter E 1.0                only profile B has it (conflicting sets)
  S  Vsesvit Spy 1.0               tabs, notifications, cookies; popup, options, optional permissions
"""
import base64, hashlib, json, os, shutil, subprocess, sys

spike, out, mkcrx = sys.argv[1], sys.argv[2], sys.argv[3]
src = os.path.join(spike, "exts-src")
os.makedirs(f"{out}/keys", exist_ok=True)

def key(name):
    p = f"{out}/keys/{name}.pem"
    if not os.path.exists(p):
        subprocess.run(["openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", p], check=True, capture_output=True)
    der = subprocess.run(["openssl", "pkey", "-in", p, "-pubout", "-outform", "DER"], check=True, capture_output=True).stdout
    return p, der

def ext_id(der):
    return "".join(chr(ord("a") + int(c, 16)) for c in hashlib.sha256(der).hexdigest()[:32])

ids = {}

def build(name, version, src_dir, manifest):
    pem, der = key(name)
    manifest = dict(manifest, version=version, key=base64.b64encode(der).decode())
    i = ext_id(der)
    ids[name] = i
    d = f"{out}/exts/{name}/{version}"
    if os.path.exists(d):
        shutil.rmtree(d)
    shutil.copytree(src_dir, d, ignore=shutil.ignore_patterns("manifest.json", "_metadata"))
    with open(f"{d}/manifest.json", "w") as f:
        json.dump(manifest, f, indent=2)
    os.makedirs(f"{out}/crx", exist_ok=True)
    subprocess.run([mkcrx, pem, d, f"{out}/crx/{name}-{version}.crx"], check=True, capture_output=True)

HOSTS = ["http://*.vsesvit.test/*"]

def reporter(name, extra=None):
    m = {
        "manifest_version": 3, "name": f"Vsesvit Reporter {name}",
        "description": "Reports what the extension sees to the spike's report server.",
        "permissions": ["storage"], "host_permissions": HOSTS,
        "background": {"service_worker": "background.js"},
        "content_scripts": [{"matches": ["http://site.vsesvit.test/*"], "js": ["content.js"], "run_at": "document_end"}],
        "action": {"default_title": f"Reporter {name}"},
    }
    m.update(extra or {})
    return m

build("H", "1.0", f"{src}/helper", {
    "manifest_version": 3, "name": "Vsesvit Helper", "description": "Vsesvit's own extension-management helper.",
    "permissions": ["management"], "host_permissions": HOSTS, "background": {"service_worker": "background.js"},
})
for v in ["1.0", "2.0"]:
    build("C", v, f"{src}/reporter", reporter("C"))
UPD = {"update_url": "http://upd.vsesvit.test/updates/D.xml"}
build("D", "1.0", f"{src}/reporter", reporter("D", UPD))
build("D", "2.0", f"{src}/reporter", reporter("D", UPD))
build("D", "3.0", f"{src}/reporter", reporter("D", dict(UPD, permissions=["storage", "tabs", "history"])))
build("E", "1.0", f"{src}/reporter", reporter("E"))
build("S", "1.0", f"{src}/spy", {
    "manifest_version": 3, "name": "Vsesvit Spy", "description": "Observes the shell from an extension's point of view.",
    "permissions": ["tabs", "storage", "notifications", "cookies"], "host_permissions": HOSTS,
    "optional_host_permissions": ["https://opt.example.org/*"], "optional_permissions": ["bookmarks"],
    "background": {"service_worker": "background.js"},
    "action": {"default_title": "Spy", "default_popup": "popup.html"}, "options_page": "options.html",
})

site = f"{out}/site"
if os.path.exists(site):
    shutil.rmtree(site)
shutil.copytree(os.path.join(spike, "fixtures", "site"), site)
os.makedirs(f"{site}/updates", exist_ok=True)
for v in ["1.0", "2.0", "3.0"]:
    shutil.copy(f"{out}/crx/D-{v}.crx", f"{site}/updates/D-{v}.crx")
    with open(f"{site}/updates/D-{v}.xml", "w") as f:
        f.write(f"""<?xml version='1.0' encoding='UTF-8'?>
<gupdate xmlns='http://www.google.com/update2/response' protocol='2.0'>
  <app appid='{ids["D"]}'><updatecheck codebase='http://upd.vsesvit.test/updates/D-{v}.crx' version='{v}' /></app>
</gupdate>
""")
shutil.copy(f"{site}/updates/D-1.0.xml", f"{site}/updates/D.xml")
with open(f"{out}/ids.json", "w") as f:
    json.dump(ids, f, indent=2)
with open(f"{out}/ids.env", "w") as f:
    for k, v in ids.items():
        f.write(f"{k}={v}\n")
print(json.dumps(ids))
