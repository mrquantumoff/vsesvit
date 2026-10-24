// Marks the page with this reporter's name, version and visit count: data-rep-<name>="<version>:<visits>".
chrome.runtime.sendMessage({ type: "visit" }).then(
  (r) => {
    document.documentElement.dataset["rep" + r.name] = r.version + ":" + r.visits + (r.incognito ? ":incognito" : "");
    fetch("http://report.vsesvit.test/report", {
      method: "POST",
      body: JSON.stringify({ ext: r.name, version: r.version, event: "content", href: location.href, visits: r.visits, incognito: r.incognito }),
    }).catch(() => {});
  },
  (e) => { document.documentElement.dataset.repError = String(e); },
);
