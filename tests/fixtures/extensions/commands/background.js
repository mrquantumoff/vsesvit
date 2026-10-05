chrome.commands.getAll().then((all) => chrome.storage.local.set({ all }));

chrome.commands.onCommand.addListener((name, tab) => {
  chrome.storage.local.set({ command: { name, tab: tab ? tab.id : null, url: tab && tab.url ? tab.url : null } });
});
