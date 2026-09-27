# Research: WebKitGTK extensions, CRX3, store downloads (2026-09-27)

Collected by a research subagent from crates.io, docs.rs, webkitgtk.org, the Epiphany source (GNOME/epiphany main, 51.1), Chromium (`crx3.proto`, `crx_verifier.cc`, `id_util.cc`, `extensions/common/file_util.cc`) and WebKit sources, plus live requests to the Chrome Web Store update endpoint and the AMO API. Items marked UNVERIFIED were not confirmed.

## 1. Crates (all on the glib 0.22 stack)

```toml
gtk = { package = "gtk4", version = "0.11", features = ["v4_22"] }
adw = { package = "libadwaita", version = "0.9", features = ["v1_9"] }
webkit = { package = "webkit6", version = "0.6", features = ["v2_52"] }
javascriptcore6 = { version = "0.6", features = ["v2_48"] }
soup = { package = "soup3", version = "0.9" }
```

Do not enable `v1_10` / `v4_24`: build scripts check system library versions through pkg-config. Prefer `v4_22` over `gnome_50` (the latter also demands GLib 2.88).

## 2. WebKitGTK has no extension runtime

- 2.52 added only a manifest parser, `WebKitWebExtension` (`webkit6::WebExtension::new(path)`, feature `v2_52`), plus `WebExtensionMatchPattern` (2.48) and `WebView:web-extension-mode` (2.38; only changes CSP).
- No context/controller, no `chrome.*` injection. `WebKitWebExtensionContext` for GTK is upstream WIP (WebKit PR #73727, open as of 2026-09-20).

### How Epiphany implements WebExtensions

- Files: `src/webextension/ephy-web-extension-manager.c` (install, views, routing, URI scheme, `api_handlers[]`), `ephy-web-extension.c` (manifest, content scripts, generated background page, CSP), `api/{alarms,browseraction,commands,cookies,downloads,menus,notifications,pageaction,runtime,storage,tabs,windows}.c`, web-process side `embed/web-process-extension/` with `resources/js/webextensions-common.js` (runtime, storage, `window.chrome = window.browser`) and `webextensions.js` (tabs etc., extension pages only).
- Supported: alarms, commands, cookies (single store), downloads, extension.getURL/getViews/getBackgroundPage, i18n (partial), runtime sendMessage/onMessage/getURL/openOptionsPage, menus (limited), notifications, browserAction.onClicked, pageAction, storage.local only, tabs query/create/update/remove/executeScript/insertCSS/sendMessage (no tab events), windows.
- Not supported: declarativeNetRequest, webRequest, MV3 service workers, CRX (XPI and unpacked only).
- Extension id is a random UUID used as host of `ephy-webextension://<guid>/path` and as the script-world name.
- URI scheme: `register_uri_scheme("ephy-webextension")` + `register_uri_scheme_as_secure`; cross-origin requests may load only `web_accessible_resources`.
- Background/popup/options views: a WebView per extension (own WebContext), `related-view` to the background view, `default-content-security-policy` from the manifest (fallback `script-src 'self'; object-src 'self';`), `set_cors_allowlist(host_permissions)`, decide-policy restricting navigation to the extension origin. Background views are never in the widget tree. `background.scripts` becomes a generated `_generated_background_page.html`.
- Content scripts: `webkit_user_script_new_for_world(js, frames, time, world=guid, allow=matches, block=exclude_matches)`; `<all_urls>` becomes `http://*/*` + `https://*/*`; `document_idle` maps to document_end; added per tab's own `UserContentManager`.
- JS→UI calls go through a web-process extension and WebKitUserMessage `"WebExtensions.<ns>.<fn>"`; UI→JS events via `evaluate_javascript("window.browser.<ns>._emit_with_reply(...)", world=guid)`.
- Security: CVE-2026-77679 was a zip-slip in XPI extraction. Sanitize archive paths.

### Recommended approach for Rust (no web-process extension needed)

- One script world per extension id. Inject bootstrap + content script with `UserScript::for_world`.
- `register_script_message_handler_with_reply("ext", Some(ext_id))`; in JS `window.webkit.messageHandlers.ext.postMessage(x)` returns a Promise (`UserMessageHandler.idl`: `Promise<any> postMessage(any)`).
- The signal gives only a JSC `Value` (no view/frame), so use one `UserContentManager` per WebView, capture the tab id in the closure, and have the script send `location.href` and `window===top`.
- Extension pages: separate UCM with the handler in the default world.

## 3. webkit6 0.6.1 primitives

- `UserContentManager`: `new`, `add_script`, `remove_script`, `add_filter`, `remove_filter_by_id`, `add_style_sheet`, `register_script_message_handler(name, world: Option<&str>) -> bool`, `register_script_message_handler_with_reply(name, Option<&str>) -> bool`, `connect_script_message_received(Some(name), |ucm, &jsc::Value|)`, `connect_script_message_with_reply_received(Some(name), |ucm, &Value, &ScriptMessageReply| -> bool)`. Reply with `ScriptMessageReply::return_value(&Value)` / `return_error_message(&str)`; for async replies clone the reply and return true.
- `UserScript::new(src, UserContentInjectedFrames, UserScriptInjectionTime, allow, block)`, `UserScript::for_world(src, frames, time, world_name, allow, block)`.
- `WebContext::default()`, `register_uri_scheme(scheme, |&URISchemeRequest|)`, `security_manager().register_uri_scheme_as_secure / _as_cors_enabled / _as_local`.
- `URISchemeRequest`: `uri`, `path`, `web_view`, `http_method`, `http_headers`, `finish(stream, len, content_type)`, `finish_with_response(&URISchemeResponse)`, `finish_error`.
- `WebViewExt`: `evaluate_javascript(script, world_name, source_uri, cancellable, cb)` / `evaluate_javascript_future`, `call_async_javascript_function(body, Option<&Variant>, world, source_uri, cancellable, cb)` / `_future`, `set_cors_allowlist`, `connect_create`, `connect_decide_policy`, `send_message_to_page`.
- `WebView::builder().network_session(&s).user_content_manager(&ucm).related_view(&p).web_extension_mode(..).default_content_security_policy(..).build()`.
- `NetworkSession::new(Some(data_dir), Some(cache_dir))`, `new_ephemeral()`, `website_data_manager()`, `cookie_manager()`, `connect_download_started`, `set_itp_enabled`.
- `UserContentFilterStore::new(path)`, `save_future(id, &glib::Bytes) -> UserContentFilter`, `load_future`, `remove_future`.
- `javascriptcore6::Value::from_json(&ctx, s)`, `to_json(0)`, `context()`.

Pitfalls: views returned from `create` must use `related_view` and be shown after `ready-to-show`; UCM scripts/filters affect later loads only; MV3 `background.service_worker` should be emulated as a hidden page (custom scheme service workers UNVERIFIED); WSLg rendering may need `WEBKIT_DISABLE_DMABUF_RENDERER=1` / `WEBKIT_DISABLE_COMPOSITING_MODE=1` (UNVERIFIED for 2.52; set before `gtk::init`); Ubuntu AppArmor userns restrictions can break the bubblewrap sandbox (dev-only escape `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1`).

## 4. Store downloads (tested live)

### Chrome Web Store

- `https://clients2.google.com/service/update2/crx?response=redirect&prodversion=140.0.0.0&acceptformat=crx2,crx3&x=id%3D<ID>%26uc` → 302 to `clients2.googleusercontent.com/crx/blobs/.../<ID>_<ver>.crx`.
- 204 if `acceptformat` is missing, if `prodversion` is below the extension's `minimum_chrome_version`, or for delisted MV2 items (uBlock Origin classic).
- `response=updatecheck` returns XML with `codebase`, `version`, `size`, `hash_sha256`.
- Id from URL: path segment matching `^[a-p]{32}$` in `chromewebstore.google.com/detail/<slug>/<id>`, `/detail/<id>`, or legacy `chrome.google.com/webstore/detail/...`.

### CRX3

- `"Cr24"`, u32le 3, u32le N, `CrxFileHeader` (N bytes), ZIP.
- Header: `sha256_with_rsa=2` (repeated `AsymmetricKeyProof{public_key=1 SPKI DER, signature=2}`), `sha256_with_ecdsa=3`, `verified_contents=4`, `signed_header_data=10000` holding `SignedData{crx_id=1}` (16 bytes).
- Signed message: `"CRX3 SignedData\x00"` + u32le(len(signed_header_data)) + signed_header_data + ZIP bytes.
- RSA PKCS#1 v1.5 SHA-256 (the verifier uses `RSA_PKCS1_SHA256`) and ECDSA P-256 SHA-256.
- Id = hex of first 16 bytes of SHA-256(pubkey) mapped 0-f → a-p.
- Chrome rejects headers containing `PK\x05\x06`, `PK\x06\x06`, `PK\x06\x07`; one proof key must derive to `crx_id`; all proofs must verify; store installs also require the Web Store publisher key with SHA-256 `61f7f2a6bfcf74cd0bc1fe2497cc9b04254c658f79f2145392867ea8366367cf`.
- A live uBlock Origin Lite CRX: 1309-byte header, two RSA proofs (developer key → `ddkjiahe…`, and a Google key) plus one ECDSA proof matching the publisher hash. Its manifest has no `"key"` and has `update_url`.

### Firefox AMO

- `GET https://addons.mozilla.org/api/v5/addons/addon/<slug|guid|id>/` → `guid`, `current_version.file.{url, hash:"sha256:…", size, permissions}`.
- `/firefox/downloads/latest/<slug>/latest.xpi` → 302 to the file.
- XPI = ZIP with `META-INF` signatures; id from `browser_specific_settings.gecko.id`.

## 5. Manifest `key`

- Without `key`, Chromium derives an unpacked extension's id from its path; with `key` (base64 SPKI DER) the id is `GenerateId(key)`. The store strips `key`, so write the proof key whose id equals `crx_id` into `manifest.json` when unpacking.
- WebView2 also needs: delete top-level `_metadata/` (Chromium rejects top-level names starting with `_` except `_locales`, `_platform_specific`, `__MACOSX`; WebView2 returns `E_ACCESSDENIED`), and unpack each version into its own directory (changing files in place uninstalls).

## 6. declarativeNetRequest → WebKit content blockers

- Port WebKit's own translator logic from `Source/WebKit/UIProcess/Extensions/Cocoa/_WKWebExtensionDeclarativeNetRequestRule.mm`.
- Actions: block→`block`; allow/allowAllRequests→`ignore-following-rules` with priority ordering (allowAllRequests adds `if-frame-url` + `load-context: child-frame`); upgradeScheme→`make-https`; redirect→`redirect`; modifyHeaders→`modify-headers`.
- Conditions: `urlFilter`→converted regex; `regexFilter` only within WebKit's subset; `isUrlFilterCaseSensitive`→`url-filter-is-case-sensitive`; resource types main_frame→`top-document`, sub_frame→`child-document`, stylesheet→`style-sheet`, xmlhttprequest→`fetch`, others direct; `domainType`→`load-type`; `domains`/`excludedDomains`→`if-domain`/`unless-domain` (with `*` prefix); `initiatorDomains`→`if-frame-url`/`unless-frame-url`; `requestMethods`→`request-method`; `requestDomains` folded into `url-filter`.
- WebKit accepts actions block, block-cookies, css-display-none, ignore-previous-rules, ignore-following-rules, make-https, notify, redirect, modify-headers; trigger keys url-filter, resource-type, load-type, load-context, request-method, if/unless-domain, if/unless-top-url, if/unless-frame-url (UNVERIFIED all in 2.52; test by compiling).
- Regex limits: ASCII only; no `|`, `\b`, `\d`/`\w`, backreferences, `{n,m}`; `^` only first, `$` only last.
- `ignore-*` only works within one `UserContentFilter`: merge each extension's rulesets into one list.
- `adblock` 0.13.3 `content-blocking` feature converts ABP/uBO lists (not DNR) to WebKit JSON.
