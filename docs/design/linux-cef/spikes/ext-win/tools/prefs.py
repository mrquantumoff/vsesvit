#!/usr/bin/env python3
"""prefs.py <profile-dir> [ids-json]: the extension state Chromium wrote for one profile.

Reads <profile-dir>/Preferences and <profile-dir>/Secure Preferences (Windows keeps
extensions.settings in the second, MAC-protected one) and prints one line per extension:
name-or-id, location, state, disable_reasons, version, path, ack_external, incognito.
Also external_uninstalls and install.initiallist."""
import json, os, sys

d = sys.argv[1]
names = {}
if len(sys.argv) > 2 and os.path.exists(sys.argv[2]):
    names = {v: k for k, v in json.load(open(sys.argv[2])).items()}

def load(name):
    p = os.path.join(d, name)
    try:
        return json.load(open(p, encoding="utf-8"))
    except Exception:
        return {}

pref, secure = load("Preferences"), load("Secure Preferences")
settings = {}
for src, doc in (("P", pref), ("S", secure)):
    for k, v in doc.get("extensions", {}).get("settings", {}).items():
        settings.setdefault(k, {}).update(v)
        settings[k]["_file"] = settings[k].get("_file", "") + src
for k, v in sorted(settings.items()):
    if v.get("location") == 5:  # component extensions
        continue
    man = v.get("manifest", {}) or {}
    print("ext {:<6} {} loc={} state={} dr={} ver={} path={} ack={} incog={} from_webstore={} file={}".format(
        names.get(k, k[:8]), k[:8], v.get("location"), v.get("state"), v.get("disable_reasons"),
        man.get("version") or v.get("version"), (v.get("path") or "")[-40:], v.get("ack_external"),
        v.get("incognito"), v.get("from_webstore"), v.get("_file")))
ext = {**pref.get("extensions", {}), **secure.get("extensions", {})}
print("external_uninstalls:", ext.get("external_uninstalls"))
print("install.initiallist:", ext.get("install", {}).get("initiallist"))
print("developer_mode:", ext.get("ui", {}).get("developer_mode"))
print("safebrowsing.enhanced:", {**pref.get("safebrowsing", {}), **secure.get("safebrowsing", {})}.get("enhanced"))
print("protection.macs present:", "protection" in secure or "protection" in pref)
