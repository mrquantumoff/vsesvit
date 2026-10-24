#!/usr/bin/env python3
"""WX.0 items 2-11 on one OS, with the same checks on both:

  python3 items.py linux|win [item ...]      (items: 2 3 4 5 6 7 8 9 10 11; default all)

Linux runs inside WSL (spike under the private Xvfb, route = External Extensions JSON);
Windows runs natively (spike on the private desktop through deskrun, sandboxed bundle,
route = per-process HKCU override holding Software\\Google\\Chrome\\Extensions). Every step
prints a line; the profile logs and report files under the scratch runs/ dir are the record.
Profiles A and B always run together, with conflicting sets (A: H, C 1.0, D 1.0, S;
B: H, C 2.0, E, S)."""
import json, os, re, shutil, socket, subprocess, sys, time

OS = sys.argv[1]
WANT = [int(a) for a in sys.argv[2:] if a.isdigit()] or list(range(2, 12))
HERE = os.path.dirname(os.path.abspath(__file__))
if OS == "linux":
    XW = os.path.expanduser("~/vsesvit-scratch/ext-win")
    SCRIPTS = f"{XW}/src/linux/scripts"
    PORTS = {"A": 47911, "B": 47912}
    SEP = "/"
else:
    import winreg
    XW = r"C:\Users\mrqua\git\vsesvit\.audit\scratch\ext-win"
    LIB = os.path.join(os.path.dirname(HERE), "win", "scripts", "lib.ps1")
    PORTS = {"A": 47811, "B": 47812}
    SEP = "\\"
FX = os.path.join(XW, "fx")
IDS = json.load(open(os.path.join(FX, "ids.json")))
NAME = {v: k for k, v in IDS.items()}
SETS = {"A": [("H", "1.0"), ("C", "1.0"), ("D", "1.0"), ("S", "1.0")], "B": [("H", "1.0"), ("C", "2.0"), ("E", "1.0"), ("S", "1.0")]}


def out(*a):
    print(" ".join(str(x) for x in a), flush=True)


def trunc(s, n=600):
    s = str(s)
    return s if len(s) <= n else s[:n] + f"…(+{len(s) - n})"


def rundir(p, L):
    return os.path.join(XW, "runs", L, p)


def root(p, L):
    return os.path.join(rundir(p, L), "root")


def c(p, line, timeout=60):
    s = socket.create_connection(("127.0.0.1", PORTS[p]), timeout=10)
    s.settimeout(timeout + 10)
    s.sendall((f"@{timeout} {line}\n").encode())
    ans = s.makefile(encoding="utf-8").readline().rstrip("\n")
    s.close()
    return ans


def val(ans):
    """JSON value of an `OK <json>` reply, else None."""
    if ans.startswith("OK "):
        try:
            return json.loads(ans[3:])
        except Exception:
            return ans[3:]
    return None


# ------------------------------------------------------------------ OS layer

def regkey(p, L):
    return rf"Software\VsesvitSpikeExtWin\{L}-{p}"


def start(p, L, extra=None):
    extra = dict(extra or {})
    if OS == "linux":
        r = subprocess.run(["bash", f"{SCRIPTS}/run.sh", p, L] + [f"{k}={v}" for k, v in extra.items()], capture_output=True, text=True)
        return r.stdout.strip().splitlines()[-1] if r.stdout.strip() else r.stderr.strip()
    extra["SPIKE_REG_OVERRIDE"] = regkey(p, L)
    vars_ = "; ".join(f"{k}='{v}'" for k, v in extra.items())
    return pwsh(f"Start-Profile -P {p} -Label {L} -Vars @{{{vars_}}}")


