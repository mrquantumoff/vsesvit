// agent.run(method, ...args) from a page context (needed where an API wants a page, e.g. a user gesture).
window.agent = {
  async run(method, ...args) {
    let line;
    try {
      const r = await chrome.management[method](...args);
      line = "vsesvit-agent " + method + " ok " + JSON.stringify(r ?? null);
    } catch (e) {
      line = "vsesvit-agent " + method + " err " + (e.message || e);
    }
    console.log(line);
    document.getElementById("out").textContent += "\n" + line;
    fetch("http://127.0.0.1:8792/report", { method: "POST", body: JSON.stringify({ ext: "agent", event: "page-run", line }) }).catch(() => {});
    return line;
  },
};
