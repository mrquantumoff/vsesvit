#!/usr/bin/env python3
"""A stand-in org.freedesktop.Notifications server for the spike's private D-Bus session.

  notifyd.py <logfile> <cmd-fifo>

Logs every Notify/CloseNotification call as one JSON line. Commands written to the FIFO
("click <id>", "action <id> <key>", "close <id> <reason>") emit ActionInvoked /
NotificationClosed the way a desktop's notification daemon does on a user's click or close."""
import json, os, sys, threading
import dbus, dbus.service, dbus.mainloop.glib
from gi.repository import GLib

log_path, fifo = sys.argv[1], sys.argv[2]

def log(o):
    with open(log_path, "a") as f:
        f.write(json.dumps(o) + "\n")

class Notifications(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(bus, "/org/freedesktop/Notifications")
        self.next = 1

    @dbus.service.method("org.freedesktop.Notifications", in_signature="", out_signature="as")
    def GetCapabilities(self):
        return ["actions", "body", "body-markup", "icon-static", "persistence"]

    @dbus.service.method("org.freedesktop.Notifications", in_signature="", out_signature="ssss")
    def GetServerInformation(self):
        return ("vsesvit-spike-notifyd", "spike", "1.0", "1.2")

    @dbus.service.method("org.freedesktop.Notifications", in_signature="susssasa{sv}i", out_signature="u")
    def Notify(self, app, replaces, icon, summary, body, actions, hints, timeout):
        nid = int(replaces) or self.next
        self.next = max(self.next, nid) + 1
        log({"call": "Notify", "id": nid, "app": str(app), "summary": str(summary), "body": str(body), "actions": [str(a) for a in actions], "timeout": int(timeout)})
        return dbus.UInt32(nid)

    @dbus.service.method("org.freedesktop.Notifications", in_signature="u", out_signature="")
    def CloseNotification(self, nid):
        log({"call": "CloseNotification", "id": int(nid)})
        self.NotificationClosed(nid, 3)

    @dbus.service.signal("org.freedesktop.Notifications", signature="uu")
    def NotificationClosed(self, nid, reason):
        pass

    @dbus.service.signal("org.freedesktop.Notifications", signature="us")
    def ActionInvoked(self, nid, key):
        pass

dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName("org.freedesktop.Notifications", bus)
svc = Notifications(bus)
log({"call": "started"})

def reader():
    if not os.path.exists(fifo):
        os.mkfifo(fifo)
    while True:
        with open(fifo) as f:
            for line in f:
                parts = line.split()
                if not parts:
                    continue
                def emit(parts=parts):
                    if parts[0] == "click":
                        svc.ActionInvoked(dbus.UInt32(int(parts[1])), "default")
                    elif parts[0] == "action":
                        svc.ActionInvoked(dbus.UInt32(int(parts[1])), parts[2])
                    elif parts[0] == "close":
                        svc.NotificationClosed(dbus.UInt32(int(parts[1])), dbus.UInt32(int(parts[2]) if len(parts) > 2 else 2))
                    log({"cmd": parts})
                    return False
                GLib.idle_add(emit)

threading.Thread(target=reader, daemon=True).start()
GLib.MainLoop().run()
