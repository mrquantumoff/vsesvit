chrome.storage.local.get({ visits: 0 }).then(({ visits }) => {
  document.getElementById("visits").textContent = String(visits);
  document.title = "visits=" + visits;
});
