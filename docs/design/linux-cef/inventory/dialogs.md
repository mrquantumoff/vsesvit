# Inventory: `crates/vsesvit-gtk/src/dialogs/`

The behaviour of every dialog and library window in the GTK shell, written so that a new GPUI + CEF Linux shell can reproduce each user-visible feature without reading the old code.

Source read: the `linux-cef-gpui` worktree at `/home/dy/git/vsesvit-cef`, all 14 files in `crates/vsesvit-gtk/src/dialogs/` (`mod.rs`, `about.rs`, `bookmarks.rs`, `downloads.rs`, `extension_prompts.rs`, `extensions.rs`, `history.rs`, `passphrase.rs`, `search_engines.rs`, `settings.rs`, `shortcut_settings.rs`, `shortcuts.rs`, `site_data.rs`, `welcome.rs`). I also read the code they call into: `browser.rs`, `extensions.rs`, `engine.rs`, `cookies.rs`, `permissions.rs`, `sync.rs`, `keymap.rs`, `downloads.rs`, `bookmark_drag.rs`, `updates/`, `self_test.rs`, the core modules (`prefs`, `search`, `shortcuts`, `spellcheck`, `onboarding`, `cookies`, `permissions`, `history`, `session`, `import`, `export`, `downloads`, `extensions::*`), and `vsesvit_sync::status`.

## Legend

- **VERIFIED**: I read it in the code. Everything is verified unless it is marked otherwise.
- **INFERENCE**: my reading of how something maps to CEF or Chromium. I did not check it in Chromium source or run it.
- Classification of each engine-touching piece:
  - **(KEEP)**: product behaviour to reproduce.
  - **(WEBKIT-ONLY)**: a WebKitGTK workaround or limitation that goes away with Chromium.
  - **(NEEDS-CEF)**: needs a CEF equivalent. The likely API is named. API names marked "verified in bindings" exist in `cef-dll-sys-154.5.0+154.0.34/src/bindings/x86_64_unknown_linux_gnu.rs`.
- Toolkit-only pieces (Adwaita widgets, GIO pickers) are not engine-touching. They are product behaviour (KEEP) that GPUI has to provide some other way.

CEF facts checked in `cef-dll-sys` 154.5.0+154.0.34 (CEF `154.0.34+g14c5a08+chromium-154.0.8037.98`) that this inventory relies on:

- `_cef_download_handler_t` (`can_download`, `on_before_download`, `on_download_updated`) and `_cef_download_item_callback_t` (`cancel`, `pause`, `resume`) exist.
- `_cef_permission_handler_t` (`on_request_media_access_permission`, `on_show_permission_prompt`, `on_dismiss_permission_prompt`) exists.
- `_cef_request_context_t` has `get_cookie_manager`, `clear_http_cache`, `clear_certificate_exceptions`, `get/set_website_setting`, `get/set_content_setting`, `set_chrome_color_scheme` and `get_chrome_color_scheme_*`, `add_setting_observer`, and `resolve_host`. It inherits `_cef_preference_manager_t` (`has/get/set_preference`, `get_all_preferences`, `can_set_preference`, `add_preference_observer`).
- `_cef_cookie_manager_t` has `visit_all_cookies` and `delete_cookies`.
- The browser host has `execute_dev_tools_method`, `run_file_dialog`, `start_download`, `replace_misspelling` and `add_word_to_dictionary`. Context-menu params have `get_dictionary_suggestions`. The life-span handler has `on_before_popup` and `on_before_popup_aborted`.
- The content-setting enum includes `CEF_CONTENT_SETTING_TYPE_COOKIES` (0), `_POPUPS` (3), `_GEOLOCATION` (4), `_NOTIFICATIONS` (5), `_MEDIASTREAM_CAMERA` (9), `_MIDI_SYSEX` (13) and `_CLIPBOARD_READ_WRITE` (51). The colour-variant enum includes `CEF_COLOR_VARIANT_SYSTEM`, `_LIGHT` and `_DARK`.
- `CEF_VERSION` and `CHROME_VERSION_*` constants exist.
- **There is no `cef_extension_t`, `cef_extension_handler_t` or `LoadExtension` anywhere in these bindings.** CEF 154 has no public extension-management API. Only Chrome command ids such as `IDC_MANAGE_EXTENSIONS` (40022) and `can_execute_chrome_command`/`execute_chrome_command` remain.

---

## 0. How the dialogs are reached