def pwsh(cmd):
    # Output through a file, never a pipe: deskrun inherits pwsh's handles and would keep a
    # pipe open for as long as the spike runs.
    f = os.path.join(XW, "runs", "pwsh-out.txt")
    os.makedirs(os.path.dirname(f), exist_ok=True)
    with open(f, "w") as o:
        subprocess.run(["pwsh", "-NoProfile", "-Command", f". '{LIB}'; {cmd}"], stdout=o, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
    lines = open(f, encoding="utf-8", errors="replace").read().strip().splitlines()
    return lines[-1] if lines else ""


def stop(p, L):
    if OS == "linux":
        r = subprocess.run(["bash", f"{SCRIPTS}/stop.sh", p, L], capture_output=True, text=True)
    else:
        return pwsh(f"Stop-Profile -P {p} -Label {L}")
    return (r.stdout.strip().splitlines() or [""])[-1]


def archive(p, L, n, v):
    a = os.path.join(rundir(p, L), "archive")
    os.makedirs(a, exist_ok=True)
    dst = os.path.join(a, f"{n}-{v}.crx")
    shutil.copy(os.path.join(FX, "crx", f"{n}-{v}.crx"), dst)
    return dst


def install(p, L, n, v):
    crx = archive(p, L, n, v)
    if OS == "linux":
        d = os.path.join(root(p, L), "External Extensions")
        os.makedirs(d, exist_ok=True)
        json.dump({"external_crx": crx, "external_version": v}, open(os.path.join(d, IDS[n] + ".json"), "w"))
    else:
        k = winreg.CreateKey(winreg.HKEY_CURRENT_USER, regkey(p, L) + rf"\Software\Google\Chrome\Extensions\{IDS[n]}")
        winreg.SetValueEx(k, "path", 0, winreg.REG_SZ, crx)
        winreg.SetValueEx(k, "version", 0, winreg.REG_SZ, v)
        winreg.CloseKey(k)


def remove_entry(p, L, n):
    if OS == "linux":
        f = os.path.join(root(p, L), "External Extensions", IDS[n] + ".json")
        if os.path.exists(f):
            os.remove(f)
    else:
        try:
            winreg.DeleteKey(winreg.HKEY_CURRENT_USER, regkey(p, L) + rf"\Software\Google\Chrome\Extensions\{IDS[n]}")
        except FileNotFoundError:
            pass


def prefs(p, L):
    r = subprocess.run([sys.executable, os.path.join(HERE, "prefs.py"), os.path.join(root(p, L), "Default"), os.path.join(FX, "ids.json")], capture_output=True, text=True)
    return r.stdout


def prefs_file_edit(p, L, fn):
    """Edit the profile's on-disk Preferences (profile not running)."""
    f = os.path.join(root(p, L), "Default", "Preferences")
    d = json.load(open(f, encoding="utf-8"))
    fn(d)
    json.dump(d, open(f, "w", encoding="utf-8"))


def log_lines(p, L):
    try:
        return open(os.path.join(rundir(p, L), "spike.log"), encoding="utf-8", errors="replace").read().splitlines()
    except FileNotFoundError:
        return []


def reports(p, L):
    try:
        return [json.loads(l) for l in open(os.path.join(rundir(p, L), "reports.jsonl"), encoding="utf-8") if l.strip()]
    except FileNotFoundError:
        return []


def mark(p, L):
    return len(log_lines(p, L)), len(reports(p, L))


def since(p, L, m):
    return log_lines(p, L)[m[0]:], reports(p, L)[m[1]:]


def toplevels(p):
    return c(p, "wtops" if OS == "win" else "xtops")


# ------------------------------------------------------------------ extension helpers

LIST_JS = ("chrome.developerPrivate.getExtensionsInfo({includeDisabled:true,includeTerminated:true}).then(a=>a.map(e=>"
           "({n:e.name.split(' ').pop(),v:e.version,state:e.state,loc:e.location,incog:e.incognitoAccess&&e.incognitoAccess.isActive,"
           "dr:Object.keys(e.disableReasons||{}).filter(k=>e.disableReasons[k]),host:e.permissions&&e.permissions.runtimeHostPermissions&&e.permissions.runtimeHostPermissions.hostAccess})))")


def mgmt(p):
    a = c(p, "labels")
    if " mgmt:" not in a and not a.startswith("OK mgmt:"):
        c(p, "hidden mgmt chrome chrome://extensions", 20)
        time.sleep(0.5)


def info(p):
    mgmt(p)
    return val(c(p, f"ev mgmt {LIST_JS}"))


def short(lst):
    return ", ".join(f"{e['n']} {e['v']} {e['state']}{'(' + '+'.join(e['dr']) + ')' if e['dr'] else ''}" for e in (lst or []))


def ack(p, names):
    """Windows route: a registry install arrives disabled (unacknowledged external); one enable
    from chrome://extensions acknowledges it, without a dialog (§1.2)."""
    if OS != "win":
        return
    mgmt(p)
    for n in names:
        c(p, f"ev mgmt chrome.management.setEnabled('{IDS[n]}', true).then(()=>'ok', e=>'ERR '+e.message)")


def bridge(p):
    a = c(p, "labels")
    if "bridge:" not in a:
        return c(p, f"hidden bridge alloy chrome-extension://{IDS['H']}/bridge.html", 20)
    return "already"


def call(p, req):
    return c(p, f"ev bridge vsesvitBridge.call({json.dumps(req)})")


def hlist(p):
    v = val(call(p, {"op": "list"}))
    if not isinstance(v, list):
        return f"ERR {v}"
    return ", ".join(f"{NAME.get(e['id'], e['id'][:6])} {e['version']} {'on' if e['enabled'] else 'off(' + str(e['disabledReason']) + ')'}" for e in sorted(v, key=lambda e: NAME.get(e['id'], 'z')))


def spypage(p, private=False):
    lbl = "spyp" if private else "spy"
    a = c(p, "labels")
    if f"{lbl}:" not in a:
        c(p, f"hidden {lbl} alloy chrome-extension://{IDS['S']}/options.html{' otr' if private else ''}", 20)
    return lbl


def spy(p, expr, private=False, gesture=False):
    lbl = spypage(p, private)
    return c(p, f"{'evg' if gesture else 'ev'} {lbl} {expr}")


def bridge_events(lines):
    return [l.split("bridge-event: ", 1)[1] for l in lines if "bridge-event: " in l]


def fresh(L):
    shutil.rmtree(os.path.join(XW, "runs", L), ignore_errors=True)
    if OS == "win":
        for p in "AB":
            subprocess.run(["reg", "delete", rf"HKCU\{regkey(p, L)}", "/f"], capture_output=True)


def setup(L, profiles="AB", sets=None):
    fresh(L)
    sets = sets or SETS
    for p in profiles:
        for n, v in sets[p]:
            install(p, L, n, v)
    for p in profiles:
        out(f"  start {p}:", start(p, L))
    time.sleep(6)
    for p in profiles:
        ack(p, [n for n, _ in sets[p]])
        out(f"  {p} bridge:", bridge(p))


def stop_all(L, profiles="AB"):
    for p in profiles:
        out(f"  stop {p}:", stop(p, L))


# ------------------------------------------------------------------ items

def item2():
    L = "i2"
    out("ITEM 2: lifecycle on the route, A and B together")
    setup(L)
    for p in "AB":
        out(f"  {p} list:", hlist(p))
    out("  -- restart both")
    stop_all(L)
    for p in "AB":
        out(f"  start {p}:", start(p, L))
    time.sleep(4)
    for p in "AB":
        bridge(p)
        out(f"  {p} list after restart:", hlist(p))
    out("  -- reload: incognito toggle on C (developerPrivate), A")
    mgmt("A")
    out("  A updateExtensionConfiguration C incognito:", c("A", f"ev mgmt chrome.developerPrivate.updateExtensionConfiguration({{extensionId:'{IDS['C']}', incognitoAccess:true}}).then(()=>'ok',e=>'ERR '+e.message)"))
    time.sleep(2)
    out("  A info after toggle:", short(info("A")))
    out("  -- enable/disable through the helper, A: D off")
    m = mark("A", L)
    out("  A setEnabled D false:", call("A", {"op": "setEnabled", "id": IDS["D"], "enabled": False}))
    time.sleep(1)
    out("  A bridge events:", bridge_events(since("A", L, m)[0]))
    out("  A list:", hlist("A"))
    out("  -- restart A (D must stay off, C must stay on with incognito)")
    out("  stop A:", stop("A", L))
    out("  start A:", start("A", L))
    time.sleep(4)
    bridge("A")
    out("  A list:", hlist("A"))
    out("  A info:", short(info("A")))
    m = mark("A", L)
    out("  A setEnabled D true:", call("A", {"op": "setEnabled", "id": IDS["D"], "enabled": True}))
    time.sleep(1)
    out("  A bridge events:", bridge_events(since("A", L, m)[0]))
    out("  -- uninstallSelf through a hidden Alloy manifest.json page, B: E")
    m = mark("B", L)
    tops_before = toplevels("B")
    out("  B hidden u:", c("B", f"hidden u alloy chrome-extension://{IDS['E']}/manifest.json", 20))
    out("  B uninstallSelf:", c("B", "ev u chrome.management.uninstallSelf({showConfirmDialog:false})", 6))
    time.sleep(2)
    out("  B bridge events:", bridge_events(since("B", L, m)[0]))
    out("  B new toplevels:", toplevels("B") != tops_before, trunc(toplevels("B"), 300) if toplevels("B") != tops_before else "")
    out("  B list:", hlist("B"))
    remove_entry("B", L, "E")
    out("  stop B:", stop("B", L))
    out("  B prefs:", prefs("B", L).replace("\n", " | "))
    out("  -- entry back without editing external_uninstalls: E must stay out")
    install("B", L, "E", "1.0")
    out("  start B:", start("B", L))
    time.sleep(5)
    bridge("B")
    out("  B list:", hlist("B"), "|", short(info("B")))
    out("  stop B:", stop("B", L))
    out("  -- drop E from external_uninstalls in Preferences before start")
    prefs_file_edit("B", L, lambda d: d.setdefault("extensions", {}).__setitem__("external_uninstalls", [x for x in d.get("extensions", {}).get("external_uninstalls", []) if x != IDS["E"]]))
    out("  start B:", start("B", L))
    time.sleep(5)
    ack("B", ["E"])
    bridge("B")
    out("  B list:", hlist("B"))
    out("  B E reports:", [ (r.get("event"), r.get("reason"), r.get("localBefore")) for r in reports("B", L) if r.get("ext") == "E"][-3:])
    out("  B prefs:", prefs("B", L).replace("\n", " | "))
    out("  -- store update then restart, A: D 1.0 -> 2.0 offered by A's server only")
    shutil.copy(os.path.join(rundir("A", L), "site", "updates", "D-2.0.xml"), os.path.join(rundir("A", L), "site", "updates", "D.xml"))
    out("  A hidden dpage:", c("A", f"hidden dpage alloy chrome-extension://{IDS['D']}/manifest.json", 20))
    out("  A requestUpdateCheck:", c("A", "ev dpage chrome.runtime.requestUpdateCheck()", 30))
    c("A", "closeh dpage")
    time.sleep(3)
    out("  stop A:", stop("A", L))
    out("  start A:", start("A", L))
    time.sleep(5)
    bridge("A")
    out("  A list (D must be 2.0, entry still says 1.0):", hlist("A"))
    out("  A D reports:", [(r.get("event"), r.get("version"), r.get("reason"), r.get("previousVersion")) for r in reports("A", L) if r.get("ext") == "D"][-4:])
    bridge("B")
    out("  B list (unaffected):", hlist("B"))
    out("  -- permission-raising update, A: D 3.0 adds tabs+history")
    shutil.copy(os.path.join(rundir("A", L), "site", "updates", "D-3.0.xml"), os.path.join(rundir("A", L), "site", "updates", "D.xml"))
    tops_before = toplevels("A")
    out("  A hidden dpage:", c("A", f"hidden dpage alloy chrome-extension://{IDS['D']}/manifest.json", 20))
    out("  A requestUpdateCheck:", c("A", "ev dpage chrome.runtime.requestUpdateCheck()", 30))
    c("A", "closeh dpage")
    time.sleep(3)
    out("  stop A:", stop("A", L))
    m = (0, 0)
    out("  start A:", start("A", L))
    time.sleep(6)
    bridge("A")
    out("  A list:", hlist("A"))
    out("  A info:", short(info("A")))
    tl = toplevels("A")
    out("  A toplevels:", trunc(tl, 900))
    out("  A windows created/mapped since start:", [l for l in log_lines("A", L)[-400:] if ("winevent: show" in l and "top=true" in l) or "rootwatch: MapNotify" in l][-8:])
    m = mark("A", L)
    out("  A helper setEnabled D true (no gesture):", call("A", {"op": "setEnabled", "id": IDS["D"], "enabled": True}))
    out("  A mgmt setEnabled D true:", c("A", f"ev mgmt chrome.management.setEnabled('{IDS['D']}', true).then(()=>'ok', e=>'ERR '+e.message)", 10))
    out("  A mgmt setEnabled D true (gesture):", c("A", f"evg mgmt chrome.management.setEnabled('{IDS['D']}', true).then(()=>'ok', e=>'ERR '+e.message)", 10))
    time.sleep(2)
    out("  A toplevels after re-enable attempts:", trunc(toplevels("A"), 900))
    out("  A info:", short(info("A")))
    stop_all(L)
    for p in "AB":
        out(f"  {p} final prefs:", prefs(p, L).replace("\n", " | "))


def item3():
    L = "i3"
    out("ITEM 3: the helper as an external CRX with Enhanced Safe Browsing on")
    setup(L, "A")
    out("  A setpref safebrowsing.enhanced true:", c("A", "setpref safebrowsing.enhanced true"))
    out("  A pref:", c("A", "pref safebrowsing.enhanced"))
    out("  stop A:", stop("A", L))
    n0 = len([r for r in reports("A", L) if r.get("ext") == "helper"])
    out("  start A (ESB on):", start("A", L))
    time.sleep(5)
    out("  A pref after restart:", c("A", "pref safebrowsing.enhanced"))
    out("  A helper sw-start reports since restart:", len([r for r in reports("A", L) if r.get("ext") == "helper"]) - n0)
    out("  A bridge:", bridge("A"))
    out("  A ping:", call("A", {"op": "ping"}))
    out("  A list:", hlist("A"))
    stop_all(L, "A")


def item4():
    L = "i4"
    out("ITEM 4: the page-session bridge")
    setup(L, "A")
    out("  A ping:", call("A", {"op": "ping"}))
    t = time.time()
    for i in range(20):
        call("A", {"op": "ping"})
    out(f"  20 ping round trips over ctl+page session: {(time.time() - t) * 1000 / 20:.1f} ms each")
    m = mark("A", L)
    out("  emitTest:", call("A", {"op": "emitTest", "n": 7}))
    out("  setEnabled C false/true:", call("A", {"op": "setEnabled", "id": IDS["C"], "enabled": False}), call("A", {"op": "setEnabled", "id": IDS["C"], "enabled": True}))
    time.sleep(1)
    lines = since("A", L, m)[0]
    out("  bridge events:", bridge_events(lines))
    out("  a console line from another browser (tab):", c("A", "ev active console.debug('vsesvit-bridge {\"forged\":1}'), 1"))
    time.sleep(0.5)
    lines = since("A", L, m)[0]
    out("  forged line parsed as bridge-event?:", any("forged" in l for l in lines if "bridge-event:" in l), "| logged as:", [l for l in lines if "forged" in l][-1:])
    out("  bridge page closed and recreated:", c("A", "closeh bridge"), time.sleep(1.5) or bridge("A"), call("A", {"op": "ping"}))
    stop_all(L, "A")


def item5():
    L = "i5"
    out("ITEM 5: developerPrivate over a hidden Chrome-style chrome://extensions page; hidden browsers seen by extensions")
    setup(L)
    for p in "AB":
        m = mark(p, L)
        mgmt(p)
        out(f"  {p} getExtensionsInfo:", short(info(p)))
        out(f"  {p} updateExtensionConfiguration C hostAccess ON_CLICK:", c(p, f"ev mgmt chrome.developerPrivate.updateExtensionConfiguration({{extensionId:'{IDS['C']}', hostAccess:'ON_CLICK'}}).then(()=>'ok',e=>'ERR '+e.message)"))
        out(f"  {p} updateExtensionConfiguration S incognito:", c(p, f"ev mgmt chrome.developerPrivate.updateExtensionConfiguration({{extensionId:'{IDS['S']}', incognitoAccess:true}}).then(()=>'ok',e=>'ERR '+e.message)"))
        out(f"  {p} info after:", json.dumps([e for e in info(p) if e['n'] in ('C', 'Spy')]))
        c(p, "hidden maint alloy about:blank", 20)
        c(p, f"hidden u alloy chrome-extension://{IDS['C']}/manifest.json", 20)
        time.sleep(1)
        out(f"  {p} labels:", c(p, "labels"))
        snap = val(spy(p, "spy.snapshot()"))
        out(f"  {p} spy tabs.query({{}}) urls:", [t['url'] for t in (snap or {}).get('tabs', [])])
        out(f"  {p} spy windows.getAll types:", [(w['type'], w['tabs']) for w in (snap or {}).get('windows', [])])
        out(f"  {p} spy tabs.onCreated/windows.onCreated seen:", [(r.get('what'), r.get('url')) for r in since(p, L, m)[1] if r.get('event') == 'tabev'])
        c(p, "closeh mgmt")
        time.sleep(1)
        out(f"  {p} after closing mgmt, spy events:", [(r.get('what'), r.get('url')) for r in since(p, L, m)[1] if r.get('event') == 'tabev'][-3:])
    stop_all(L)
    for p in "AB":
        out(f"  {p} prefs:", prefs(p, L).replace("\n", " | "))


def item6():
    L = "i6"
    out("ITEM 6: extension popup (Alloy) in an AuxHost above the content host")
    setup(L, "A")
    m = mark("A", L)
    out("  popup:", c("A", f"popup chrome-extension://{IDS['S']}/popup.html"))
    time.sleep(2.5)
    out("  popupinfo:", c("A", "popupinfo"))
    if OS == "win":
        out("  popupprobe:", c("A", "popupprobe"))
    out("  popup view:", c("A", "ev popup window.spyPopup"))
    out("  popupfocus:", c("A", "popupfocus"))
    time.sleep(0.5)
    out("  popup document.hasFocus:", c("A", "ev popup document.hasFocus()"))
    out("  activeinfo:", c("A", "activeinfo"))
    if OS == "win":
        hw = re.search(r"popup_child=(0x[0-9a-f]+)", c("A", "popupprobe"))
        if hw:
            c("A", "ev popup document.getElementById('field').focus(), 1")
            out("  key posted to the popup's HWND:", c("A", f"wkey {hw.group(1)} x"))
    time.sleep(1)
    out("  popup reports:", [(r.get('event'), r.get('activeCurrent') or r.get('key') or r.get('hasFocus')) for r in since("A", L, m)[1] if r.get('event', '').startswith('popup')])
    out("  window.close():", c("A", "ev popup window.close(), 1"))
    time.sleep(1)
    out("  closed:", [l for l in since("A", L, m)[0] if "popup closed" in l or "client=auxpopup" in l and "on_before_close" in l][-2:])
    stop_all(L, "A")


FOREIGN_CASES = [
    ("tabs.create", "spy.open('tab','http://site.vsesvit.test/page2.html')"),
    ("windows.create", "spy.open('win','http://site.vsesvit.test/index.html')"),
    ("openOptionsPage", "spy.open('options')"),
    ("slow url (1.5 s)", "spy.open('tab','http://site.vsesvit.test/slow?ms=1500&to=page2.html')"),
    ("explicit about:blank", "spy.open('tab','about:blank')"),
    ("incognito window", "spy.open('win','http://site.vsesvit.test/index.html',{incognito:true})"),
]


def item7():
    L = "i7"
    out("ITEM 7: Chrome-created windows: neutralize, focus, close-and-reopen")
    setup(L, "A")
    mgmt("A")
    out("  allow S in incognito:", c("A", f"ev mgmt chrome.developerPrivate.updateExtensionConfiguration({{extensionId:'{IDS['S']}', incognitoAccess:true}}).then(()=>'ok',e=>'ERR '+e.message)"))
    time.sleep(1.5)
    c("A", "closeh mgmt")
    out("  activation after setup (hidden Chrome-style management page used):", c("A", "activeinfo"))
    out("  a shell private tab first (so the shell's private context exists):", c("A", "newp about:blank"))
    time.sleep(1)
    spypage("A")
    for name, js in FOREIGN_CASES:
        m = mark("A", L)
        before = c("A", "activeinfo")
        r = spy("A", js)
        time.sleep(4)
        lines, reps = since("A", L, m)
        fl = [l for l in lines if "foreign:" in l or "gpui: open tab" in l]
        shows = [l for l in lines if ("winevent: show" in l and "top=true" in l) or "rootwatch: MapNotify" in l or "rootwatch: X focus" in l]
        out(f"  [{name}] spy -> {trunc(r, 200)}")
        for l in fl:
            out("     ", trunc(l.split("] ", 1)[-1], 400))
        out("      toplevel show/map/focus events:", [trunc(l.split('] ', 1)[-1], 160) for l in shows])
        out("      activeinfo before/after:", before, "|", c("A", "activeinfo"))
        out("      spy events:", [(x.get('what'), x.get('url'), x.get('incognito')) for x in reps if x.get('event') == 'tabev'])
    out("  tabs now:", c("A", "tabs"))
    out("  labels:", c("A", "labels"))
    out("  -- baseline without neutralize (SPIKE_FOREIGN=none)")
    out("  stop A:", stop("A", L))
    out("  start A:", start("A", L, {"SPIKE_FOREIGN": "none"}))
    time.sleep(4)
    m = mark("A", L)
    before = c("A", "activeinfo")
    r = spy("A", "spy.open('tab','http://site.vsesvit.test/page2.html')")
    time.sleep(4)
    lines, _ = since("A", L, m)
    out("  [baseline tabs.create]", trunc(r, 200))
    out("      ", [trunc(l.split('] ', 1)[-1], 200) for l in lines if "foreign:" in l or ("winevent: show" in l and "top=true" in l) or "rootwatch: MapNotify" in l or "winevent: focus" in l or "rootwatch: X focus" in l][:10])
    out("      activeinfo before/after:", before, "|", c("A", "activeinfo"))
    stop_all(L, "A")


SHIM = r"""(() => {
  const s = { t: performance.now(), scripts: document.scripts.length, readyState: document.readyState, href: location.href,
              webstorePrivate: (typeof chrome !== 'undefined' && chrome.webstorePrivate) ? Object.keys(chrome.webstorePrivate).length : -1 };
  if (s.webstorePrivate > 0) {
    chrome.webstorePrivate.beginInstallWithManifest3 = function () { console.log('vs-shim intercepted beginInstallWithManifest3'); return Promise.reject(new Error('vsesvit shim')); };
    s.wrapped = true;
  }
  window.__vsShim = s;
  console.log('vs-shim ran ' + JSON.stringify(s));
})();"""


def item8():
    L = "i8"
    out("ITEM 8: the Web Store shim runs before the store's own scripts (Chrome-style child)")
    setup(L, "A")
    for url in ["http://site.vsesvit.test/store.html", f"https://chromewebstore.google.com/detail/{IDS['S']}", "https://chromewebstore.google.com/detail/lmjegmlicamnimmfhcmpkclmigmmcbeh"]:
        m = mark("A", L)
        c("A", "new about:blank")
        time.sleep(2)
        tabs = c("A", "tabs")
        b = re.findall(r":(b\d+):", tabs)[-1]
        out(f"  [{url}] tab {b}: Page.enable:", c("A", f"dt {b} Page.enable {{}}"))
        out(f"  [{url}] tab {b}: add script:", c("A", f"dt {b} Page.addScriptToEvaluateOnNewDocument {json.dumps({'source': SHIM})}"))
        out("    navigate:", c("A", f"ev {b} location.href={json.dumps(url)}"))
        time.sleep(7)
        out("    after load __vsShim:", c("A", f"ev {b} JSON.stringify(window.__vsShim)"))
        out("    page saw shim first (fixture):", c("A", f"ev {b} String(window.__pageSawShim)"))
        out("    wrapper intact:", c("A", f"ev {b} String(chrome.webstorePrivate && chrome.webstorePrivate.beginInstallWithManifest3).slice(0,120)"))
        out("    console:", [trunc(l.split('] ', 1)[-1], 300) for l in since("A", L, m)[0] if "vs-shim" in l or "sawShim" in l][:4])
    stop_all(L, "A")


def item9():
    L = "i9"
    out("ITEM 9: extension notifications (show, click, close, per-extension block); permissions.request")
    notifyd = None
    if OS == "linux":
        nlog = os.path.join(XW, "runs", L + "-notifyd.jsonl")
        fifo = os.path.join(XW, "runs", L + "-notifyd.fifo")
        os.makedirs(os.path.dirname(nlog), exist_ok=True)
        for f in (nlog, fifo):
            if os.path.exists(f):
                os.remove(f)
        env = dict(os.environ)
        env.update(dict(l.strip().split("=", 1) for l in open(os.path.join(XW, "dbus.env")) if "=" in l))
        notifyd = subprocess.Popen([sys.executable, os.path.join(HERE, "notifyd.py"), nlog, fifo], env=env)
        time.sleep(1)
    setup(L, "A")
    spypage("A")
    out("  permission level:", spy("A", "spy.permLevel()"))
    m = mark("A", L)
    tops0 = toplevels("A")
    out("  notify n1:", spy("A", "spy.notify('n1','hello from spy')"))
    time.sleep(2)
    out("  getAll:", spy("A", "spy.notifAll()"))
    tops1 = toplevels("A")
    new = [t for t in tops1.split(" || ") if t not in tops0]
    out("  new toplevels:", trunc(new, 900))
    if OS == "linux":
        out("  notifyd calls:", open(nlog).read().strip().splitlines()[-3:])
        nid = [json.loads(l)["id"] for l in open(nlog) if '"Notify"' in l][-1]
        open(fifo, "w").write(f"click {nid}\n")
        time.sleep(1)
        out("  after click: spy reports:", [(r.get('what'), r.get('id'), r.get('byUser')) for r in reports("A", L) if r.get('event') == 'notif'])
        out("  notify n2 then daemon close (user dismissed, reason 2):", spy("A", "spy.notify('n2','second')"))
        time.sleep(1)
        nid = [json.loads(l)["id"] for l in open(nlog) if '"Notify"' in l][-1]
        open(fifo, "w").write(f"close {nid} 2\n")
        time.sleep(1)
    else:
        win = [t for t in new if "Chrome_WidgetWin" in t]
        hw = re.search(r"(0x[0-9a-f]+)", win[0]).group(1) if win else None
        if hw:
            out("  shot of the notification window:", c("A", f"wshot {hw} {os.path.join(rundir('A', L), 'notif.png')}"))
            geo = re.search(r'" (\d+)x(\d+)\+', win[0])
            w, h = (int(geo.group(1)), int(geo.group(2))) if geo else (300, 100)
            out("  click its body:", c("A", f"wclick {hw} {w // 3} {h // 2}"))
            time.sleep(1.5)
        out("  after click: spy reports:", [(r.get('what'), r.get('id'), r.get('byUser')) for r in reports("A", L) if r.get('event') == 'notif'])
        out("  notify n2:", spy("A", "spy.notify('n2','second')"))
        time.sleep(1.5)
        new2 = [t for t in toplevels("A").split(" || ") if "Chrome_WidgetWin" in t and t not in tops0]
        out("  n2 window:", trunc(new2, 400))
        if new2:
            hw2 = re.search(r"(0x[0-9a-f]+)", new2[0]).group(1)
            g2 = re.search(r'" (\d+)x(\d+)\+', new2[0])
            out("  click its close button (x):", c("A", f"wclick {hw2} {int(int(g2.group(1)) * 0.957)} {int(int(g2.group(2)) * 0.09)}"))
            time.sleep(1.5)
    out("  clear n1 from the extension:", spy("A", "spy.clearNotif('n1')"))
    time.sleep(1)
    out("  notif events:", [(r.get('what'), r.get('id'), r.get('byUser')) for r in reports("A", L) if r.get('event') == 'notif'])
    out("  -- per-extension block")
    out("  content setting NOTIFICATIONS block for chrome-extension://S/:", c("A", f"cs chrome-extension://{IDS['S']}/ notifications block"))
    out("  permission level:", spy("A", "spy.permLevel()"))
    out("  notify n3:", spy("A", "spy.notify('n3','after content setting')"))
    out("  setpref message_center.disabled_extension_ids [S]:", c("A", f"setpref message_center.disabled_extension_ids [\"{IDS['S']}\"]"))
    time.sleep(0.5)
    out("  permission level:", spy("A", "spy.permLevel()"))
    out("  notify n4:", spy("A", "spy.notify('n4','after pref')"))
    time.sleep(1)
    out("  getAll:", spy("A", "spy.notifAll()"))
    if OS == "linux":
        out("  notifyd calls since:", [json.loads(l).get("summary") or json.loads(l) for l in open(nlog)][-4:])
    out("  unblock:", c("A", "setpref message_center.disabled_extension_ids []"), spy("A", "spy.permLevel()"))
    if OS == "linux":
        out("  -- notifications without a D-Bus notification server (Chromium's own popups)")
        notifyd.terminate(); notifyd.wait(); time.sleep(1)
        m0 = mark("A", L)
        out("  notify n5:", spy("A", "spy.notify('n5','no daemon')"))
        time.sleep(2)
        out("  maps:", [l.split('] ', 1)[-1] for l in since("A", L, m0)[0] if "MapNotify" in l][:3])
        notifyd = None
    out("  -- permissions.request from the options page in a Chrome-style tab")
    c("A", f"new chrome-extension://{IDS['S']}/options.html")
    time.sleep(2.5)
    b = re.findall(r":(b\d+):", c("A", "tabs"))[-1]
    m = mark("A", L)
    tops0 = toplevels("A")
    out("  request (gesture), answer arrives later:", c("A", f"evg {b} (window.reqP = req('bookmarks'), 'pending')"))
    time.sleep(2.5)
    tops1 = toplevels("A")
    new = [t for t in tops1.split(" || ") if t not in tops0]
    out("  new toplevels (dialog?):", trunc(new, 900))
    out("  window events:", [trunc(l.split('] ', 1)[-1], 200) for l in since("A", L, m)[0] if "winevent: show" in l or "rootwatch: MapNotify" in l or "rootwatch: ReparentNotify" in l][:6])
    if OS == "win":
        dlg = [t for t in new if "Chrome_WidgetWin" in t and " visible " in t]
        if dlg:
            hw = re.search(r"(0x[0-9a-f]+)", dlg[0]).group(1)
            geo = re.search(r'" (\d+)x(\d+)\+', dlg[0])
            w, h = int(geo.group(1)), int(geo.group(2))
            out("  dialog:", dlg[0][:300])
            out("  dialog shot:", c("A", f"wshot {hw} {os.path.join(rundir('A', L), 'perm-dialog.png')}"))
            out("  click Allow (posted to the dialog HWND; Windows order is Allow, Deny):", c("A", f"wclick {hw} {int(w * 0.71)} {int(h * 0.78)}"))
    else:
        maps = [l for l in since("A", L, m)[0] if "rootwatch: MapNotify" in l]
        if maps:
            mm = re.search(r"MapNotify (0x[0-9a-f]+) (\d+)x(\d+)\+(-?\d+)\+(-?\d+)", maps[-1])
            xid, w, h, x, y = mm.group(1), *map(int, mm.groups()[1:])
            env = {"PATH": "/usr/bin:/bin", "DISPLAY": ":94", "SPIKE_XTOKEN": open(os.path.join(XW, "xtoken")).read()}
            xp = subprocess.run([os.path.expanduser("~/vsesvit-scratch/opt/usr/bin/xprop"), "-id", xid, "WM_TRANSIENT_FOR", "_NET_WM_WINDOW_TYPE"], capture_output=True, text=True, env=env).stdout
            out("  dialog props:", xp.strip().replace(chr(10), " | "), "| activeinfo:", c("A", "activeinfo"))
            xc = os.path.join(XW, "target/debug/xctl")
            out("  shot:", subprocess.run([xc, "shot", os.path.join(rundir('A', L), 'perm-dialog.png'), xid], capture_output=True, text=True, env=env).stdout.strip())
            out("  XTEST click on Allow (private Xvfb):", subprocess.run([xc, "click", str(x + int(w * 0.877)), str(y + int(h * 0.81))], capture_output=True, text=True, env=env).stdout.strip())
    time.sleep(2)
    out("  answer reached the extension:", c("A", f"ev {b} window.reqP"), "| perm reports:", [r for r in reports("A", L) if r.get('event') == 'perm'])
    out("  permissions now:", c("A", f"ev {b} has()"))
    out("  -- the same from the options page in the Alloy popup host (host permission)")
    m = mark("A", L)
    tops0 = toplevels("A")
    out("  popup:", c("A", f"popup chrome-extension://{IDS['S']}/options.html"))
    time.sleep(2)
    out("  request (gesture):", c("A", "evg popup (window.reqP = req('host'), 'pending')"))
    time.sleep(2.5)
    new = [t for t in toplevels("A").split(" || ") if t not in tops0]
    out("  new toplevels:", trunc(new, 600))
    out("  window events:", [trunc(l.split('] ', 1)[-1], 200) for l in since("A", L, m)[0] if ("winevent: show" in l and "top=true" in l) or "rootwatch: MapNotify" in l][:4])
    out("  answer (no click, 3 s):", c("A", "ev popup Promise.race([window.reqP, new Promise(r=>setTimeout(()=>r('still pending'),3000))])", 10))
    dlg = [t for t in new if ("Chrome_WidgetWin" in t and " visible " in t) or "depth=24 VIEWABLE" in t]
    if OS == "win" and dlg:
        hw = re.search(r"(0x[0-9a-f]+)", dlg[0]).group(1)
        geo = re.search(r'" (\d+)x(\d+)\+', dlg[0])
        out("  click Allow:", c("A", f"wclick {hw} {int(int(geo.group(1)) * 0.71)} {int(int(geo.group(2)) * 0.78)}"))
        time.sleep(1.5)
        out("  answer:", c("A", "ev popup window.reqP", 5))
    maps = [l for l in since("A", L, m)[0] if "rootwatch: MapNotify" in l]
    if OS == "linux" and maps:
        mm = re.search(r"MapNotify (0x[0-9a-f]+) (\d+)x(\d+)\+(-?\d+)\+(-?\d+)", maps[-1])
        xid, w, h, x, y = mm.group(1), *map(int, mm.groups()[1:])
        env = {"PATH": "/usr/bin:/bin", "DISPLAY": ":94", "SPIKE_XTOKEN": open(os.path.join(XW, "xtoken")).read()}
        subprocess.run([os.path.join(XW, "target/debug/xctl"), "click", str(x + int(w * 0.877)), str(y + int(h * 0.81))], capture_output=True, text=True, env=env)
        time.sleep(1.5)
        out("  XTEST click on Allow; answer:", c("A", "ev popup window.reqP", 5))
    stop_all(L, "A")
    if notifyd:
        notifyd.terminate()


def item10():
    L = "i10"
    out("ITEM 10: extensions in private windows; chrome.cookies.set against a blocked site")
    setup(L, "A")
    mgmt("A")
    for n in ("C", "S"):
        out(f"  allow {n} in incognito:", c("A", f"ev mgmt chrome.developerPrivate.updateExtensionConfiguration({{extensionId:'{IDS[n]}', incognitoAccess:true}}).then(()=>'ok',e=>'ERR '+e.message)"))
    time.sleep(2)
    out("  info:", short(info("A")))
    m = mark("A", L)
    out("  private tab:", c("A", "newp http://site.vsesvit.test/index.html"))
    time.sleep(3)
    b = re.findall(r":(b\d+):", c("A", "tabs"))[-1]
    out("  private tab marks:", c("A", f"ev {b} JSON.stringify(document.documentElement.dataset)"))
    out("  private tab ctx:", c("A", f"ctx {b}"))
    out("  C/S reports from the private context:", [(r.get('ext'), r.get('event'), r.get('incognito')) for r in since("A", L, m)[1] if r.get('incognito')])
    out("  spy snapshot (normal context) sees incognito tab?:", [(t['url'], t['incognito']) for t in (val(spy("A", "spy.snapshot()")) or {}).get('tabs', [])])
    out("  -- cookies: the shell blocks blocked.vsesvit.test through its IO cookie filter")
    out("  cookieblock:", c("A", "cookieblock blocked.vsesvit.test"))
    out("  extension cookies.set:", spy("A", "spy.setCookie('http://blocked.vsesvit.test/','ext','1')"))
    out("  cookie store has it:", c("A", "cookies http://blocked.vsesvit.test/"))
    m = mark("A", L)
    c("A", "new http://blocked.vsesvit.test/echo")
    time.sleep(2.5)
    b = re.findall(r":(b\d+):", c("A", "tabs"))[-1]
    out("  page request Cookie header (server log):", [l.split('] ', 1)[-1] for l in since("A", L, m)[0] if "report: echo" in l or "cookiefilter" in l][:4])
    out("  page document.cookie:", c("A", f"ev {b} document.cookie"))
    out("  -- plus a COOKIES block content setting for the site")
    out("  cs:", c("A", "cs http://blocked.vsesvit.test/ cookies block"))
    out("  extension cookies.set again:", spy("A", "spy.setCookie('http://blocked.vsesvit.test/','ext2','2')"))
    out("  cookie store:", c("A", "cookies http://blocked.vsesvit.test/"))
    out("  page document.cookie after reload:", c("A", f"ev {b} location.reload(), 1"))
    time.sleep(2)
    out("  page document.cookie:", c("A", f"ev {b} document.cookie"))
    out("  extension cookies.getAll:", spy("A", "spy.getCookies('http://blocked.vsesvit.test/')"))
    stop_all(L, "A")


def item11():
    L = "i11"
    out("ITEM 11: Extensions.setStorageItems over a page session")
    setup(L)
    for p in "AB":
        cid = IDS["C"]
        out(f"  {p} Alloy manifest.json page:", c(p, f"hidden sta alloy chrome-extension://{cid}/manifest.json", 20))
        out(f"  {p} Alloy page setStorageItems:", c(p, f"dt sta Extensions.setStorageItems {json.dumps({'id': cid, 'storageArea': 'local', 'values': {'viaAlloy': 1}})}"))
        out(f"  {p} Alloy page chrome.storage.local.set by Runtime.evaluate:", c(p, f"ev sta chrome.storage.local.set({{viaEval:'{p}'}}).then(()=>chrome.storage.local.get('viaEval'))"))
        out(f"  {p} Chrome-style manifest.json page:", c(p, f"hidden st chrome chrome-extension://{cid}/manifest.json", 20))
        out(f"  {p} setStorageItems:", c(p, f"dt st Extensions.setStorageItems {json.dumps({'id': cid, 'storageArea': 'local', 'values': {'fromShell': p, 'n': 42}})}"))
        out(f"  {p} getStorageItems:", c(p, f"dt st Extensions.getStorageItems {json.dumps({'id': cid, 'storageArea': 'local', 'keys': ['fromShell', 'n', 'starts']})}"))
        out(f"  {p} extension reads it:", c(p, "ev st chrome.storage.local.get(['fromShell','n','viaEval'])"))
        out(f"  {p} sync area set:", c(p, f"dt st Extensions.setStorageItems {json.dumps({'id': cid, 'storageArea': 'sync', 'values': {'s': 1}})}"))
        out(f"  {p} other extension's id from this session:", c(p, f"dt st Extensions.setStorageItems {json.dumps({'id': IDS['S'], 'storageArea': 'local', 'values': {'x': 1}})}"))
        c(p, "nav http://site.vsesvit.test/index.html")
        time.sleep(2)
        out(f"  {p} from the tab where C's content script runs:", c(p, f"dt active Extensions.getStorageItems {json.dumps({'id': cid, 'storageArea': 'local', 'keys': ['fromShell']})}"))
    stop_all(L)


def item12():
    """Windows only: does a hidden browser activate the hidden host (and so deactivate GPUI)?"""
    out("ITEM 12 (Windows): hidden host activation by hidden browsers, per host kind")
    for mode in (sys.argv[3:] if len(sys.argv) > 3 and not sys.argv[3].isdigit() else ["popup", "noactivate", "child", "message"]):
        L = f"i12-{mode}"
        fresh(L)
        for n, v in SETS["A"]:
            install("A", L, n, v)
        out(f"  [{mode}] start:", start("A", L, {"SPIKE_HIDDEN_HOST": mode}))
        time.sleep(4)
        out(f"  [{mode}] before hidden browsers:", c("A", "activeinfo"))
        ack("A", [n for n, _ in SETS["A"]])
        out(f"  [{mode}] after the Chrome-style management page:", c("A", "activeinfo"))
        out(f"  [{mode}] bridge:", bridge("A"), call("A", {"op": "ping"}))
        out(f"  [{mode}] after the Alloy bridge page:", c("A", "activeinfo"))
        out(f"  [{mode}] foreground/activation events:", [trunc(l.split('] ', 1)[-1], 160) for l in log_lines("A", L) if "winevent: foreground" in l or "activation changed" in l])
        r = spy("A", "spy.open('tab','http://site.vsesvit.test/page2.html')")
        time.sleep(3)
        out(f"  [{mode}] after a Chrome-created window was neutralized into it:", c("A", "activeinfo"), "| hidden host:", [trunc(t, 200) for t in toplevels("A").split(" || ") if "VsesvitHiddenHost" in t])
        out(f"  [{mode}] stop:", stop("A", L))


def item13():
    """Windows only: (a) the browser-target CDP pipe through --remote-debugging-io-pipes (outcome
    B's mechanism, smoke only); (b) what the HKCU override does to settings Chromium reads from
    HKCU, with a proxy set only inside the override key."""
    out("ITEM 13 (Windows): CDP io-pipes smoke; HKCU override side effect (proxy)")
    L = "i13"
    fresh(L)
    for n, v in SETS["A"]:
        install("A", L, n, v)
    out("  start (pipe):", start("A", L, {"SPIKE_CDP_PIPE": "1"}))
    time.sleep(4)
    ack("A", [n for n, _ in SETS["A"]])
    out("  pipe setup:", [l.split('] ', 1)[-1] for l in log_lines("A", L) if "cdp io pipes" in l])
    out("  Browser.getVersion:", trunc(c("A", "cdp Browser.getVersion"), 300))
    out("  Extensions.getExtensions:", trunc(c("A", "cdp Extensions.getExtensions"), 300))
    unpacked = os.path.join(FX, "exts", "E", "1.0")
    out("  Extensions.loadUnpacked E:", c("A", "cdp Extensions.loadUnpacked " + json.dumps({"path": unpacked})))
    bridge("A")
    out("  list:", hlist("A"))
    out("  stop:", stop("A", L))
    k = winreg.CreateKey(winreg.HKEY_CURRENT_USER, regkey("A", L) + r"\Software\Microsoft\Windows\CurrentVersion\Internet Settings")
    winreg.SetValueEx(k, "ProxyEnable", 0, winreg.REG_DWORD, 1)
    winreg.SetValueEx(k, "ProxyServer", 0, winreg.REG_SZ, "127.0.0.1:9")
    winreg.CloseKey(k)
    out("  proxy 127.0.0.1:9 set inside the override key only; start:", start("A", L))
    time.sleep(4)
    m = mark("A", L)
    c("A", "new http://site.vsesvit.test/page2.html")
    time.sleep(3)
    b = re.findall(r":(b\d+):", c("A", "tabs"))[-1]
    out("  page title:", c("A", f"ev {b} document.title"), "| load errors:", [l.split('] ', 1)[-1] for l in since("A", L, m)[0] if "load_error" in l][:2])
    out("  stop:", stop("A", L))


ITEMS = {13: item13, 12: item12, 2: item2, 3: item3, 4: item4, 5: item5, 6: item6, 7: item7, 8: item8, 9: item9, 10: item10, 11: item11}
for i in WANT:
    try:
        ITEMS[i]()
    except Exception as e:
        out(f"ITEM {i} harness error: {e!r}")
        for L in (f"i{i}",):
            for p in "AB":
                try:
                    stop(p, L)
                except Exception:
                    pass