| Entry point | What it opens | Code |
|---|---|---|
| Primary menu (hamburger, tooltip "Main Menu"), "Library" section: `_Bookmarks`, `_History`, `_Downloads`, `_Extensions`, then `Show Bookmarks _Bar` (a toggle) | `win.show-bookmarks`, `win.show-history`, `win.show-downloads`, `win.show-extensions` | `window/menu.rs:29-34`, `window/actions.rs:157-197` |
| Primary menu, "App" section: `_Settings`, `_Keyboard Shortcuts`, `_Welcome`, `_About Vsesvit` | `win.show-settings`, `app.shortcuts`, `app.welcome`, `app.about` | `window/menu.rs:36-40`, `app.rs:322-342` |
| Header bar's downloads button (`folder-download-symbolic`, "Downloads") | `win.show-downloads` | `window/mod.rs:327` |
| Extension toolbar menu, `_Manage Extensions` | `win.show-extensions` | `window/ext_actions.rs:226,385` |
| Keyboard (core defaults, reassignable): Ctrl+Shift+O Bookmarks, Ctrl+H History, Ctrl+J Downloads, Ctrl+, Settings, Ctrl+? Keyboard shortcuts | the actions above | `vsesvit_core::shortcuts` `Command::{ShowBookmarks, ShowHistory, ShowDownloads, ShowSettings, ShowShortcuts}`; `keymap::binding` |
| The first window of the run that created the profile | the welcome, over the first normal window, after startup windows and targets have opened | `browser.rs:183` (`!SCRIPTED && onboarding::should_show`), `browser.rs:496-500` |
| The Settings button on an extension's notification (`app` action `notifications::ACTION`, parameter `(sss)`, `Activation::Settings`) | the Extensions dialog with that extension's row expanded (`present_extension`) | `app.rs:343-364` |
| A user's install from a store or package (Extensions dialog) | Chrome's install prompt (`extension_prompts::install_dialog`) | `extensions.rs:123-138` |
| `chrome.permissions.request` from an extension (vsesvit-webext's `ask_permissions`) | Chrome's request prompt (`extension_prompts::request_dialog`) over the window it came from | `browser.rs:1593-1601` |
| Sync, on its own | the passphrase offer and the passphrase prompt over the first **normal** window, never a private one | `sync.rs:185-236` → `dialogs::passphrase::prompt` |
| Settings > General > Profiles > "Manage Profiles…" | `crate::profiles::manage(window)`, which lives outside `dialogs/` | `settings.rs:520-525` |
| Development builds only: D-Bus `app.debug-close-dialog()` | closes the active window's dialog, or the active Bookmarks/History/Downloads window | `app.rs:388-395` |

(KEEP) Every entry point above. The menu labels, actions and default shortcuts are core's or the shell's product decisions.

---

## 1. Shared helpers: `dialogs/mod.rs`

### 1.1 Windows of their own: `Windowed` and `present_window`

- `enum Windowed { Bookmarks, History, Downloads(Browsing) }`. There is one window of each kind per browser process. Downloads gets **one window for normal browsing and a separate one for private browsing**. Only the private one lists private downloads (see §14).
- `present_window(opener, kind, build)`: if `browser.windowed(kind)` holds a visible window, it is presented (raised) and nothing new is built. Otherwise `build()` makes an `adw::Window`, it is set on the application, registered with `browser.add_windowed(kind, &window)`, and presented.
- The library window belongs to the **browser window it was opened from** (`opener`). When the opener unrealizes (closes), the library window closes too. If another browser window asks for it later, the same window is raised (`browser.windowed` finds it), but it still closes with its original opener. VERIFIED by the test `a_window_of_its_own_opens_once_and_closes_with_its_browser_window`: closing a second window that also asked for it leaves it open, and closing the first closes it.
- A library window is not a browser window: it does not count in `browser.windows()`.
- `Browser::windowed` returns only *visible* windows, and `add_windowed` prunes entries that are dead or hidden (`browser.rs:521-538`).
- (KEEP) One instance per kind, raise on re-open, close with the opener, and a separate private Downloads window.

### 1.2 `LibraryWindow`: the searchable list window (Bookmarks, History)

- Layout, top to bottom:
  - an `adw::HeaderBar`, with extra widgets packed at the **end** in the order given;
  - a full-width `gtk::SearchEntry` with placeholder text, centred in a clamp of at most 560 px with margins 12/12 and 6 at the bottom;
  - a content slot (`adw::Bin`) inside an `adw::ToastOverlay`.
- Window: default size **640×680**, title as given.
- `toast(text)` adds a plain-text toast.
- The search uses the `search-changed` signal (GTK's `SearchEntry` debounces it). No key-capture widget is set, so typing elsewhere does not start a search. No initial focus is set explicitly.
- (KEEP) The layout and toast area. GPUI needs its own toast component.

### 1.3 `plain_toast(text)`

Adwaita reads toast titles as markup by default. Every toast in the shell is built with `use_markup(false)`, because a file name, an extension name or a server error could contain `&`, `<` and so on.

A source-scanning unit test (`no_toast_reads_its_title_as_markup`) fails the build if `adw::Toast::new(` appears anywhere in `src`. The test `a_toast_shows_a_name_as_written` checks that `"Imported 2 items from Q&A <b>.html"` shows verbatim.

(KEEP) Never interpret user or remote text as markup. In GPUI this is natural, but the rule stands.

### 1.4 Small modal prompts (`adw::AlertDialog`)

- `prompt_text(parent, heading, initial, accept)`: an entry prefilled with `initial`, with Enter activating the default. Responses are `_Cancel` and `accept` (suggested, default); closing counts as cancel. The entry is focused. It returns the trimmed text, or `None` when cancelled **or the text is empty**.
- `prompt_choice(parent, heading, body, options, accept)`: a drop-down of `options`, plus `_Cancel` and `accept` (suggested, default). It returns the selected index or `None`.
- `confirm(parent, heading, body, accept)`: `_Cancel` plus a **destructive** `accept`. The **default response is Cancel**, and closing counts as cancel.
- `row_button(icon, tooltip)`: a flat icon button, vertically centred, for list-row suffixes.
- `boxed_list()`: an unselectable, inset "boxed-list" `ListBox` with margins 12/12/6/12, aligned to the top.
- `format_time(unix_ms)`: the local time formatted `"%x %H:%M"` (the locale's date plus 24-hour time). It returns an empty string on failure.
- (KEEP) All of them. `format_time` needs a locale-aware date formatter in the new shell, since `glib::DateTime` is gone.

---

## 2. Settings dialog: `settings.rs`

### 2.1 Frame

- `adw::PreferencesDialog`, title **"Settings"**, `content_width(1040)`, so the six page names fit in the header rather than in a bottom view switcher. It opens over the browser window it was invoked from (`win.show-settings`).
- Pages in order, each with a **name** (`visible-page-name`, which the self-test selects pages by), a title and an icon:

| # | name | title | icon |
|---|---|---|---|
| 1 | `general` | General | `preferences-system-symbolic` |
| 2 | `sync` | Sync | `view-refresh-symbolic` (`SYNC_ICON`; Adwaita has no sync icon) |
| 3 | `appearance` | Appearance | `applications-graphics-symbolic` |
| 4 | `search` | Search | `system-search-symbolic` |
| 5 | `privacy` | Privacy | `security-high-symbolic` |
| 6 | `shortcuts` | Shortcuts | `preferences-desktop-keyboard-shortcuts-symbolic` |

- **Every change applies at once, in every window.** An open dialog follows what sync changes, through `Browser::watch_prefs`, which `Browser::sync_applied` calls when `changed.prefs` is non-empty or `changed.search_engines` is set.
- The generic row helpers enforce one rule: **a value that is already stored is never written back.** `choice_row` and `pref_switch_row` call `set` only if the stored value differs from the widget's. This stops a value that arrived from sync being re-stamped and re-uploaded. (KEEP: this matters for the CRDT/LWW semantics.)
- Subpages (Search Engines, Site Data, Site Permissions) are pushed onto the dialog's navigation stack with `push_subpage`. Each has a header bar with a back button.
- Error and notice toasts show inside the dialog (`PreferencesDialog::add_toast`), except "Clear Browsing Data", whose toasts go to the browser window.

### 2.2 General page

Groups in order: **Startup**, **Profiles** (conditional), **Downloads**, **Spell Check**, **System**, **Memory**, **Updates** (conditional).

**Startup**

- **"On Startup"**, a combo:
  - "Restore the previous session" → `Startup::RestoreSession` (the default);
  - "Open the homepage" → `Startup::Homepage`;
  - "Open a new tab" → `Startup::NewTab`.

  Pref `startup` (synced). It is written with `browser.set_pref` and has no live effect.
- **"Homepage"**, an entry row with an apply button and URL input purpose. It initially shows the stored pref text verbatim, so **a fresh profile shows `about:home`**. On apply it runs `vsesvit_core::prefs::homepage_input(text)`:
  - `Some(HomepageValue::Default)` (empty or whitespace) stores `"about:home"`. The text field is left as typed.
  - `Some(HomepageValue::Url(url))` stores the normalized URL and replaces the field's text with it.
  - `None` (not an address) shows the dialog toast **"Not a web address"** and stores nothing.

  It follows sync: on a prefs change the text is replaced if it differs. Pref `homepage` (synced, default `"about:home"`). The home button and startup read it through `prefs::homepage_url`.

**Profiles** (only when `browser.home()` is `Some`, that is, when the profile is in the install's `profiles/` list rather than an outside `--profile-dir`)

- **"Show Profile Picker at Startup"**, a switch with subtitle "Ask which profile to open when Vsesvit starts and there is more than one". Its initial state is `browser.profiles().show_picker()`. On toggle it calls `browser.set_show_profile_picker(on)` → `home.dir.set_show_picker(on)` → rewrites **`profiles/profiles.json`** under the `profiles/profiles.lock` lock → `set_profiles(registry)`. This is an install-wide switch, not a profile pref, as in Chrome. It does **not** follow changes made elsewhere while the dialog is open (there is no watch).
- **"Manage Profiles…"**, a button row → `crate::profiles::manage(window)` (an inventory item outside `dialogs/`).

**Downloads**

- **"Download Folder"**, an action row:
  - The subtitle is the effective folder, `downloads().directory()`: the pref, or else the XDG Downloads directory (`glib::user_special_dir(Downloads)`, falling back to `~/Downloads`). It is selectable and not markup. The test `a_download_folder_shows_as_named_not_as_markup` uses `/tmp/R&D <x>`.
  - Suffix button **"Use Default"** (tooltip "Save to the Downloads folder") shows only while `downloads.directory` is set. It resets the pref.
  - Suffix button **"Change…"** opens a folder chooser titled "Download Folder", starting in the current folder and modal. The chosen folder sets `downloads.directory = Some(path)`.

  Pref `downloads.directory` (local, `Option<PathBuf>`).
- **"Ask Where to Save Each File"**, a switch. Pref `downloads.ask` (synced, default false). It has no live effect; the downloads controller reads it per download.

**Spell Check** (group title "Spell Check")

- **"Check Spelling"** (`SPELLCHECK_ROW`), a switch with subtitle "Underline misspelled words as you type on web pages". Pref `spellcheck.enabled` (synced, default true), applied through `Browser::set_engine_switch` → `Engine::apply_prefs`.
- **"Languages"** (`SPELLING_LANGUAGES_ROW`), sensitive only while Check Spelling is on (property binding):
  - With **no dictionaries installed**: an action row with subtitle "No dictionaries are installed. Install your distribution's Hunspell dictionaries for your languages."
  - Otherwise: an expander row whose subtitle names the checked languages, joined with ", ", or "None". Each **installed** dictionary has a child switch titled `spellcheck::display_name(lang)` ("English (United States)" for `en_US`, "Ukrainian" for `uk`; an unknown name such as `de_DE_frami` shows as named). A switch is on if the language is in `Dictionaries::checked(pref)`. Toggling calls `browser.set_spellcheck_language(lang, on)`, which stores `spellcheck.languages = Some(Dictionaries::choose(pref, lang, on))` and re-applies the engine prefs. It follows sync through `watch_prefs`.
  - Core semantics, VERIFIED in `spellcheck.rs`: while the pref is `None`, the checked languages are the installed dictionaries for the system locales (`LANGUAGE`, then `LC_ALL`/`LC_MESSAGES`/`LANG`). The first choice starts from that set. Languages chosen on another device that are not installed here are kept in the pref but not checked.
  - The engine (WebKit) turns checking **off** when the checked list is empty, because WebKit given none would check the system languages.
- Engine side (WEBKIT-ONLY): `engine::dictionaries()` scans Enchant's Hunspell directories (`~/.config/enchant/hunspell`, `/usr/share/hunspell`, `/usr/share/myspell`, `/usr/share/myspell/dicts`) for `<lang>.dic` + `<lang>.aff` pairs (`spellcheck::installed_in`), and `WebContext::set_spell_checking_languages`/`set_spell_checking_enabled` apply them.
- (NEEDS-CEF, INFERENCE) Chromium's spell checker uses its own `.bdic` dictionaries, normally downloaded per language into the profile's `Dictionaries/` folder. It does not use the system's Hunspell `.dic`/`.aff` files. The likely controls are the Chromium prefs `browser.enable_spellchecking` and `spellcheck.dictionaries` (a list), set through `CefRequestContext`/`CefPreferenceManager::SetPreference` (verified in bindings). So the "installed dictionaries" list, the "No dictionaries are installed" text and the Enchant scan are WebKit-only. The new list probably has to be Chromium's supported spell-check languages. The synced pref semantics (keep unknown languages, `None` = system languages) remain (KEEP). Suggestions and Add to Dictionary in the context menu map to `CefContextMenuParams::GetDictionarySuggestions`, `CefBrowserHost::ReplaceMisspelling` and `AddWordToDictionary` (verified in bindings), which belong to the page-menu inventory.

**System**

- **"Smooth Scrolling"**, a switch with no subtitle. Pref `scrolling.smooth` (synced, default true) → `set_engine_switch` → `webkit::Settings::set_enable_smooth_scrolling`, applied **live** (WEBKIT-ONLY: live application).
- **"Hardware Acceleration"**, a switch with subtitle "Use the graphics card to draw pages when available". Pref `system.hardware_acceleration` (**local**, default true) → `HardwareAccelerationPolicy::Always/Never`, live (WEBKIT-ONLY: live application).
- (NEEDS-CEF, INFERENCE) In Chromium both are process-start switches (`--disable-smooth-scrolling`, `--disable-gpu`/`--disable-gpu-compositing`), applied in `CefApp::OnBeforeCommandLineProcessing`. They cannot change live. The Windows shell already shows "Takes effect when you restart Vsesvit." under both switches (`vsesvit-winui/src/dialogs/settings.rs:75-84`). The CEF shell probably needs the same note.
- **"Profile Folder"**, a static row whose subtitle is `core.paths().root` (selectable, not markup). (KEEP)

**Memory**

- **"Memory Saver"** (`memory_saver::TITLE`), a switch with subtitle `memory_saver::DESCRIPTION` = "Frees up memory from inactive tabs, which become active again when you go back to them". Pref `performance.memory_saver` (local, default true). Written with `set_pref` only; the sweep reads it.
- **"Memory Savings"** (`MEMORY_SAVINGS_ROW`), a combo of `MemorySaverMode::ALL` labels:
  - "Moderate": "Tabs become inactive after 6 hours in the background";
  - "Balanced" (the default): "… after 4 hours …";
  - "Maximum": "… after 2 hours …".

  The subtitle is the description of the chosen mode. It is sensitive only while Memory Saver is on, and follows sync. Pref `performance.memory_saver.mode` (local).
- (KEEP) The policy is core's. How a tab sleeps is engine-specific and outside this inventory (CEF: likely `CefBrowserHost` discard/close plus restore, or `WasHidden`; an inference).

**Updates** (only when `browser.updates()` is `Some`)

- `Updates::new` returns `Some` only when `vsesvit_update::Installation::detect().self_updates()` is true (the NSIS, Deb, Rpm, Pacman or AppImage formats), `Config::builtin()` succeeds, and the updater constructs. Unpackaged builds (`cargo run`) and Flatpak get **no Updates group**.
  - Note: the comment in `settings.rs` says "a package from a distribution or Flatpak has no group", but the code says deb, rpm and pacman installs from Vsesvit's own repository *do* self-update. VERIFIED in `vsesvit-update/src/installation.rs:76-90`.
- **Status row**:
  - Title is `Status.title` from the update lifecycle. The texts (VERIFIED, `updates/lifecycle.rs:204-243`) are:
    - "Vsesvit {current}"
    - "Checking for updates…"
    - "Vsesvit is up to date"
    - "Downloading Vsesvit {v}…"
    - "Vsesvit {v} is ready to install"
    - "Installing Vsesvit {v}…"
    - "Vsesvit {v} will open next time"
    - "Restart to finish updating"
    - "Vsesvit {v} is installed"
    - "Could not check for updates: {message}"
    - "Updates are off: {reason}"
  - Subtitle is "Version {CARGO_PKG_VERSION}".
  - Suffix: either **"Check for Updates"**, insensitive while a check, download or install runs (`StatusButton::Check{enabled}`), which calls `updates.check()`; or a button labelled "Update", "Restart" or "Restart now" (`StatusButton::Update(label)`) that activates `app.update`, the same action as the window banner's button. Only one of the two is visible. The row follows the lifecycle (`updates.watch_status`).
- **"Update Channel"**, a combo of Stable, Beta, Weekly and Nightly, with subtitle "Vsesvit moves to a steadier channel once that channel has a version newer than this one." `Browser::set_updates_channel` writes pref `updates.channel` (local; default `UpdateChannel::of_build()`) and calls `updates.set_channel` → `Event::ChannelChanged`, which checks the new channel at once, even with automatic updates off.
- **"Automatic Updates"**, a switch with subtitle "Download new versions of Vsesvit in the background". `Browser::set_updates_automatic` writes pref `updates.automatic` (local, default true) and starts or stops the timer.
- (KEEP) The behaviour and the `vsesvit_update` contract. Packaging changes with CEF (the bundle is much larger), but that is outside this inventory.

### 2.3 Sync page

Groups in order: the account (untitled), **Customize Sync**, the server (untitled), and Delete (untitled).

**Account row** (`sync_account_row`)

- Title and subtitle come from `vsesvit_sync::status::State::status(now_secs)` (not markup). VERIFIED texts (`vsesvit-sync/src/status.rs:172-238`):
  - Signed out: "Not signed in" / the error, or "Sign in to sync bookmarks, history, open tabs, extensions and settings across your devices."
  - Signing in: "Signing in…" / "Finish signing in on the page that opened."
  - Signed in: title "Signed in as {name}" or "Signed in to {host}". Subtitle, in priority order:
    - needs_sign_in: "Sign-in failed: {e}" or "Sign in again to keep syncing.";
    - "Sync failed: {e}";
    - the passphrase ask: "Checking for a sync passphrase…", "Enter your sync passphrase to sync on this device.", "Your sync passphrase was changed on another device. Enter the new one to keep syncing.";
    - "Syncing…";
    - "Last synced {ago}" or "Not synced yet", followed by ". Not encrypted: whoever runs the sync server can read your synced data." when encryption is Off.
- Suffixes: a spinner (hidden), then **one button per action in the fixed order** `SignIn, Cancel, EnterPassphrase, SyncNow, SetPassphrase, ChangePassphrase, SignOut`. Labels come from `Action::label()`: "Sign In", "Cancel", "Enter Passphrase…", "Sync Now", "Encrypt with a Passphrase…", "Change Passphrase…", "Sign Out". A button is visible iff `status.actions` contains its action. The **first** listed action gets the `suggested-action` style. "Sync Now" is insensitive while `status.busy`.
  - The unit test `every_sync_state_lists_its_buttons_in_their_fixed_order` checks that core always lists actions in this order.
- Clicks:
  - Set Passphrase and Change Passphrase run `passphrase::take(row, syncer, busy)` (§8). Meanwhile the row is insensitive and the spinner shows. A failure gives the dialog toast "Could not use the sync passphrase: {e}".
  - Everything else calls `syncer.act(action)`: SignIn opens the server's sign-in page in a tab; Cancel; SyncNow; EnterPassphrase shows the prompt even while a welcome is open; SignOut.
- It follows the state through `syncer.watch`.

**Customize Sync** (title "Customize Sync", description "What this device syncs")

- **"Sync Everything"**, a switch, then one switch per `DataType::ALL` with core labels: "Bookmarks", "History", "Open tabs", "Extensions", "Settings".
- State model, as in Chrome's "Customize sync":
  - While Sync Everything is on (all types chosen and not "customizing"), every type switch shows on and is **insensitive**.
  - Turning Sync Everything off while all types are chosen enters "customizing". The type switches become sensitive and stay on; nothing is written.
  - Turning a type off or on writes `sync.types = DataType::toggled(chosen, type, on)` (in `ALL` order) and calls `syncer.types_changed()`, which syncs now.
  - Turning Sync Everything back on writes all types if any were missing.
- Pref `sync.types` (**local**, default all). (KEEP)

**Sync Server** (`sync_server_row`, also used by the welcome)

- An entry row titled **"Sync Server"**, with an apply button and URL input purpose. It initially shows pref `sync.server` (local; default `https://vsesvit-service.mrquantumoff.dev`, `DEFAULT_SYNC_SERVER`).
- Editing clears any error.
- On apply it runs `vsesvit_sync::server_input(text)`:
  - `Ok(None)` (empty) resets the pref and shows `DEFAULT_SYNC_SERVER`.
  - `Ok(Some(s))` (normalized: https, or http on loopback only, with no query, fragment or userinfo, and the trailing `/` stripped) stores it and replaces the text if it differs.
  - `Err(e)` shows the error text **as the group's description, above the field** (for example "{input} is not a valid server address") and gives the row the `error` style.
- The row is sensitive only while `status.server_editable` (signed out).

**Delete Data on Server** (`delete_server_data_group`)

- A destructive button row, "Delete Data on Server…". The group is visible only while `status.actions` contains `DeleteServerData`.
- On click: `confirm(DELETE_CONFIRMATION)`, with title "Delete your data on the sync server?", body "Your bookmarks, history, open tabs, extensions and settings are deleted from the server, along with your sync passphrase, and all your devices sign out. They stay on your devices, which upload them again, unencrypted until a passphrase is set, when they next sign in." and button "Delete".
- The row is then insensitive during `syncer.delete_server_data().await`. A failure gives the dialog toast "Could not delete the data on the server: {e}", and the profile stays signed in.

(KEEP) All of the sync page. It touches no engine. The sign-in page opens in a browser tab, which uses the normal tab machinery.

### 2.4 Appearance page (one untitled group)

- **"Theme"**, a combo: "Follow the system" / "Light" / "Dark". `Browser::set_theme` writes `theme` (synced, default `system`) and calls `apply_theme()` → `adw::StyleManager::set_color_scheme(Default/ForceLight/ForceDark)`.
  - (KEEP) for the chrome.
  - (NEEDS-CEF, INFERENCE) Web content's `prefers-color-scheme` and Chromium-drawn UI: `CefRequestContext::SetChromeColorScheme(CEF_COLOR_VARIANT_SYSTEM|LIGHT|DARK, color)` (verified in bindings; the effect on web content is an inference).
- **"Tab Position"**, a combo: "Left sidebar" / "Right sidebar" / "Top bar", with subtitle "Vertical tabs in a sidebar, or a bar above the page". `Browser::set_tabs_position` writes `tabs.position` (synced, default `left`) and re-lays out every window (`window.apply_layout`). (KEEP)
- **"Show Bookmarks Bar"**, a switch → `set_bookmarks_bar_visible`. Pref `bookmarks_bar.visible` (synced, default true); every window updates. The primary menu's "Show Bookmarks _Bar" toggle and Ctrl+Shift+B do the same. (KEEP)
- **"Show Home Button"**, a switch with subtitle "Next to Reload, opens the homepage" → `set_home_button_visible`. Pref `toolbar.home_button` (synced, default false). (KEEP)

### 2.5 Search page

Groups in order: the search engines group (untitled, §3), **Address Bar**, **Suggestions**.

- Address Bar:
  - **"Compact Address Bar"**, subtitle "A narrow address bar in the middle of the toolbar" → `set_compact_address_bar`. Pref `address_bar.compact` (synced, default **true**); every window updates.
  - **"Always Show Full URLs"**, subtitle "Otherwise https:// and www. show only while editing the address" → `set_full_urls`. Pref `address_bar.full_urls` (synced, default false); every window updates.
- Suggestions (each a plain `set_pref`; the omnibox reads them per keystroke):
  - **"Search Suggestions"**, subtitle "Sends what you type in the address bar to your search engine". Pref `address_bar.suggest.search` (synced, default true).
  - **"Browsing History"**. Pref `address_bar.suggest.history` (synced, default true).
  - **"Bookmarks"**. Pref `address_bar.suggest.bookmarks` (synced, default true).
- (KEEP) All of them.

### 2.6 Privacy page

Groups in order: **Trackers**, **Cookies**, **Security**, **Pop-ups**, **Passwords**, **Permissions**, **Browsing Data**.

**Trackers**

- **"Tracking Protection"** (`TRACKING_PROTECTION_ROW`), a combo of `TrackingProtection::ALL` labels:
  - "Off": "Sites can load trackers";
  - "Standard" (the default): "Blocks known advertising and analytics trackers on other sites";
  - "Strict": "Also blocks social media trackers, such as Like and Share buttons. Some sites may not work".

  The subtitle is the chosen level's description. Choosing calls `browser.set_tracking_protection(level)`, which writes `privacy.tracking_protection` (synced) and calls `trackers().apply()` (WebKit content blockers on every tab). It follows sync. Sites with tracking protection off appear in Site Permissions as "Trackers".
- (KEEP) The setting and its wording.
- (NEEDS-CEF, INFERENCE) How it is enforced: per-request blocking in `CefResourceRequestHandler::OnBeforeResourceLoad` or `CefRequestHandler::GetResourceRequestHandler` against core's tracker list. That belongs to the trackers inventory.

**Cookies**

- Three **radio rows**, not a combo, because core's labels are too long. Each row has a check-button prefix in one group, title = `ThirdPartyCookies::label()`, subtitle = `description()`:
  - "Allow third-party cookies" / "Sites can use cookies from other sites they embed, which can track you across sites";
  - "Block third-party cookies in private windows" (the **default**) / "In private windows, sites can't use cookies from other sites they embed";
  - "Block third-party cookies" / "Sites can't use cookies from other sites they embed. Some sites may not work".

  Choosing calls `browser.set_third_party_cookies(choice)`, which writes `privacy.third_party_cookies` (synced) and calls `cookies().apply()`, setting the WebKit sessions' `CookieAcceptPolicy` (`Always` or `NoThirdParty`). It follows sync.
- (WEBKIT-ONLY) "WebKitGTK sets one policy for every site, so a site's Allow does not lift it". That is why `cookies::site_choices(.., exceptions=false)` hides Allow on Linux.
- (NEEDS-CEF, INFERENCE) Chromium has per-site exceptions: `CEF_CONTENT_SETTING_TYPE_COOKIES` with `set_content_setting(url, url, …)` (verified in bindings). The global mode probably maps to Chromium's pref `profile.cookie_controls_mode` (0 allow, 1 block third-party in incognito, 2 block third-party), which lines up one-to-one with `ThirdPartyCookies::{Allow, BlockInPrivate, Block}`. So on CEF, `site_choices(.., exceptions=true)` would offer Allow, as on Windows.
- **"See All Site Data"** row → the Site Data subpage (§4).

**Security**

- **"Always Use Secure Connections"** (`HTTPS_ONLY_ROW`), a switch with subtitle `https_only::DESCRIPTION` = "Upgrade navigations to HTTPS and warn you before loading sites that don't support it". Pref `privacy.https_only` (synced, default false); plain `set_pref`, read at navigation time. (KEEP) The enforcement is core's `https_only` plus the shell's navigation hook (CEF: `CefRequestHandler::OnBeforeBrowse` or `OnBeforeResourceLoad`; an inference, in another inventory).
- **"Secure DNS"** (`SECURE_DNS_ROW`), an informational row with subtitle "Your system's network settings control DNS, and whether it is secure". The self-test only requires a non-empty subtitle.
  - (WEBKIT-ONLY) The wording reflects WebKitGTK using the system resolver.
  - (NEEDS-CEF, INFERENCE) Chromium has its own DNS-over-HTTPS (prefs `dns_over_https.mode` and `dns_over_https.templates`). The Windows shell's wording is "Vsesvit looks sites up with secure DNS when your DNS provider supports it. Windows network settings choose the provider." Keeping a description-only row is a decision for the rewrite.

**Pop-ups**

- **"Block Pop-ups"**, a switch with subtitle "Sites can still open windows when you click". Pref `content.block_popups` (synced, default true) → `set_engine_switch` → `webkit::Settings::set_javascript_can_open_windows_automatically(!block)`.
- (NEEDS-CEF, INFERENCE) `CefLifeSpanHandler::OnBeforePopup`, whose `user_gesture` argument lets the shell refuse popups without a gesture, or the default `CEF_CONTENT_SETTING_TYPE_POPUPS` via `set_content_setting(null, null, …)` (verified in bindings).

**Passwords**

- A notice row with prefix icon `dialog-password-symbolic`: **"Vsesvit Doesn't Save Passwords"** (`PASSWORDS_NOTICE`) / "Use a password manager such as Bitwarden or Proton Pass through its browser extension". The self-test requires it to be mapped on the Privacy page.
- (KEEP) This is a product decision.
- (NEEDS-CEF, INFERENCE) Chromium has a built-in password manager that must stay off (likely prefs `credentials_enable_service=false` and `profile.password_manager_enabled=false`). Otherwise the notice would be false.

**Permissions**

- **"Site Permissions"**, an activatable row with subtitle "Camera, microphone, location, notifications and more" and a `go-next-symbolic` suffix → the Site Permissions subpage (§5).

**Browsing Data** (title "Browsing Data", description "History, cookies and cached files")

- **"Clear Browsing Data…"**, a button row with a `user-trash-symbolic` start icon. It runs `confirm`: "Clear Browsing Data?" / "History on every synced device, and this device's cookies, site storage and cache." / "_Clear" (destructive, default Cancel). On accept:
  1. `core.history().delete_range(0, now_ms())`, which writes a **synced history deletion directive** for all time. On error the **window** toast is "History: {e}".
  2. WebKit `website_data_manager().clear(WebsiteDataTypes::ALL, 0 s)` on the profile's network session, asynchronously. Errors are only logged.
  3. The window toast "Browsing data cleared".

  It does not clear downloads, favicons, bookmarks, the session, or the private session.
- (KEEP) The flow and wording.
- (NEEDS-CEF, INFERENCE) CEF has no single "clear browsing data" call. Candidates: `CefRequestContext::ClearHttpCache` (verified), `CefCookieManager::DeleteCookies("", "")` (verified), and DevTools methods through `CefBrowserHost::ExecuteDevToolsMethod` (verified), such as `Storage.clearDataForOrigin` per origin (the Windows shell uses it for site data), `Network.clearBrowserCache` and `Network.clearBrowserCookies`. Service workers, IndexedDB and localStorage for every origin need the origin list, from cookie domains or history.

### 2.7 Shortcuts page

See §6.

### 2.8 Absent rows (and why)

- `autofill.forms` has **no row on Linux**. The file's doc says "WebKitGTK fills no forms". (WEBKIT-ONLY) The Windows shell shows "Save and fill form entries such as addresses" bound to `autofill.forms` (synced, default true). With Chromium, autofill exists (INFERENCE: prefs such as `autofill.profile_enabled`), so the CEF shell can show the row, as Windows does.
- `media.player` and `media.picture_in_picture` are Windows-only rows (Show media player, Picture-in-picture). Linux has no media player, so a PiP site setting synced from Windows is **hidden** from Site Permissions (`permissions::listed`).

### 2.9 Every pref key the dialogs read or write

VERIFIED in `crates/vsesvit-core/src/prefs.rs` (`keys`), `extensions/private.rs` and `extensions/notifications.rs`.

| Key | Scope | Default | Written by (dialog) | Live effect on write |
|---|---|---|---|---|
| `startup` | synced | `restore_session` | General > On Startup | none |
| `homepage` | synced | `"about:home"` | General > Homepage | none (read on Home / startup) |
| `downloads.directory` | local | `None` (XDG Downloads) | General > Download Folder | downloads controller reads it per download |
| `downloads.ask` | synced | `false` | General > Ask Where to Save Each File | read per download |
| `spellcheck.enabled` | synced | `true` | General > Check Spelling | `Engine::apply_prefs` |
| `spellcheck.languages` | synced | `None` (system) | General > Languages | `Engine::apply_prefs` |
| `scrolling.smooth` | synced | `true` | General > Smooth Scrolling | `Engine::apply_prefs` (WebKit: live) |
| `system.hardware_acceleration` | local | `true` | General > Hardware Acceleration | `Engine::apply_prefs` (WebKit: live) |
| `performance.memory_saver` | local | `true` | General > Memory Saver | sweep reads it |
| `performance.memory_saver.mode` | local | `balanced` | General > Memory Savings | sweep reads it |
| `updates.channel` | local | channel of build | General > Update Channel | `Updates::set_channel` (re-check) |
| `updates.automatic` | local | `true` | General > Automatic Updates | `Updates::set_automatic` |
| `sync.types` | local | all five | Sync > Customize Sync | `Syncer::types_changed` (sync now) |
| `sync.server` | local | `https://vsesvit-service.mrquantumoff.dev` | Sync > Sync Server; Welcome > Sync | used at next sign-in |
| `sync.passphrase_offered` | local | `false` | passphrase offer (answered either way) | stops re-offering |
| `theme` | synced | `system` | Appearance > Theme | `apply_theme` |
| `tabs.position` | synced | `left` | Appearance > Tab Position | every window re-laid out |
| `bookmarks_bar.visible` | synced | `true` | Appearance > Show Bookmarks Bar | every window |
| `toolbar.home_button` | synced | `false` | Appearance > Show Home Button | every window |
| `search.default` | synced | built-in default (DuckDuckGo per tests) | Search > Search Engine; engine menu "Make Default"; Welcome > Search | via `SearchEngines::set_default` |
| `address_bar.compact` | synced | `true` | Search > Compact Address Bar | every window |
| `address_bar.full_urls` | synced | `false` | Search > Always Show Full URLs | every window |
| `address_bar.suggest.search` | synced | `true` | Search > Search Suggestions | read per keystroke |
| `address_bar.suggest.history` | synced | `true` | Search > Browsing History | read per keystroke |
| `address_bar.suggest.bookmarks` | synced | `true` | Search > Bookmarks | read per keystroke |
| `privacy.tracking_protection` | synced | `standard` | Privacy > Tracking Protection | `trackers().apply()` |
| `privacy.third_party_cookies` | synced | `block_in_private` | Privacy > Cookies radios | `cookies().apply()` |
| `privacy.https_only` | synced | `false` | Privacy > Always Use Secure Connections | read per navigation |
| `content.block_popups` | synced | `true` | Privacy > Block Pop-ups | `Engine::apply_prefs` |
| `keyboard.shortcuts` | synced | `{}` | Shortcuts page, capture dialog, Reset All | `keymap::apply` (all accelerators) |
| `onboarding.done` | local | `false` | Welcome (on close) | none |
| `extensions.allowed_in_private` | synced | `{}` | Extensions > Allow in private windows | runtime + private windows' toolbars |
| `extensions.notifications_off` | synced | `{}` | Extensions > Notifications | runtime closes and refuses notifications |
| `autofill.forms` | synced | `true` | *(no Linux row)* | — |

Outside prefs:

- `profiles/profiles.json` (`show_picker`), from the General > Profiles picker switch.
- Core DB tables: search engines, site permissions, bookmarks, history deletion directives, downloads, extensions (enabled, withheld, approved).
- The sync account and secrets, through `vsesvit_sync` (passphrase and delete flows).
- WebKit's website data under `ProfilePaths::engine_data` and `engine_cache`, cleared by Clear Browsing Data and Site Data.

---

## 3. Search engines: `search_engines.rs` (inside Settings > Search)

- **Group on the Search page** (untitled):
  - **"Search Engine"**, a combo with subtitle "Used for words typed in the address bar", listing every engine by `name`. The order is `SearchEngines::list()`: built-ins overlaid with stored records, minus tombstones, sorted case-insensitively by name, then by id. The default is selected. Choosing calls `search_engines().set_default(id)` (pref `search.default`). A `filling` guard ignores the selection changes caused by refilling.
  - **"Manage Search Engines"**, an activatable row with subtitle "Add search engines and the shortcuts that search with them" and a `go-next-symbolic` suffix. It pushes the **"Search Engines"** subpage (tag `search-engines`). The same subpage object is pushed each time, so its rows persist.
- **Subpage**: one group with description "Type a shortcut and a space in the address bar to search with its engine" and a header-suffix flat button **"_Add"** (`list-add-symbolic` plus label).
  - One row per engine:
    - title "{name}", or "{name} (Default)" for the default;
    - subtitle `EngineForm::of(engine).url`, the search URL with `%s` where the terms go, on one line, not markup;
    - suffix: the shortcut (keyword) as a dimmed label;
    - suffix: a **"More Actions"** menu button (`view-more-symbolic`) with **"Make Default"**, **"Edit…"**, **"Remove"**. The default engine's menu has **only "Edit…"**. Actions live in the row's `engine` action group (`engine.make-default`, `engine.edit`, `engine.remove`); the self-test activates `engine.remove` by name.
  - Make Default calls `set_default`. Remove calls `search_engines().remove(id)`, which core refuses for the default (`Error::RemoveDefaultEngine`) and which tombstones built-ins too. A failure gives the dialog toast "Search engines: {e}". The list is shown again at idle.
  - It follows sync and pref changes (`watch_prefs`). The test `the_list_shows_every_engine_and_follows_changes` checks this.
- **Editor**: an `adw::AlertDialog` titled **"Add Search Engine"** or **"Edit Search Engine"**, at least 360 px wide.
  - A boxed list of three entry rows (Enter activates the default response):
    - **"Name"**;
    - **"Shortcut"** (no spell check);
    - **"URL with %s in Place of Query"** (no spell check, URL input purpose).
  - An error label below the list.
  - Responses `_Cancel` and **`_Add`** or **`_Save`** (suggested, default); close counts as cancel. The Name field is focused.
  - Validation runs on open and on every change: `search_engines().check(editing, form)` returns at most one `FormError` per field. Errors and wording (VERIFIED, `search.rs:452-468`):
    - "Enter a name", "Enter a shortcut", "Enter a URL": *blank* errors. They keep Save disabled but are **not shown**.
    - "A shortcut is one word, without spaces".
    - "Another search engine has this shortcut". The engine being edited does not clash with its own shortcut.
    - "Put %s where the search terms go".
    - "Not a web address".

    Non-blank errors are joined with newlines in the error label, and the fields they belong to get the `error` style. Save is enabled iff there are no problems at all.
  - A URL without a scheme gets `https://` (core; the test turns `crates.io/search?q=%s` into `https://crates.io/search?q=serde`).
  - Saving calls `search_engines().save(editing, form)`. A failure gives the dialog toast "Cannot save the search engine: {e}".
  - Engines added in the editor have no suggestion URL, as in Chrome.
- (KEEP) All of it, through core `vsesvit_core::search::{SearchEngines, EngineForm, FormField, FormError, SearchEngine, SearchEngineId}`. The address bar re-reads the engine list on each classify, so a new shortcut works at once (`fx rust`).

---

## 4. Site Data subpage: `site_data.rs` (Settings > Privacy > Cookies > "See All Site Data")

- The row: **"See All Site Data"** (`SEE_ALL_ROW`), subtitle "Cookies and other data sites keep on this device", `go-next-symbolic`. It pushes a fresh **"Site Data"** subpage (tag `site-data`) each time.
- Contents are filled asynchronously from `engine.session().website_data_manager().fetch_future(WebsiteDataTypes::ALL)`. Records without a name are dropped and the rest are sorted by name.
  - Empty: a status page (icon `security-high-symbolic`) "No Site Data" / "Sites that keep cookies or other data on this device show here."
  - Otherwise:
    - A group with a destructive button row **"Remove All…"** (`user-trash-symbolic`). It runs `confirm` "Remove All Site Data?" / "Sites will forget you, and you will be signed out of them." / "_Remove", then removes every listed record.
    - A group with one row per site: title = the record name (a registrable domain, such as `127.0.0.1`), subtitle = `cookies::holds(types)`, a trash "Remove" button. `holds` gives a capitalized comma list of "cookies", "site storage" (local/session storage, IndexedDB, DOM cache, appcache), "service workers" and "cached files" (disk/memory cache), or "Other site data". Example: "Cookies, site storage".
  - Removing runs `cookies::remove_data(manager, ALL, records)` → `WebsiteDataManager::remove`, then refills.
  - It lists the **profile (normal) session only**, not the private session.
- (KEEP) The user-facing feature: a list by site, what each holds, remove one, remove all.
- (WEBKIT-ONLY) `WebsiteDataManager`, its record types and the `holds()` wording table are tied to WebKit's data types.
- (NEEDS-CEF, INFERENCE) Chromium/CEF has no "list every site with stored data" API. The Windows shell (WebView2, also Chromium) already does a cookie-based version: core's `cookies::group_by_site(domains)` → `SiteCookies { site, count }`, with rows like "example.com · 3 cookies", a Delete button per site and Delete all. Deleting runs DevTools `Storage.clearDataForOrigin` (`storageTypes: "all"`) for the `http://` and `https://` origins of each site, then deletes the cookies (`vsesvit-winui/src/dialogs/site_data.rs:80-110`). The CEF equivalents are `CefCookieManager::VisitAllCookies` and `DeleteCookies`, plus `CefBrowserHost::ExecuteDevToolsMethod("Storage.clearDataForOrigin", …)` (all verified in bindings). Sites that hold storage but no cookies would not be listed, which matches Windows.

---

## 5. Site Permissions subpage (in `settings.rs`)

- A navigation page **"Site Permissions"** (tag `site-permissions`), rebuilt from core each time it opens and after each change (at idle).
- Data: `core.site_permissions().by_site()` returns `Vec<SiteGroup { origin, heading, settings }>`. `heading` is the host, or the whole origin when two listed origins share a host (http vs https). Settings are in `Permission::ALL` order. The shell filters each site's settings with `permissions::listed(p)` (everything except `PictureInPicture`) and drops sites left with none.
  - The test `picture_in_picture_synced_from_windows_is_not_listed`: a site with only a PiP setting shows "No Site Permissions".
- Empty: a status page (icon `security-high-symbolic`) **"No Site Permissions"** / **"Sites you allow or block show here."** (the self-test asserts the description).
- Otherwise there is a preferences group per site (title = heading). Each stored permission is a **combo row** titled `permission.label()`:
  - "Camera", "Microphone", "Location", "Notifications", "Screen sharing", "Clipboard", "MIDI devices", "Trackers", "Insecure connections", "Cookies and site data".
  - Choices are `permission.settings()`: Allow and Block where Allow is remembered, Block only for screen sharing, Allow only for Insecure connections. For **Cookies** the choices are `cookies::site_choices(Some(current), false)` without the `None` default, so: Allow only if it is the current value (synced from Windows), then Block, then "Clear on exit".
  - Labels: "Allow", "Block", "Clear on exit". The current setting is selected.
  - A trash **"Remove"** button sets the setting to `None`. The site asks again, Trackers goes back to protected, and Cookies goes back to following Settings.
- After any change: `site_permissions().set(origin, permission, setting)` (errors logged), then `permissions::enforce(browser)`, which stops captures that are no longer allowed, answers waiting prompts and reseeds notification permissions. Then `trackers().apply()` for Trackers and `cookies().apply()` for Cookies.
- (KEEP) The list and its semantics, through core `vsesvit_core::permissions::{SitePermissions::by_site, set, Permission, Setting, Origin, SiteGroup}` and `vsesvit_core::cookies::site_choices`.
- (NEEDS-CEF, INFERENCE) The enforcement hooks belong to the permissions and cookies inventories: `CefPermissionHandler::OnRequestMediaAccessPermission` and `OnShowPermissionPrompt` (verified), `set_content_setting` for cookies, notifications and geolocation, and closing live captures. With CEF, PiP might become possible, which would make `permissions::listed` obsolete.

---

## 6. Settings > Shortcuts page and the capture dialog: `shortcut_settings.rs`

**Page** (name `shortcuts`)

- One group per `vsesvit_core::shortcuts::Section::ALL` ("Tabs and Windows", "Navigation", "Page", "General"). Each lists **only the commands the shell can reassign**: `keymap::actions()`, the commands whose binding is `Binding::Action`, in `Command::ALL` order.
  - The GTK shell left out `SelectTab1…8` and `SelectLastTab` (no binding) and listed `NextTab`/`PreviousTab` as built-in, fixed and not reassignable (`<Control>Tab`, `<Control><Shift>Tab`). The CEF shell decides its own binding table. Core has all of them.
- Row:
  - title = command title (for example "History", "Downloads", "New tab");
  - an `adw::ShortcutLabel` with all current chords, space-separated, or the text **"Disabled"** when there are none;
  - a **"Reset to Default"** button (`edit-undo-symbolic`), visible only when the command differs from its defaults.
  - Activating the row opens the capture dialog.
- Then an **"Extension Shortcuts"** group, present only if any enabled extension declares commands: one row per extension command, title = command description or name, **subtitle = extension name**. `ExtensionShortcuts` comes from `keymap.extension_shortcuts(commands::extension_commands(installed))`.
- Last, an untitled group with a **"Reset All"** button row, sensitive only when the keymap differs from the default. It runs `confirm` "Reset All Shortcuts?" / "Every command gets its default shortcuts back." / "_Reset" → `browser.edit_keymap(Keymap::reset_all)`. This resets extension commands too, because they live in the same register.
- The page follows sync (`watch_prefs`).
- Every change goes through `Browser::edit_keymap(f)`: read the keymap, apply `f`, store it with `prefs().set_keymap(&edited)` (pref `keyboard.shortcuts`, one synced LWW register for the whole keymap), then `keymap::apply` re-binds every accelerator, including extension commands.

**Capture dialog** (`capture(parent, browser, target, done)`)

- An `adw::AlertDialog` with heading **"Set Shortcut"** and body **"Press the new shortcut for {title}"**.
  - Extra content: a centred `ShortcutLabel` preview, initially the current chords or "Disabled", and a dim note, initially **"Backspace removes the shortcut"**.
  - Responses "Cancel" and "Save" (suggested), **without mnemonics**, because Alt+letter is a shortcut being captured. Save starts disabled. Close counts as Cancel.
- While it is open, **every app accelerator is removed** (`keymap::suspend`), so any chord can be pressed. On close, `browser.apply_keymap()` restores them.
- A key controller in the capture phase on the dialog handles every non-modifier key press (`keymap::pressed`):
  - **Escape** alone: cancel and close.
  - **BackSpace** alone: save "no shortcut" and close.
  - **Return / KP_Enter / ISO_Enter** alone: save the pending chord, if any, and close.
  - A key with core's `Key` name forms a chord. Letters keep Shift. A shifted symbol is named by what it typed, so Ctrl+? is `Ctrl+Question`, not `Ctrl+Shift+Question`. A symbol core cannot name falls back to the key's base keyval, keeping Shift (Ctrl+! becomes Ctrl+Shift+1). `ISO_Left_Tab` is Tab.
  - A key core cannot use gives the note `NOT_A_KEY_NOTE` = "This key cannot be part of a shortcut".
  - A key that needs a modifier gives `NEEDS_MODIFIER_NOTE` = "A shortcut needs Ctrl or Alt, unless it is a function key".
- Offer rules (`Target::offer`):
  - Browser command: `keymap.offer(cmd, chord, fixed)`.
    - A chord held by a built-in fixed binding is refused: "Used by {title}, which cannot be changed".
    - A chord held by another command is accepted with the note "Also used by {title}. Saving moves it here." (the self-test checks Ctrl+T for History).
    - Otherwise the extension note `extensions.note_for_browser(chord)` = "Also used by {extension}: {command}. Saving moves it here."
  - Extension command: needs Ctrl or Alt, otherwise "An extension's shortcut needs Ctrl or Alt". Then `keymap.offer_extension(...)`. The browser's own chords cannot be taken.
- Save calls `Target::assign`, which gives a browser command **exactly one chord** (or none); `keymap.assign(cmd, chord)` takes the chord from other commands. Extension commands use `assign_extension`. Reset uses `reset` or `reset_extension`.
- The self-test drives the capture through `Capture::press(Pressed)` and reads `Capture::note()` (feature `self-test`).
- (KEEP) All semantics. The core APIs are `vsesvit_core::shortcuts::{Keymap::{chords, is_default, offer, assign, reset, reset_all}, Chord, Command, Section, NEEDS_MODIFIER_NOTE, NOT_A_KEY_NOTE}` and `vsesvit_core::extensions::commands::{ExtensionCommand, ExtensionShortcuts::{chord, is_default, note_for_browser}, Keymap::{extension_shortcuts, offer_extension, assign_extension, reset_extension}}`.
- (KEEP / toolkit) The GTK accelerator spelling (`<Control><Shift>y`, `keymap::accelerator`) is GTK-specific. GPUI needs its own keystroke ↔ `Chord` mapping and a way to suspend its keybindings during capture.
- (NEEDS-CEF, INFERENCE) With CEF, page focus swallows keys unless the shell sees them first in `CefKeyboardHandler::OnPreKeyEvent` (the keyboard-handling inventory). Extension command dispatch (`commands.onCommand`) is Chromium's own when Chromium runs the extension. That conflicts with core's resolution of extension shortcuts, which needs a decision.

---

## 7. Keyboard Shortcuts help: `shortcuts.rs`

- `adw::ShortcutsDialog` opened by `app.shortcuts` (Ctrl+? by default). One section per `Section::ALL` title. Each `Command` of that section with a GTK binding shows:
  - an `Action` binding as an item bound to that action, so it **always shows the live accelerators**;
  - a `BuiltIn` binding as an item with the fixed accelerator.

  Commands with no binding are omitted.
- (KEEP) A read-only list of every shortcut, drawn from the live keymap.

---

## 8. Sync passphrase dialogs and prompts: `passphrase.rs`

Wording comes from `vsesvit_sync::status` (VERIFIED, `status.rs:43-158`).

**Passphrase dialog** (`ask`)

- An `adw::AlertDialog` with heading `words.title` and body `words.body`, in wide layout.
- Extra content: a boxed list of `PasswordEntryRow`s (Enter activates the default response):
  - `words.field`;
  - plus `words.confirm` when a new passphrase is being set (typed twice).

  Below them, an error label.
- Responses: dismiss = `words.dismiss`, accept = `words.accept` (suggested, default). **Accept is disabled until the fields validate.** The first field is focused.
- The four variants (`passphrase_dialog(Encryption)`):
  - `Off`, setting one: title "Encrypt Sync with a Passphrase"; fields "Passphrase" and "Confirm passphrase"; accept "Set Passphrase"; dismiss "Cancel".
  - `Enter`: "Enter Your Sync Passphrase"; one field "Passphrase"; accept "Start Syncing"; dismiss **"Sign Out"**.
  - `Changed`: "Enter Your New Sync Passphrase"; "New passphrase"; "Start Syncing"; dismiss **"Sign Out"**.
  - `Ready`, changing it: "Change Your Sync Passphrase"; "New passphrase" and "Confirm new passphrase"; "Change Passphrase"; dismiss "Cancel".
  - `Checking`: no dialog.

  Bodies are the long texts in `status.rs:113-142`.
- Validation (`check_passphrase(text, confirm)`): "The passphrases don't match", or "Use at least 8 characters". The mismatch error shows **only once the confirmation field has text**; until then only the too-short error shows.
- A failure from the last attempt is shown under the fields with its **first letter capitalized**. The self-test expects "The passphrase is wrong" after a wrong passphrase.
- When dismiss is **"Sign Out"**, the dialog **cannot be closed** by Escape or by clicking outside (`can_close = false`). A response force-closes it. The self-test checks that `close()` returns false and the dialog stays. Otherwise close counts as dismiss.
- **It closes by itself**, returning "Gone", when the sync state no longer asks the same encryption question or when the window under it unrealizes (`choose_while`). The test `a_dialog_that_cannot_close_ends_unanswered_when_it_is_moot_or_its_window_closes` covers both.

**`take(parent, syncer, busy)`** (Settings' "Encrypt with a Passphrase…" and "Change Passphrase…", and the prompts)

- While `syncer.encryption()` is unchanged:
  - Ask.
  - Typed: `busy(true)`, `syncer.passphrase(p).await`, `busy(false)`. Success ends. An error is shown in the next ask.
  - Dismissed with "Sign Out": `syncer.act(SignOut)`.
  - Dismissed otherwise, or Gone: end.
- It returns `Err(last failure)` if the loop ended because the state changed after a failure.

**`prompt(window, syncer, Prompt)`** (shown by `Syncer` of its own accord, `sync.rs:208-236`)

- `Prompt::Enter`: `take(window, …)`.
- `Prompt::Offer`: the alert `OFFER`, with title "Encrypt Your Synced Data?", body "Your bookmarks, history, open tabs, extensions and settings sync unencrypted, so whoever runs the sync server can read them. Set a sync passphrase and Vsesvit encrypts them before they leave your devices. You can also do this later in Settings > Sync.", accept "Set Passphrase" (suggested, default) and decline "Not Now" (the close response).
  - It closes itself if the offer stops applying.
  - Any answer sets `sync.passphrase_offered = true`, so **it is offered once per profile**.
  - Accept runs `take` with the Off dialog.
- Syncer rules:
  - One prompt at a time.
  - Shown over the first **normal** window, never a private one.
  - **Held back while a welcome dialog is open** (`hold_prompts`). The due prompt shows once the welcome closes; the test `the_offer_waits_for_the_welcome_and_comes_once` covers this.
  - Settings' "Enter Passphrase…" (`act(EnterPassphrase)` → `prompt(asked=true)`) shows the prompt even while a welcome is open.
  - The next due prompt chains after the current one ends.
- (KEEP) All of it. No engine involvement.

---

## 9. First-run welcome: `welcome.rs`

**When**

- Automatically, once, on the launch that **created** the profile: `onboarding::should_show(profile) = profile.is_new() && !prefs[onboarding.done]`. Not in scripted (self-test) runs.
- It opens over the first normal window after startup windows and targets.
- Again whenever the user picks primary menu > **Welcome** (`app.welcome`).
- **Closing it at any page ends the first run**: `onboarding::finish` sets `onboarding.done = true` (local). Quitting midway does not bring it back.
- It holds the window weakly and does not keep the profile open.
- Sync prompts wait for it to close (`syncer.hold_prompts(&dialog)`).

**Frame**

- An `adw::Dialog` titled **"Welcome to Vsesvit"**, content **560×640**.
- A header bar with no title.
- A **carousel** that the user cannot swipe, scroll or drag. Pages change only through the buttons, with an animated scroll.
- A bottom bar: **Back** (start; hidden on the first page), carousel indicator **dots** (centre), **Next** (end; `suggested-action`; the dialog's **default widget**, so Enter activates it).
- Next's label per step: Welcome "Get Started"; Sync **"Skip"**; Done **"Start Browsing"**, which closes the dialog; all others "Next".
- A click during the scroll counts from the target page.

**Steps** (`STEPS`, in order; the self-test names them `welcome`, `search`, `import`, `extensions`, `default-browser`, `sync`, `done`)

Every page is an `adw::StatusPage` (icon, title, description, optional child clamped to 440 px), with the `compact` style except the first page.

1. **Welcome**: icon = the app icon `dev.mrquantumoff.vsesvit` if the icon theme has it, else `web-browser-symbolic`. Title "Welcome to Vsesvit". Description "A web browser that runs extensions from the Chrome Web Store. A few choices and you are ready to go."
2. **Search**: icon `system-search-symbolic`. Title "Search Engine". Description "What the address bar searches when you type words instead of an address." A group of **radio rows**, one per engine in `search_engines().list()`, titled with the engine name. The current default is checked. Choosing one calls `search_engines().set_default(id)` at once (errors logged).
3. **Import**: icon `user-bookmarks-symbolic`. Title "Import Bookmarks". Description "Bring your bookmarks from another browser. Each import goes into a folder of its own on the bookmarks bar."
   - Group 1: a row "Looking for other browsers…" with a spinner while `vsesvit_core::import::installed_browsers()` runs **on a worker thread**. This finds Chromium-family and Firefox profiles that have bookmarks; names look like "Google Chrome" or "Google Chrome (Work)".
     - Then one **task row** per browser found: title = browser/profile name, subtitle "Into “Imported from {name}” on the bookmarks bar", button "Import".
     - Or the row "No other browsers found".
   - Group 2: the task row **"Bookmarks File"** / "An HTML export, or a Chromium Bookmarks file" / button "Choose File…". The file chooser is titled "Import Bookmarks File", with filter "Bookmarks (.html, .json)" (suffixes `html`, `htm`, `json`, plus the exact name `Bookmarks`). The import goes into a folder named "Imported" (`import::FILE_FOLDER_TITLE`).
   - The import (`bookmarks::Import::run`): `Source::read()` on a worker thread (HTML, Chromium JSON by content, or Firefox `places.sqlite`), then `bookmarks().import_folder(folder, items)`, then `browser.bookmarks_changed()`.
   - Outcomes: "Nothing to import", "1 item imported", "{n} items imported". Errors: "Could not read {from}: {e}", "Could not read {from}", "Bookmarks: {e}", "The window is closed".
4. **Extensions**: icon `application-x-addon-symbolic`. Title "Extensions". Description "A few to start with, from the Chrome Web Store. The Extensions page adds more."
   - One task row per `onboarding::RECOMMENDED_EXTENSIONS` (name and blurb):
     - "uBlock Origin Lite", `ddkjiahejlhfcafbddmgiahcphecmpfh`;
     - "Bitwarden", `nngceckbapebfimnlniiiahkandclblb`;
     - "Proton Pass", `ghmbeldphafepmbegfdlkpapadhbakde`;

     all on `CrxStore::ChromeWebStore`.
   - Each row has an "Install" button, or "Installed" if `extensions().get(id)` already has it.
   - Install calls `browser.install(recommended.install_source(), |_| {})`. This is **the plain install without Chrome's install prompt**. Results: "Installed"; `Ok(None)` gives the error "Removed on another device while it downloaded"; any other error shows its text.
5. **Default Browser**: icon `web-browser-symbolic`. Title "Default Browser". Description "Open links from other apps in Vsesvit."
   - A status label, a pill suggested button **"Make Vsesvit the Default Browser"** (visible only when another browser is the default and this copy can register), and an error label.
   - State (`DefaultBrowser::now`):
     - Vsesvit is the default when the GIO default for `x-scheme-handler/https` has id `dev.mrquantumoff.vsesvit.desktop`: "Vsesvit is your default browser."
     - Inside Flatpak (`/.flatpak-info` exists): "Flatpak apps cannot change the default browser themselves. Choose Vsesvit as the web browser under Default Apps in your system settings."
     - Desktop entry not installed: "This copy of Vsesvit has no desktop entry installed, so the system cannot open links with it."
     - Otherwise: "Links from other apps open in {name}." or "No default browser is set."
   - The button runs `app.set_as_default_for_type` for `x-scheme-handler/http`, `x-scheme-handler/https` and `text/html` (this writes the user's `mimeapps.list` through GIO). A failure shows "Could not make Vsesvit the default: {e}". The page then refreshes.
6. **Sync**: icon `view-refresh-symbolic`. Title "Sync your browser". Description "Have your bookmarks, history, open tabs, extensions and settings on all your devices."
   - The **Sync Server** entry row from Settings (§2.3, same behaviour and validation).
   - A pill suggested **"Sign In"** button, visible only while `status.actions` contains SignIn. It calls `syncer.act(SignIn)`, which opens the sign-in page in a tab, and then **activates Next at once** (moves to Done without waiting).
   - When Sign In is not offered, a centred label shows the status title ("Signing in…", "Signed in as …").
7. **Done**: icon `object-select-symbolic`. Title **"You’re All Set"** (typographic apostrophe). Description **"Settings has these choices and more. The main menu’s Welcome brings this tour back."**

**Task row** (`TaskRow`)

- An action row whose suffix is a crossfading stack: button, then spinner, then dim outcome label.
- Every click runs the work. Outcomes:
  - `Ok(Some(text))` shows the text.
  - `Ok(None)`, the user backed out (for example cancelled the file chooser), brings the button back.
  - `Err(e)` puts the error in the subtitle with the `error` style, relabels the button **"Try Again"** and brings it back.
- The original subtitle is restored at each new run.

(KEEP) The whole flow and wording. The import, onboarding and search contracts are core's.

(NEEDS-CEF) only through `browser.install` (§10's engine loading).

(KEEP / toolkit, INFERENCE) Default-browser detection uses GIO `AppInfo`. GPUI has no equivalent, so the new shell needs `xdg-mime query default` / `xdg-settings set default-web-browser` or the `gio` crate without GTK.

---

## 10. Extensions dialog: `extensions.rs`

**Frame**

- An `adw::PreferencesDialog` titled **"Extensions"** with one page.
- Opened by `win.show-extensions`, or by `present_extension(window, id)` with that extension's row **expanded**; a notification's Settings button leads there, as Chrome's does.
- It holds the window weakly.

**Install group** (title "Install", description "Paste a store link or id, choose a .crx or .xpi file, or load a folder for development")

- Entry row **"Chrome Web Store, Edge Add-ons or Firefox Add-ons link or ID"**, with an apply button and URL purpose. On apply: trim; if non-empty, `InstallSource::parse(text)` and clear the field.
  - A parse error gives the toast with core's text, for example "not a Chrome Web Store, Edge Add-ons or addons.mozilla.org extension URL, an extension id, or an extension file" or "the store URL does not contain a valid extension id".
  - Sources accepted (PLAN.md and core): CWS URLs and ids, Edge Add-ons, AMO URLs and gecko ids (not bare slugs), local `.crx`/`.xpi`, and unpacked folders.
- Button row **"Install from _File…"** (`package-x-generic-symbolic`): a file chooser "Install Extension from File" with filter "Extension packages (.crx, .xpi)" → `InstallSource::from_path`.
- Button row **"Load _Unpacked Extension…"** (`folder-symbolic`): a folder chooser "Load Unpacked Extension" → `InstallSource::from_path`.
- Progress row **"Installing…"** with a spinner prefix, hidden unless installs are running. The subtitle is the latest `InstallPhase::describe()`: "Starting…" (shell), "Looking up the add-on…", "Downloading… {x} of {y}", "Verifying the signature…", "Unpacking…", "Reading the manifest…". **Concurrent installs share the row**, with a counter, and it hides when the last one ends (test `the_progress_row_stays_while_another_install_runs`).

**Install flow** (`Browser::install_asking`)

- `extensions().prepare_install(source)` on the UI thread, then `InstallJob::run` on a worker thread with phases relayed back, giving a `StagedInstall`.
- If `staged.needs_approval()` (a user's install from a store or a package, not an unpacked folder and not a sync reconcile), **Chrome's install prompt** (§11) is shown over the browser window. Anything but Add gives `InstallFailure::Cancelled`, which is silent.
- Then `extensions().commit(staged)`, `apply_keymap()` (to bind the extension's commands) and `load_into_runtime`.
- Toasts:
  - "Installed {name} {version}";
  - "The extension was removed elsewhere while it downloaded" (`Ok(None)`);
  - "Installed {name} {version}, but it cannot run: {e}" (committed but the runtime failed to load it);
  - "Install failed: {e}".

**Installed group** (title "Installed", header suffix flat button **"_Update"**, widget name `update-extensions`, tooltip "Check every extension for a newer version now")

- Rows: `browser.installed_extensions()` = `extensions().list()`. With none, a dim row "No extensions installed".
- Each extension is an **expander row**:
  - title = `manifest.name`;
  - subtitle **"{version} · {verification label}"**. Labels: "Chrome Web Store, publisher verified", "Edge Add-ons, publisher verified", "Firefox Add-ons, hash checked", "Local CRX, signature verified", "Local XPI, not verified", "Unpacked folder, not verified";
  - prefix: the 32-px icon from the **largest** manifest icon that exists on disk, else `application-x-addon-symbolic`;
  - not markup.
- Suffixes:
  - **Enabled switch** (tooltip "Enabled"), **insensitive while the extension has withheld permissions**. Toggling calls `browser.set_extension_enabled(id, on)`: core `set_enabled` (synced for store installs), `apply_keymap`, then load or unload in the runtime.
    - `EnableFailure::Load(e)` gives the toast "Enabled, but it cannot run: {e}" and the switch stays.
    - Any other error gives the toast "Cannot change the extension: {e}", the switch is **not** changed and the list refreshes (test `a_switch_that_cannot_change_the_extension_shows_what_the_profile_holds`).
    - If the running or error state flipped, the list refreshes after the handler returns.
  - **Remove** (trash, tooltip "Remove") calls `browser.uninstall_extension(id)` at once, **with no confirmation**. It unloads first, then core `uninstall`; if core refuses, it reloads. A failure gives the toast "Cannot remove the extension: {e}".
- Expanded children, in this order (each of the first three also forces the row expanded):
  1. (WEBKIT-ONLY) **"Not supported by the Linux runtime"** (warning style, `dialog-warning-symbolic`), subtitle "This extension requests: {list}. Features that depend on them will not work." The list comes from `vsesvit_webext::unsupported_features(manifest)`: unsupported permissions and optional permissions (for example `webRequest`), unsupported manifest keys, and `externally_connectable.matches`. This goes away with Chromium.
  2. **Re-enable**: present when `ext.withheld` is non-empty, that is, an update asked for more than the user approved. Title `permissions::re_enable_heading(name)` = **"Enable “{name}”?"**. Subtitle `re_enable_text`: "It can now:" then "• {warning}" lines, with each detail on a "    ◦ {detail}" line. Warning style. Button **"_Re-enable"** calls `browser.approve_extension_permissions(id)` (core `approve_permissions` grants what the update added, local to the device), then load. Toasts: "Re-enabled, but it cannot run: {e}" and "Cannot re-enable the extension: {e}".
  3. **"Not running"** (error style, `dialog-error-symbolic`): only when enabled and the runtime failed to load it. The subtitle is the load error, for example a missing file named in the manifest.
  4. **"Notifications"** switch with subtitle "Let this extension show notifications": only if `manifest.permissions` contains `"notifications"`. It shows `extensions().notifications_allowed(id)` (on until turned off). Toggling calls `extensions().set_notifications_allowed(id, on)` (synced pref `extensions.notifications_off`), then `runtime().notification_permission_changed(id)`, which closes its notifications and makes `chrome.notifications` refuse more; the extension's `onPermissionLevelChanged` fires "denied" or "granted". A failure gives the toast "Cannot change the extension's notifications: {e}" and a refresh.
  5. **"Allow in private windows"** (`ALLOW_IN_PRIVATE_ROW`) switch with subtitle "Vsesvit cannot stop it from recording what you do there". It shows `extensions().allowed_in_private(id)` (off until turned on). Toggling calls `browser.set_extension_allowed_in_private`: core writes synced pref `extensions.allowed_in_private`, then `runtime().allowed_in_private_changed()`, which joins or leaves open private tabs at once, and every private window refreshes its extension toolbar. A failure gives the toast "Cannot change the extension: {e}".
  6. **"ID"** property row: the extension id (selectable).
  7. **"Location"** property row: `ext.dir` (selectable, wraps).
- **Update button** (Chrome's Update): insensitive while running. It calls `browser.update_extensions()`: `prepare_update_check`, `UpdateCheck::run` on a worker, `commit_updates`, then for each updated extension `apply_keymap` + load, then `extensions_updated()`, which refreshes the open dialog through `watch_extensions`.
  - The toast is `UpdateReport::summary()`: "Your extensions are up to date", "Updated 1 extension", "Updated {n} extensions; {m} could not be updated", "{n} extensions could not be updated". When an update asks for more permissions, it adds "The newest version of the extension “X” requires more permissions, so it has been disabled. It can now: …".
  - Errors: "Cannot update the extensions: {e}", for example "an update check is already running".
  - The same check runs periodically: the first check a minute after startup if the last one is five hours old, then every five hours (`schedule_extension_updates`), logged only.
  - The extension updated with new warnings stays installed at the new version but **off**, with its switch insensitive and its code not running, until Re-enable (test `an_update_asking_for_new_permissions_waits_for_the_re_enable_button`).
- Refresh model: the dialog rebuilds its rows after installs and its own actions, and on `watch_extensions` (after update checks only). Changes that sync brings in (`reconcile_extensions`) do **not** refresh an open dialog. VERIFIED that `extensions_updated` is called only from `update_extensions`.

**Classification**

- (KEEP) The product UI: install by link, id, file or folder, with progress and the install prompt; the list with provenance, enable, remove, update and Re-enable; per-extension Notifications and Allow in private windows; ID and Location. Also core's install pipeline and its contract: `vsesvit_core::extensions::{InstallSource::{parse, from_path, web_store}, Extensions::{prepare_install, commit, list, get, set_enabled, uninstall, approve_permissions, allowed_in_private, set_allowed_in_private, notifications_allowed, set_notifications_allowed, prepare_update_check, commit_updates, next_update_check, reconcile}, StagedInstall::{needs_approval, manifest}, InstallPhase::describe, UpdateReport::summary, Verification::label, InstalledExtension{id, manifest, version, verification, enabled, withheld, dir}}`, `extensions::permissions::{re_enable_heading, RE_ENABLE_LEAD, PermissionMessage}`, and the synced records (enabled and installed registers, `extensions.allowed_in_private`, `extensions.notifications_off`).
- (WEBKIT-ONLY) The "Not supported by the Linux runtime" row and `unsupported_notice`. The `vsesvit_webext::LoadError` text in "Not running" is WebKit-runtime-specific.
- (NEEDS-CEF, risk) Everything that touches the runtime:
  - load and unload (`load_into_runtime` / `unload_from_runtime`);
  - the per-extension incognito switch (`allowed_in_private_changed`);
  - the per-extension notification permission;
  - load errors;
  - running updated versions;
  - binding extension commands.

  **CEF 154 exposes no extension API** (verified: no `cef_extension_t` or `LoadExtension` in the bindings). Chromium's extension service (enable/disable, incognito-enabled, uninstall, load unpacked, the updater) is reachable only through Chrome-runtime internals, the `chrome://extensions` WebUI, or command-line switches. INFERENCE: Chromium's own extension updater and its install-prompt UI may compete with core's update pipeline and §11's prompts. How the shell's dialog drives Chromium's extension system is the central open question for "total extension compatibility".

---

## 11. Extension prompts: `extension_prompts.rs`

**Install prompt** (`install_dialog(manifest)`), shown by `install_asking` when `needs_approval()`

- An `adw::AlertDialog` with heading `permissions::install_heading(name)` = **"Add “{name}”?"**. The extra child is a vertical list of labels:
  - the lead **"It can:"** (`INSTALL_LEAD`), only if there are warnings;
  - each warning "• {text}", indented 6;
  - each detail "◦ {site}", indented 24. Details are the sites behind "a number of websites".
  - (WEBKIT-ONLY) then a warning caption: "Not supported by the Linux runtime: {notice}. Features that depend on them will not work."
- Warnings come from `permissions::install_warnings(manifest)`: Chrome's message rules, coalesced (`<all_urls>` + `tabs` reads only "Read and change all your data on all websites"), with content-script sites counted.
- Responses `_Cancel` and **`_Add Extension`** (suggested). The **default response is Cancel**, and closing counts as Cancel. The `ADD` response id is `"add"`.
- Expected self-test lines for the probe: `["Add “Vsesvit Probe”?", "It can:", "• Read and change all your data on all websites", "• Display notifications"]`.
- The unit test expects `["Add “Reader”?", "It can:", "• Read and change your data on a.example", "• Read your browsing history", "Not supported by the Linux runtime: webRequest. …"]`.

**Request prompt** (`request_dialog(prompt)`), for `chrome.permissions.request`

- Heading `request_heading(name)` = **"“{name}” has requested additional permissions."**. The lead is **"It could:"** (`REQUEST_LEAD`), followed by the bullets.
- Responses `_Deny` and **`_Allow`** (suggested). The default is Deny, and closing counts as Deny. The `ALLOW` id is `"allow"`.
- It is shown over the window the request came from. vsesvit-webext decides that window: never a private window the extension does not run in.

`shown_lines(dialog)`: the heading plus the label texts in order, for the self-test.

- (KEEP) Chrome's prompt wording and the rules for when to ask (`needs_approval`, and asking during a user gesture only for permissions with warnings). These are core's `vsesvit_core::extensions::permissions::{install_heading, request_heading, install_warnings, warnings, INSTALL_LEAD, REQUEST_LEAD, PermissionMessage}`.
- (WEBKIT-ONLY) The unsupported caption, and `vsesvit_webext::permissions::Prompt { extension, name, warnings }` as the request's source.
- (NEEDS-CEF, risk, INFERENCE) With Chromium running extensions, `permissions.request` prompts come from Chromium's own UI (a Views dialog), which CEF does not expose for a custom-drawn shell. Whether Chromium's dialogs appear, can be suppressed, or can be replaced by these core-worded prompts is an open question.

---

## 12. Bookmarks window: `bookmarks.rs`

**Frame**

- A `LibraryWindow` titled **"Bookmarks"** with search placeholder **"Search bookmarks"**, 640×680. It is a window of its own (`Windowed::Bookmarks`).
- Header end buttons, appearing left to right as: **New Folder** (`folder-new-symbolic`), **Edit…** (`document-edit-symbolic`), **Move To…** (`go-jump-symbolic`), **Delete** (`user-trash-symbolic`), and the **main menu** (`open-menu-symbolic`, tooltip "Main Menu"). The menu has **"_Import Bookmarks…"** (`bookmarks.import`) and **"_Export Bookmarks…"** (`bookmarks.export`), as in Chrome's bookmark manager. The self-test asserts exactly these two items.

**Tree view**

- A `ListView` over a `TreeListModel` in sidebar style.
- The roots are the children of `BookmarkId::ROOT`: the bookmarks bar, Other bookmarks and Mobile bookmarks, with core's titles.
- The **first build expands the three roots**. Every rebuild keeps the folders that were expanded.
- A folder's children are fetched **lazily** when it expands, with their stored favicons (`favicons::stored(profile, url)` → `profile.favicons().get(url)`, PNG bytes).
- Row:
  - an icon: the favicon, else `folder-symbolic`, `web-browser-symbolic` or `view-more-horizontal-symbolic` (separator);
  - the title, ellipsized at the end;
  - for URLs, a dim caption with `display_uri(url)`, ellipsized in the middle and hidden when empty.

  A separator's title is "—". Rows are indented by depth and have an expander arrow.
- **Activation** (double-click or Enter) on a URL opens it **in a new foreground tab** of the opener window (`window.open_tab(url, None, Focus::Foreground)`). On a folder, activation does nothing beyond the expander.
- Rebuilds happen on `Browser::watch_bookmarks`, which fires after every bookmark write anywhere (bar, star, sync, this window) and when favicons arrive.

**Search**

- With non-empty trimmed text, the tree is replaced by a results list: `bookmarks().search(text, 60)`, URL nodes only. Each row has the favicon, title, `display_uri` subtitle, and activation that opens a new foreground tab.
- No matches: a status page "No Matching Bookmarks" (`edit-find-symbolic`).
- Clearing the text brings the tree back.
- Results follow changes: a rebuild re-runs the search. The test `a_search_hides_the_tree_selection_and_follows_changes` covers this.
- While results show, **toolbar actions do not act on the hidden tree selection**.

**Toolbar actions** (each a single core call, then `report()`: toast "Bookmarks: {e}" on error, then `browser.bookmarks_changed()`, which refreshes every window's bar and star, this window, and favicon preloading)

- **New Folder**: the parent is the selected folder, or the selected item's parent, or the **bookmarks bar** when nothing is selected. `prompt_text("New Folder", initial "New Folder", accept "_Create")` → `bookmarks().add_folder(parent, InsertAt::End, title)`.
- **Edit…**: needs a selected non-root, non-separator node, otherwise the toast "Select a bookmark or folder to edit". It opens `bookmark_editor::edit(window, core, Subject::Existing(node))`, the same dialog the bookmarks bar uses (name, URL and folder for a bookmark; name and parent for a folder). It saves through core `rename`, `set_url` and `move_to`.
- **Move To…**: needs a non-root node, otherwise the toast "Select a bookmark or folder to move". It lists every live folder in tree order as a path such as "Bookmarks Bar / Work / …", at most `MAX_DEPTH` (64) levels below a root, **excluding the node's own subtree**. `prompt_choice("Move To", "Move “{title}” into:", …, "_Move")` → `bookmarks().move_to(id, target, End)`. The test `move_to_folders_stop_at_the_maximum_depth` checks that the walk handles 200-deep chains.
- **Delete**: needs a non-root node, otherwise the toast "Select a bookmark or folder to delete". A **folder** asks `confirm("Delete Folder?", "“{title}” and everything in it will be deleted.", "_Delete")`. A bookmark or separator is deleted without asking. Calls `bookmarks().remove(id)`.

**Drag and drop** (`bookmark_drag`)

- Every row except the roots is draggable (`DragAction::MOVE`, payload = the node id as a string, drag icon = the row's snapshot).
- Every row is a drop target, with zones along the vertical axis (`bookmark_drag::zone`):
  - a folder's **middle half** means Into, its outer quarters mean Before or After;
  - roots accept only Into;
  - other rows split at half into Before and After.

  CSS classes `drop-before`, `drop-into` and `drop-after` show the indicator.
- A drop runs `bookmark_drag::apply` → `destination()`. The index is adjusted when moving down within the same parent, and a no-op is skipped. Then one core `move_to`. An error such as `BookmarkError::WouldCycle` gives the toast. The tree keeps its expanded folders (test `dropping_rows_moves_bookmarks_in_core`).

**Import** (menu)

- `prompt_choice("Import Bookmarks", "The bookmarks go into a new folder on the bookmarks bar.", [found browser names…, "Bookmarks File (HTML)…"], "_Import")`. Here `import::installed_browsers()` runs **synchronously on the UI thread**; the welcome uses a worker.
- Choosing the file entry opens the same file chooser as the welcome (§9).
- Then `Import::run`. Toasts: "No bookmarks found in {from}", "Imported {n} items from {from} into “{folder}”", or the error text. The folder is "Imported from {name}" or "Imported".

**Export** (menu)

- A save dialog "Export Bookmarks", starting in `export::default_folder()` (Documents, else home), with the suggested name `export::file_name(y, m, d)`. That is Chrome's `bookmarks_{M}_{D}_{YY}.html`, for example `bookmarks_10_6_26.html`.
- `export::html(&bookmarks())` (Netscape format laid out as Chrome writes it: bar as the toolbar folder, then Other, then Mobile if non-empty; no favicons) is written on a worker thread.
- Toasts: "Exported bookmarks to “{file name}”" or "Could not export bookmarks: {e}".
- `export_to(browser, path)` is `pub(crate)` so the self-test can call it.

**Lifetime**: the window owns its state until it unrealizes. Handlers hold it weakly, and it holds the browser window weakly (test `closing_the_window_drops_its_state`).

**Classification**

- (KEEP) Everything above, through core `vsesvit_core::bookmarks::{Bookmarks::{children, search, get, add_folder, move_to, remove, import_folder}, BookmarkId::{ROOT, TOOLBAR, OTHER, MOBILE, is_root}, BookmarkNode, NodeKind, InsertAt, MAX_DEPTH}`, `vsesvit_core::import::{installed_browsers, Found::folder_title, Source::read, FILE_FOLDER_TITLE, parse_html}`, `vsesvit_core::export::{html, file_name, default_folder}`, and `profile.favicons().get`.
- (WEBKIT-ONLY) `tab::display_uri` uses `webkit::functions::uri_for_display`. The platform-neutral replacement is core's `vsesvit_core::address::readable_url`, which the Windows shell uses; it is VERIFIED to exist. Note that `display_uri` also maps `about:blank` to "".
- No CEF involvement apart from opening tabs.

---

## 13. History window: `history.rs`

**Frame**

- A `LibraryWindow` titled **"History"** with placeholder **"Search history"**, 640×680. One header button: **"_Clear…"** (destructive style, tooltip "Clear browsing history").
- Content, scrollable, top to bottom: the **devices section**, the heading **"Recent History"**, then the visits list or the empty page.

**Tabs from other devices** (hidden, with its heading, while searching)

- A group **"Tabs from Other Devices"**:
  - Not signed in to sync: description "Sign in to sync to see tabs from your other devices."
  - Signed in with no devices: "Tabs from your other devices appear here when they sync."
- Then one group per device from `core.session().other_devices()`: sessions updated within **14 days**, newest first. Group title = `device_name`; description **"Synced {ago}"** (`vsesvit_sync::status::ago`: "just now", "1 minute ago", "5 minutes ago", "2 hours ago", "3 days ago"; unit test `a_device_says_how_long_ago_it_synced`).
- Rows: every tab of every window of that device. Title = tab title, or `display_uri(url)` when the title is blank. Tooltip = the URL. Prefix = the stored favicon or `web-browser-symbolic`. Activation opens the URL in a new foreground tab.

**Visits**

- With an empty query: `history().visits_between(0, now+1, 300)`, visits newest first. Each row's time is that **visit's** time.
- With a query: `history().search(query, 100)` (URL-prefix or title match, by frecency). The time is the page's **last visit**.
- Row:
  - title = page title, or `display_uri(url)`;
  - subtitle **"{format_time(at)} · {display_uri(url)}"**;
  - prefix: the generic `web-browser-symbolic`, **not a favicon**;
  - suffix: a trash button **"Forget this page"**, which calls `history().delete_url(url)`. That writes a synced deletion directive for the URL over all time. A failure gives the toast "History: {e}". The list refreshes.
  - Activation opens the URL in a new foreground tab.
- Empty: a status page "No History" / "Pages you visit appear here" (`document-open-recent-symbolic`).

**Clear…**

- `prompt_choice("Clear Browsing History", "Visits in the chosen range are forgotten on this device and on every synced device.", ClearRange labels, "_Clear")`. The labels are "Last hour", "Last 24 hours", "Last 7 days", "Last 4 weeks", "All time".
- It calls `history().delete_range(range.start(now), now)`, a synced deletion directive. A failure gives the toast "History: {e}". The list refreshes.
- It does not touch cookies or the cache (Settings' Clear Browsing Data does that).

**Refresh**

- On search changes, after its own actions, and on `Browser::watch_history`.
- `watch_history` fires **only when sync applies history or session changes**. VERIFIED: an open History window does not update as the user browses.

**Classification**

- (KEEP) All of it, through core `vsesvit_core::history::{History::{visits_between, search, delete_url, delete_range}, ClearRange::{ALL, label, start}, HistoryEntry}`, `vsesvit_core::session::{Session::other_devices, DeviceTabs{device_name, updated_ms, windows}, TabSnapshot}`, `vsesvit_sync::status::ago`, and `browser.sync().is_signed_in()`.
- (WEBKIT-ONLY) `display_uri`; use `address::readable_url` instead.
- No engine involvement.

---

## 14. Downloads window: `downloads.rs`

**Frame**

- An `adw::Window` titled **"Downloads"**, **560×560**. It is a window of its own per browsing kind (`Windowed::Downloads(Browsing::Normal|Private)`).
- Header: **start** "Open Download Folder" (`folder-open-symbolic`), which creates the folder if missing and opens it in the file manager. **End** **"_Clear List"** (tooltip "Remove every finished download from the list"), which calls `downloads.clear(browsing)` → core `downloads().clear(browsing)`. That removes every *final* entry the window lists, and the files stay on disk.

**List**

- `downloads.list(browsing)` = core `downloads().list(500, browsing)`, newest first.
- A **private** window lists every download, normal and private; a normal window never lists a private one (`downloads::listed_in`).
- Empty: a status page "No Downloads" / "Files you download appear here" (`folder-download-symbolic`).
- Row:
  - title = the file name (not markup);
  - subtitle = **`downloads::status_line(download, live, exists)`**, Chrome's wording, the same in both shells;
  - prefix = a symbolic icon for the file's content type, guessed from its name (`gio::content_type_guess`).

  `status_line` texts:
  - in progress: "1.3 MB/s - 3.2 MB of 10 MB, 5 secs left", without the time left while nothing arrives or the size is unknown;
  - paused: "0 B/s - … , Paused";
  - "Interrupted · {counts}";
  - "This type of file can harm your device" (unconfirmed);
  - "Deleted" (completed but the file is gone);
  - "{size} · {host}";
  - "Failed";
  - "Cancelled".
- Suffixes by state:
  - **Live** (`is_live`: in progress, paused, interrupted): a 120-px progress bar (a fraction when the total is known, otherwise it pulses on each update) and a **"Cancel"** button (`process-stop-symbolic`) → `downloads.cancel(id)`, which cancels the engine download.
  - **Unconfirmed** (a dangerous type held under `<name>.unconfirmed`): flat **"_Keep"**, which calls core `keep` and renames the file to its real name (numbered if that name is taken), and flat **"_Discard"**, which deletes the file and its entry.
  - **Completed and the file exists**: a flat **"_Open"**, which opens the file with the default app. It is the row's activatable widget, so activating the row opens the file.
  - **File exists and not unconfirmed**: **"Show in Folder"** (`folder-open-symbolic`), which opens the containing folder.
  - **Final** (completed, failed, cancelled): **"Remove from List"** (`window-close-symbolic`) → core `remove(id)`. The file is kept.

**Live updates**

- `downloads.subscribe(change)`:
  - `Change::List` rebuilds the list.
  - `Change::Progress(id)` (at most every 250 ms per download) updates that row's subtitle and bar.
- A **1-second timer** rewords the live rows, so a stalled download's speed falls to "0 B/s" within about 10 seconds. The self-test expects "0 B/s - 1.0 KB of 1.0 MB" for `/stalled.bin`.
- Bars move only when bytes arrive.
- On destroy, the subscription and the timer are removed.

**Classification**

- (KEEP) The list, the states, Chrome's status wording, Keep/Discard for dangerous files, Open, Show in Folder, Remove, Clear List, and the separate private-window list, through core `vsesvit_core::downloads::{Downloads::{list, clear, remove, keep, discard}, Download, DownloadId, State::{is_live, is_final, Unconfirmed, Completed, …}, Progress, Transfer, status_line, listed_in, unconfirmed_path, is_dangerous}` and the shell's `crate::downloads::Downloads` controller (`progress`, `cancel`, `subscribe`, `directory`).
- (WEBKIT-ONLY) "WebKitGTK cannot pause a download, so nothing offers to", and a failed download cannot be resumed.
- (NEEDS-CEF) The live download comes from `CefDownloadHandler::OnBeforeDownload` (destination, the unconfirmed path, the downloads.ask save dialog through `CefBeforeDownloadCallback::Continue(path, show_dialog)`) and `OnDownloadUpdated` (bytes, speed, state). Cancel is `CefDownloadItemCallback::Cancel`. **Pause and Resume become possible** with `CefDownloadItemCallback::Pause`/`Resume` (all verified in bindings). The Windows Downloads dialog already offers Pause (in progress) and Resume (paused or interrupted), and core's states and `status_line` already cover Paused and Interrupted. Offering them is a parity decision.
- (KEEP / toolkit) The content-type icon (needs a MIME guess and an icon set), and opening files and folders (`gtk::FileLauncher` → the portal or `org.freedesktop.FileManager1`). These need a GTK-free equivalent.

---

## 15. About dialog: `about.rs`

- `adw::AboutDialog`:
  - application name **"Vsesvit"**;
  - icon `dev.mrquantumoff.vsesvit` if installed in the icon theme, else `web-browser-symbolic`;
  - version `CARGO_PKG_VERSION`;
  - comments **"A web browser for Linux and Windows"**;
  - debug info "Vsesvit {v}\nGTK {a.b.c}\nlibadwaita {a.b.c}\nWebKitGTK {a.b.c}\n".
- It opens over the active window (`app.about`).
- (KEEP) Name, icon, version and comment.
- (WEBKIT-ONLY) The GTK, libadwaita and WebKitGTK version lines.
- (NEEDS-CEF) Report CEF and Chromium instead: `CEF_VERSION` = "154.0.34+g14c5a08+chromium-154.0.8037.98" and the `CHROME_VERSION_*` constants (verified in bindings), plus the GPUI version.

---

## 16. Interactions with other modules (summary)

| Dialog | Calls into the shell | Calls into core / sync / update |
|---|---|---|
| Settings | `Browser::{pref, set_pref, reset_pref, switch, set_engine_switch, set_spellcheck_language, set_tracking_protection, set_third_party_cookies, set_theme, set_tabs_position, set_bookmarks_bar_visible, set_home_button_visible, set_compact_address_bar, set_full_urls, set_updates_channel, set_updates_automatic, set_show_profile_picker, watch_prefs, downloads().directory(), engine(), trackers().apply(), cookies().apply(), home(), profiles(), updates(), sync()}`; `crate::engine::dictionaries()`; `crate::permissions::{listed, enforce}`; `crate::profiles::manage`; `Updates::{check, watch_status}` and `app.update`; `Syncer::{watch, act, delete_server_data, types_changed}`; `window.toast` | `prefs::{keys, homepage_input, DEFAULT_SYNC_SERVER, …}`, `sync::DataType`, `spellcheck`, `memory_saver`, `trackers::TrackingProtection`, `cookies::{ThirdPartyCookies, site_choices}`, `https_only::DESCRIPTION`, `permissions::{SitePermissions, Permission, Setting, Origin}`, `history().delete_range`; `vsesvit_sync::{server_input, now_secs, status::{State, Action, DELETE_CONFIRMATION}}` |
| Search engines | `Browser::watch_prefs` | `search::{SearchEngines::{list, default_engine, set_default, check, save, remove}, EngineForm, FormField, FormError}` |
| Site data | `engine().session().website_data_manager()` (WebKit), `crate::cookies::{holds, remove_data}` | — |
| Shortcuts page and capture | `Browser::{keymap, edit_keymap, apply_keymap, extension_shortcuts}`, `crate::keymap::{actions, binding, accelerators, pressed, suspend}` | `shortcuts::*`, `extensions::commands::*` |
| Passphrase | `Syncer::{state, encryption, watch, passphrase, act}`, `crate::sync::encryption` | `vsesvit_sync::status::{passphrase_dialog, check_passphrase, OFFER, Prompt, prompt}`, `prefs keys::SYNC_PASSPHRASE_OFFERED` |
| Welcome | `Browser::install`, `bookmarks::Import`, `settings::sync_server_row`, `Syncer::{act, watch, hold_prompts}` | `onboarding::{finish, RECOMMENDED_EXTENSIONS}`, `search_engines()`, `import::installed_browsers`, `extensions().get` |
| Extensions | `Browser::{install_asking, update_extensions, installed_extensions, extension_error, set_extension_enabled, approve_extension_permissions, uninstall_extension, set_extension_allowed_in_private, runtime().notification_permission_changed, watch_extensions}`, `crate::extensions::{icon_path, progress_to, unsupported_notice}` | `extensions::*` (see §10) |
| Bookmarks | `Browser::{bookmarks_changed, watch_bookmarks}`, `crate::bookmark_drag`, `crate::bookmark_editor::edit`, `crate::favicons`, `window.open_tab` | `bookmarks`, `import`, `export`, `favicons` |
| History | `Browser::{watch_history, sync().is_signed_in()}`, `crate::favicons`, `window.open_tab` | `history`, `session::other_devices`, `vsesvit_sync::status::ago` |
| Downloads | `crate::downloads::{Downloads, Change, open, show_in_folder, file_name}` | `downloads::{status_line, State, Progress}` |

Cross-cutting rules the new shell has to honour:

- Every pref write that has a live effect goes through one shell method that **writes and applies to every window**.
- Sync-applied changes refresh open Settings through `watch_prefs`, the Bookmarks window through `bookmarks_changed`, and History through `watch_history`.
- Extension enable, disable, install and uninstall always re-apply the keymap, because extension commands bind shortcuts.
- Every view holds the browser and profile **weakly**, so an open dialog never keeps the profile open. Tests verify this (`the_page_and_a_closed_capture_let_the_browser_go`, `closing_the_window_drops_its_state`, `a_closed_window_goes`, and in `app.rs` `releasing_the_profile_frees_its_lock`, which opens every dialog and library window and then checks that the profile lock is released for a restart).

---

## 17. Self-test coverage of these dialogs

`vsesvit --self-test OUT_DIR [--network]` (`self_test.rs`; 62 checks, plus `cws_install` with `--network`) drives the real widgets. The checks that touch `dialogs/`, and what they assert:

| Check | Asserts |
|---|---|
| `install_prompt` | `install_asking` shows an AlertDialog whose `shown_lines` are exactly `["Add “Vsesvit Probe”?", "It can:", "• Read and change all your data on all websites", "• Display notifications"]`; closing it gives `InstallFailure::Cancelled` and nothing is committed; `install-prompt.png` |
| `bookmark_export` | `win.show-bookmarks` opens the window; its main menu holds exactly `("_Import Bookmarks…","bookmarks.import")`, `("_Export Bookmarks…","bookmarks.export")`; `export_to` writes HTML that `import::parse_html` reads back with the renamed fixture |
| `memory_saver` (end) | General shows the "Memory Saver" switch and the "Memory Savings" combo with Moderate/Balanced/Maximum, Balanced chosen with its description, insensitive while the switch is off |
| `extension_commands` (end) | Settings > Shortcuts lists the probe's command ("Vsesvit Probe command", subtitle "Vsesvit Probe") showing `<Alt><Shift>k`; reassigning moves the accelerator; "Reset to Default" restores it and hides itself; `extension-shortcuts.png` |
| `extension_notifications` | the notification's Settings activation opens the dialog titled "Extensions" with the probe's expander row expanded and a "Notifications" switch on; switching off fires level "denied", closes the notification and stores it; on again fires "granted"; `extension-notifications.png` |
| `extension_update` | `win.show-extensions`; the `update-extensions` button is insensitive while it runs; after an update asking for `tabs`, the extension is at 3.0, off, not running, `withheld == ["Read your browsing history"]`; its row shows "Enable “Vsesvit update probe”?" with `re_enable_text` and an insensitive switch; "_Re-enable" runs it again; `extension-update.png` |
| `download` | after a completed download, `win.show-downloads` opens `Windowed::Downloads(Normal)`; `downloads.png` |
| `download_safety` | Keep/Discard in the window's download warning (not the Downloads window) |
| `download_speed` | the newest Downloads row reads speed and time left for `/slow.bin`; for `/stalled.bin` it comes to read "0 B/s - 1.0 KB of 1.0 MB" (driven by the 1 s tick) |
| `settings` | engine defaults (pop-ups blocked, smooth scrolling, GPU on); each of the pages `general`, `sync`, `appearance`, `search`, `privacy`, `shortcuts` can be selected by name; Privacy shows "Vsesvit Doesn't Save Passwords"; `settings-{name}.png` |
| `spellcheck` | General shows the "Check Spelling" switch on and "Languages" sensitive; toggling a language changes WebKit's languages; `settings-spell-check.png` |
| `search_engines` | Search > "Manage Search Engines" → subpage with "_Add"; the "DuckDuckGo (Default)" row's menu has no `engine.remove`; the editor refuses shortcut `w` with "Another search engine has this shortcut" (Add disabled), accepts `fx` with a URL, "_Add" saves; "fx vsesvit fixture" searches it; Remove in a reopened Settings deletes it; `search-engine-editor.png`, `settings-search-engines.png` |
| `shortcuts` | the History and Downloads rows show the reassigned accelerators (`<Control>j` and none); capturing Ctrl+T for History says "Also used by New tab. Saving moves it here."; Reset All restores every default; `shortcuts-edited.png`, `shortcut-capture.png` |
| `site_permissions` | Privacy > "Site Permissions" lists Location as "Block"; its "Remove" button goes back to Ask and leaves "Sites you allow or block show here."; `settings-site-permissions*.png` |
| `tracking_protection` | the Privacy combo "Tracking Protection" shows Standard with its description; choosing Strict stores it and shows Strict's description |
| `https_only` | the Privacy switch "Always Use Secure Connections" starts off, and on stores the pref; the "Secure DNS" row has a non-empty subtitle |
| `cookies` | the Privacy radios start at "Block third-party cookies in private windows" with its description; Block stores it (WebKit `NoThirdParty`), back restores it; "See All Site Data" lists "127.0.0.1" with a subtitle starting "Cookies"; `settings-site-data.png` |
| `sync_passphrase` | the offer appears by itself with OFFER's body and buttons, and "Not Now" records it and does not return; Settings > Sync shows "Sync Now" and "Encrypt with a Passphrase…" with a "Not encrypted" subtitle; the dialog has the fields "Passphrase"/"Confirm passphrase", accept disabled when empty, mismatched or short, enabled when valid; accepting gives a Ready account and "Change Passphrase…"; without keys, the Enter prompt appears by itself, cannot be closed with Escape, says "The passphrase is wrong" after a wrong one, accepts the right one; its "Sign Out" signs out; `sync-passphrase*.png` |
| `profiles` | General's "Show Profile Picker at Startup" switch is on, and off writes `profiles.json` `show_picker = false` |
| `welcome` | walks every step, light and dark screenshots `welcome-{step}[-dark].png`. Search: one radio per engine, the default checked, choosing one changes core's default. Import: one "Import" per browser found or "No other browsers found", plus "Choose File…"; Back then Next returns to the page. Extensions: each recommended row shows Install xor Installed (with `--network`, installing Bitwarden works). Default browser: a status text. Sync: the "Sync Server" row shows the pref, "Sign In" visible, applying `https://sync.example.com/` stores `https://sync.example.com`, clearing restores the default with no error style. "Start Browsing" closes it and `onboarding.done` is true |

Cargo unit tests in `dialogs/` (behaviour to keep): `a_toast_shows_a_name_as_written`, `no_toast_reads_its_title_as_markup`, `a_window_of_its_own_opens_once_and_closes_with_its_browser_window`, `every_sync_state_lists_its_buttons_in_their_fixed_order`, `a_download_folder_shows_as_named_not_as_markup`, `picture_in_picture_synced_from_windows_is_not_listed`, `the_offer_waits_for_the_welcome_and_comes_once`, `a_dialog_that_cannot_close_ends_unanswered_when_it_is_moot_or_its_window_closes`, `the_private_windows_switch_lets_the_extension_into_private_tabs`, `an_update_asking_for_new_permissions_waits_for_the_re_enable_button`, `the_progress_row_stays_while_another_install_runs`, `a_switch_that_cannot_change_the_extension_shows_what_the_profile_holds`, `the_install_prompt_lists_chromes_warnings_and_what_linux_lacks`, `the_request_prompt_lists_the_sites_behind_a_number_of_websites`, `the_menu_exports_a_file_the_importer_reads_back`, `dropping_rows_moves_bookmarks_in_core`, `closing_the_window_drops_its_state`, `a_search_hides_the_tree_selection_and_follows_changes`, `move_to_folders_stop_at_the_maximum_depth`, `a_device_says_how_long_ago_it_synced`, `a_closed_window_goes`, `the_editor_marks_what_core_refuses_and_saves_through_it`, `the_list_shows_every_engine_and_follows_changes`, `the_page_and_a_closed_capture_let_the_browser_go`, `an_extension_shortcut_needs_ctrl_or_alt_and_the_browser_names_the_command_it_takes_from`.

---

## 18. Checklist

### Shared

- [ ] Library windows are single-instance per kind; asking again raises the existing one
- [ ] A library window closes with the browser window it was opened from, not with later askers
- [ ] Downloads has separate windows for normal and private browsing
- [ ] Library window layout: header buttons, clamped search entry (max 560 px), toast area, 640×680
- [ ] All toasts and labels show names and errors as plain text, never as markup
- [ ] Text prompt (trimmed, empty means cancel), choice prompt, and destructive confirm with Cancel as the default
- [ ] Visit times shown in the locale's date format plus 24-hour time (`%x %H:%M`)
- [ ] Views hold the browser and profile weakly; closing everything releases the profile lock

### Settings frame

- [ ] Settings opens over the browser window, about 1040 px wide, with six pages in order: General, Sync, Appearance, Search, Privacy, Shortcuts (names `general`…`shortcuts`)
- [ ] Every change applies at once in every window
- [ ] An open Settings follows sync-applied pref and search-engine changes
- [ ] A value that is already stored is never written back (no re-stamping of synced values)

### Settings > General

- [ ] On Startup combo: Restore the previous session / Open the homepage / Open a new tab (`startup`)
- [ ] Homepage entry with apply: empty stores `about:home`, an address is normalized, anything else gives the toast "Not a web address"; follows sync (`homepage`)
- [ ] Profiles group only for profiles in the install's list: "Show Profile Picker at Startup" writes `profiles.json`
- [ ] "Manage Profiles…" opens Manage Profiles
- [ ] Download Folder row: effective path, "Use Default" only while customized, "Change…" folder picker (`downloads.directory`)
- [ ] "Ask Where to Save Each File" switch (`downloads.ask`)
- [ ] "Check Spelling" switch (`spellcheck.enabled`)
- [ ] Languages list with a switch per available language, named by `spellcheck::display_name`; insensitive while checking is off; "None" when empty (`spellcheck.languages`; Chromium dictionaries replace Hunspell scanning)
- [ ] "Smooth Scrolling" switch (`scrolling.smooth`), with a restart note if CEF needs a restart
- [ ] "Hardware Acceleration" switch (`system.hardware_acceleration`, local), with a restart note if CEF needs a restart
- [ ] Profile Folder row showing the profile root (selectable)
- [ ] "Memory Saver" switch (`performance.memory_saver`, local)
- [ ] "Memory Savings" combo Moderate/Balanced/Maximum with descriptions; insensitive while Memory Saver is off (`performance.memory_saver.mode`)
- [ ] Updates group only on self-updating installs
- [ ] Update status row with lifecycle text, "Version x", and Check for Updates or Update/Restart/Restart now
- [ ] "Update Channel" combo, which re-checks at once (`updates.channel`)
- [ ] "Automatic Updates" switch (`updates.automatic`)

### Settings > Sync

- [ ] Account row: core status title and subtitle; buttons in fixed order; the first one suggested; Sync Now disabled while syncing; follows state
- [ ] Encrypt with a Passphrase… and Change Passphrase… run the passphrase flow with a spinner; failure toast
- [ ] Customize Sync: "Sync Everything" plus five type switches with the customizing-mode semantics (`sync.types`, local); a change triggers sync now
- [ ] Sync Server entry: validation error shown in the group description and error style; empty restores the default; editable only while signed out (`sync.server`)
- [ ] "Delete Data on Server…" only while offered: confirmation with core wording; insensitive while running; failure toast keeps the profile signed in

### Settings > Appearance

- [ ] Theme: Follow the system / Light / Dark (`theme`), applied to the chrome and to web content's colour scheme
- [ ] Tab Position: Left sidebar / Right sidebar / Top bar; re-lays out every window (`tabs.position`)
- [ ] Show Bookmarks Bar (`bookmarks_bar.visible`)
- [ ] Show Home Button (`toolbar.home_button`)

### Settings > Search

- [ ] Default "Search Engine" combo (`search.default`)
- [ ] "Manage Search Engines" subpage with "_Add"
- [ ] Engine rows: "Name (Default)", URL with %s, shortcut label, More Actions menu (Make Default / Edit… / Remove; only Edit… for the default)
- [ ] Search engine editor: three fields, live core validation, blank errors hidden, field error styles, Add/Save enabled only when valid; toasts on failure
- [ ] Removing the default engine refused (toast); built-ins removable
- [ ] "Compact Address Bar" switch (`address_bar.compact`)
- [ ] "Always Show Full URLs" switch (`address_bar.full_urls`)
- [ ] Suggestions switches: Search Suggestions / Browsing History / Bookmarks (`address_bar.suggest.*`)

### Settings > Privacy

- [ ] "Tracking Protection" combo Off/Standard/Strict with description subtitle (`privacy.tracking_protection`), applied to all tabs
- [ ] Third-party cookies as three radio rows with descriptions (`privacy.third_party_cookies`), applied to the engine
- [ ] "See All Site Data" subpage: sites with what they hold, Remove per site, "Remove All…" with confirmation, empty state (CEF: cookies plus `Storage.clearDataForOrigin`)
- [ ] "Always Use Secure Connections" switch (`privacy.https_only`)
- [ ] "Secure DNS" informational row (wording revisited for Chromium DoH)
- [ ] "Block Pop-ups" switch (`content.block_popups`), applied to the engine
- [ ] "Vsesvit Doesn't Save Passwords" notice (and Chromium's password manager kept off)
- [ ] "Site Permissions" subpage: per-site groups, a combo per stored permission with the allowed choices, Remove back to Ask; enforcement re-run; "No Site Permissions" empty state; PiP hidden while Linux has no PiP
- [ ] "Clear Browsing Data…": confirmation, synced all-time history deletion, cookies, site storage and cache cleared, window toast "Browsing data cleared"
- [ ] (New, Windows parity) "Save and fill form entries such as addresses" switch for `autofill.forms` now that the engine can autofill

### Settings > Shortcuts

- [ ] A group per core section listing the reassignable commands with live chords or "Disabled"
- [ ] "Reset to Default" per row, visible only when changed
- [ ] "Extension Shortcuts" group for enabled extensions' commands, with the extension name as subtitle
- [ ] "Reset All" with confirmation; sensitive only when the keymap differs from the default
- [ ] Capture dialog: all accelerators suspended; Escape cancels, Backspace clears, Enter saves; live preview; notes for conflicts, needs-modifier and unusable keys; Save enabled only for an offerable chord
- [ ] Extension shortcut needs Ctrl or Alt; browser chords cannot be taken; "Also used by …" notes
- [ ] All edits stored as the one synced `keyboard.shortcuts` register and applied in every window

### Keyboard Shortcuts help

- [ ] Shortcuts help dialog listing every bound command per section with live accelerators

### Sync passphrase

- [ ] Passphrase dialog in core's four variants, with confirmation field rules, error under the fields, accept disabled until valid
- [ ] A dialog whose dismiss is "Sign Out" cannot be closed except by answering
- [ ] A passphrase dialog closes by itself when the state no longer asks or its window closes
- [ ] Wrong passphrase: asks again showing the capitalized error
- [ ] "Encrypt Your Synced Data?" offer: once per profile (`sync.passphrase_offered`); accept sets a passphrase
- [ ] Sync prompts appear only over normal windows, one at a time, never while the welcome is open (unless asked from Settings)

### Welcome

- [ ] Opens on the launch that created the profile, over the first normal window; closing at any step sets `onboarding.done`
- [ ] Reachable again from the main menu's Welcome
- [ ] Non-swipeable pager with Back, dots and Next; per-step Next labels (Get Started / Next / Skip / Start Browsing); Enter activates Next
- [ ] Welcome page with app icon and wording
- [ ] Search step: radio list of engines, choice applied at once
- [ ] Import step: other browsers looked for on a worker thread, Import per browser into "Imported from X"; bookmarks file import into "Imported"; outcomes and errors with Try Again
- [ ] Extensions step: recommended uBlock Origin Lite, Bitwarden and Proton Pass with Install/Installed; plain install without a prompt
- [ ] Default Browser step: status text, Make Default button (http, https, text/html), Flatpak and no-desktop-entry explanations, error text
- [ ] Sync step: Sync Server row, Sign In that moves on, status text once signing in or signed in
- [ ] Done step: "You’re All Set" and wording

### Extensions dialog

- [ ] Install by store link or id (CWS, Edge, AMO); parse-error toast
- [ ] Install from a .crx/.xpi file with a filter
- [ ] Load an unpacked folder
- [ ] Shared progress row with core's phase text, staying while any install runs
- [ ] Chrome's install prompt before committing user installs from a store or package; cancel is silent; result toasts
- [ ] Installed list: icon, name, "version · provenance", "No extensions installed"
- [ ] Enabled switch (insensitive while withheld); failure toasts; the list shows what the profile holds
- [ ] Remove button (no confirmation); failure toast
- [ ] Update button: insensitive while running, summary toast, "already running" error
- [ ] Periodic extension update checks (a minute after start, then every five hours)
- [ ] Re-enable row "Enable “X”?" with "It can now:" warnings; "_Re-enable" grants and runs
- [ ] "Not running" row with the engine's load error for enabled extensions that failed to load
- [ ] Per-extension "Notifications" switch (only with the notifications permission), synced, takes effect at once
- [ ] Per-extension "Allow in private windows" switch, synced, takes effect at once in private windows
- [ ] ID and Location rows (selectable)
- [ ] Open the dialog with a given extension's row expanded (from a notification's Settings button)
- [ ] Drop the "Not supported by the Linux runtime" notices (WebKit only)

### Extension prompts

- [ ] `permissions.request` prompt "“X” has requested additional permissions." / "It could:" / Deny-Allow, over the requesting window

### Bookmarks window

- [ ] Bookmarks window: tree of the three roots expanded at first open, lazily loaded folders with favicons, expanded state kept across rebuilds
- [ ] Rows: icon, title, readable URL (core `readable_url`); separators as "—"
- [ ] Activating a bookmark opens it in a new foreground tab
- [ ] Search: up to 60 results with favicons, "No Matching Bookmarks", follows changes; toolbar ignores the hidden selection
- [ ] New Folder into the selected folder, next to the selected bookmark, or on the bar; default name "New Folder"
- [ ] Edit… through the shared bookmark editor; "Select a bookmark or folder to edit" toast
- [ ] Move To… listing folder paths, excluding the node's own subtree, depth-capped at 64
- [ ] Delete; folders confirm "Delete Folder?"
- [ ] Drag-and-drop reordering and moving into folders, with before/into/after zones and indicator; roots not draggable; cycle errors toasted
- [ ] Main menu Import Bookmarks… (browsers found or an HTML/JSON file) into a new folder on the bar, with result toasts
- [ ] Main menu Export Bookmarks… to Chrome-format HTML, starting in Documents with Chrome's dated file name, with result toasts
- [ ] Every edit refreshes all bookmark bars, stars and the window

### History window

- [ ] "Tabs from Other Devices" section: sign-in hint, empty hint, per device "Synced {ago}", tabs with favicons that open in a new tab
- [ ] Recent visits (300, visit times) or search (100, last visit times); rows "time · URL"; open in a new foreground tab
- [ ] "Forget this page" per row (synced deletion)
- [ ] "_Clear…" with ranges Last hour / 24 hours / 7 days / 4 weeks / All time (synced deletion)
- [ ] "No History" empty state
- [ ] Refreshes when sync changes history or sessions

### Downloads window

- [ ] Newest-first list (500), private window listing every download, normal window never listing private ones
- [ ] Rows: file-type icon, file name, core `status_line`
- [ ] Live rows: progress bar (fraction or pulse) and Cancel
- [ ] Unconfirmed rows: Keep and Discard
- [ ] Completed rows: Open (also row activation); Show in Folder when the file exists; Remove from List when final
- [ ] Live progress updates, plus a 1-second re-wording so stalled speeds fall to 0 B/s
- [ ] Header: Open Download Folder (created if missing) and Clear List (final entries only; files kept)
- [ ] "No Downloads" empty state
- [ ] (New, Windows parity, decision) Pause and Resume, now that CEF can pause and resume downloads

### About

- [ ] About dialog: name, icon (fallback), version, "A web browser for Linux and Windows", debug info with CEF, Chromium and GPUI versions

### Self-test

- [ ] Self-test equivalents for `install_prompt`, `bookmark_export`, `extension_notifications`, `extension_update`, `download*`, `settings`, `spellcheck`, `search_engines`, `shortcuts`, `site_permissions`, `tracking_protection`, `https_only`, `cookies`, `sync_passphrase`, `profiles`, `welcome`
