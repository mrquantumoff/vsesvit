//! Settings' search engines, on the Search page: the default engine, and every engine with its
//! shortcut and URL, which the editor adds and edits and the row's menu makes the default or
//! removes, as Chrome's "Manage search engines" does. Core checks the editor's form
//! (`SearchEngines::check`) and refuses to remove the default.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use vsesvit_core::search::{EngineForm, FormField, SearchEngine, SearchEngineId};

use super::plain_toast;
use crate::browser::Browser;

/// The default's row, the list's group and its rows. Their handlers hold it, so it holds them
/// weakly.
struct Engines {
    browser: Browser,
    default_row: glib::WeakRef<adw::ComboRow>,
    group: glib::WeakRef<adw::PreferencesGroup>,
    rows: RefCell<Vec<glib::WeakRef<adw::ActionRow>>>,
    /// The engines the default's row lists, in its order.
    listed: RefCell<Vec<SearchEngineId>>,
    /// Set while the default's row is refilled, whose selection then is not the user's.
    filling: Cell<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowAction {
    MakeDefault,
    Edit,
    Remove,
}

/// An engine row's menu: each item's action, its name in the row's `engine` group and its label.
const ROW_ACTIONS: [(RowAction, &str, &str); 3] =
    [(RowAction::MakeDefault, "make-default", "Make Default"), (RowAction::Edit, "edit", "Edit…"), (RowAction::Remove, "remove", "Remove")];

/// The default engine's row, and a row that opens the list of every engine.
pub(super) fn group(browser: &Browser) -> adw::PreferencesGroup {
    with_list(browser).0
}

/// [`group`], and the group its subpage lists the engines in.
fn with_list(browser: &Browser) -> (adw::PreferencesGroup, adw::PreferencesGroup) {
    let default_row = adw::ComboRow::builder()
        .title("Search Engine")
        .subtitle("Used for words typed in the address bar")
        .build();
    let manage = adw::ActionRow::builder()
        .title("Manage Search Engines")
        .subtitle("Add search engines and the shortcuts that search with them")
        .activatable(true)
        .build();
    manage.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let group = adw::PreferencesGroup::new();
    group.add(&default_row);
    group.add(&manage);

    let add = gtk::Button::builder()
        .child(&adw::ButtonContent::builder().icon_name("list-add-symbolic").label("_Add").use_underline(true).build())
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let list = adw::PreferencesGroup::builder()
        .description("Type a shortcut and a space in the address bar to search with its engine")
        .header_suffix(&add)
        .build();
    let engines = Rc::new(Engines {
        browser: browser.clone(),
        default_row: default_row.downgrade(),
        group: list.downgrade(),
        rows: RefCell::default(),
        listed: RefCell::default(),
        filling: Cell::new(false),
    });
    engines.show();
    default_row.connect_selected_notify(glib::clone!(
        #[strong]
        engines,
        move |row| {
            let chosen = engines.listed.borrow().get(row.selected() as usize).cloned();
            if let Some(id) = chosen.filter(|_| !engines.filling.get())
                && engines.default_id().as_ref() != Some(&id)
            {
                engines.act(row, |p| p.search_engines().set_default(&id));
            }
        }
    ));
    add.connect_clicked(glib::clone!(
        #[strong]
        engines,
        move |add| engines.edit(add, None)
    ));
    browser.watch_prefs(glib::clone!(
        #[weak]
        engines,
        #[upgrade_or]
        false,
        move |_: &Browser| {
            engines.show();
            true
        }
    ));
    // Pushed again each time the row opens it, so the list keeps its rows while it is closed.
    let page = adw::PreferencesPage::new();
    page.add(&list);
    let shown = list.clone();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page));
    let subpage = adw::NavigationPage::builder().title("Search Engines").tag("search-engines").child(&toolbar).build();
    manage.connect_activated(move |row| {
        if let Some(dialog) = row.ancestor(adw::PreferencesDialog::static_type()).and_downcast::<adw::PreferencesDialog>() {
            dialog.push_subpage(&subpage);
        }
    });
    (group, shown)
}

impl Engines {
    fn default_id(&self) -> Option<SearchEngineId> {
        self.browser.core().borrow_mut().search_engines().default_engine().ok().map(|e| e.id)
    }

    /// Fills the default's row and the list from core.
    fn show(self: &Rc<Self>) {
        let (engines, default) = {
            let mut profile = self.browser.core().borrow_mut();
            let mut engines = profile.search_engines();
            (engines.list().unwrap_or_default(), engines.default_engine().ok().map(|e| e.id))
        };
        if let Some(row) = self.default_row.upgrade() {
            let names: Vec<&str> = engines.iter().map(|e| e.name.as_str()).collect();
            let selected = engines.iter().position(|e| Some(&e.id) == default.as_ref()).unwrap_or(0);
            self.filling.set(true);
            row.set_model(Some(&gtk::StringList::new(&names)));
            row.set_selected(u32::try_from(selected).unwrap_or(0));
            self.filling.set(false);
        }
        *self.listed.borrow_mut() = engines.iter().map(|e| e.id.clone()).collect();
        let Some(group) = self.group.upgrade() else { return };
        for row in self.rows.take() {
            if let Some(row) = row.upgrade() {
                group.remove(&row);
            }
        }
        let rows = engines.iter().map(|engine| {
            let row = self.row(engine, default.as_ref() == Some(&engine.id));
            group.add(&row);
            row.downgrade()
        });
        *self.rows.borrow_mut() = rows.collect();
    }

    /// An engine's name, shortcut and URL as Chrome lists them, and its menu: Make Default,
    /// Edit… and Remove, of which the default has only Edit….
    fn row(self: &Rc<Self>, engine: &SearchEngine, is_default: bool) -> adw::ActionRow {
        let title = if is_default { format!("{} (Default)", engine.name) } else { engine.name.clone() };
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(EngineForm::of(engine).url)
            .subtitle_lines(1)
            .use_markup(false)
            .build();
        let shortcut = gtk::Label::builder()
            .label(engine.keyword.as_deref().unwrap_or_default())
            .valign(gtk::Align::Center)
            .css_classes(["dimmed"])
            .build();
        row.add_suffix(&shortcut);

        let menu = gio::Menu::new();
        let actions = gio::SimpleActionGroup::new();
        for (kind, name, label) in ROW_ACTIONS {
            if is_default && kind != RowAction::Edit {
                continue;
            }
            let action = gio::SimpleAction::new(name, None);
            let engine = engine.clone();
            action.connect_activate(glib::clone!(
                #[weak(rename_to = engines)]
                self,
                #[weak]
                row,
                move |_, _| match kind {
                    RowAction::MakeDefault => engines.act(&row, |p| p.search_engines().set_default(&engine.id)),
                    RowAction::Edit => engines.edit(&row, Some(&engine)),
                    RowAction::Remove => engines.act(&row, |p| p.search_engines().remove(&engine.id)),
                }
            ));
            actions.add_action(&action);
            menu.append(Some(label), Some(&format!("engine.{name}")));
        }
        row.insert_action_group("engine", Some(&actions));
        let more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("More Actions")
            .menu_model(&menu)
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        row.add_suffix(&more);
        row
    }

    /// Runs `change` on the profile, then shows the engines again once the widget that asked
    /// for it has finished emitting. A failure shows as a toast.
    fn act(self: &Rc<Self>, widget: &impl IsA<gtk::Widget>, change: impl FnOnce(&mut vsesvit_core::Profile) -> Result<(), vsesvit_core::Error>) {
        let changed = change(&mut self.browser.core().borrow_mut());
        if let Err(e) = changed {
            toast(widget, &format!("Search engines: {e}"));
        }
        self.show_soon();
    }

    fn show_soon(self: &Rc<Self>) {
        glib::idle_add_local_once(glib::clone!(
            #[weak(rename_to = engines)]
            self,
            move || engines.show()
        ));
    }

    /// Opens the editor over `widget`, for `engine` or for a new one.
    fn edit(self: &Rc<Self>, widget: &impl IsA<gtk::Widget>, engine: Option<&SearchEngine>) {
        let (widget, engine, engines) = (widget.clone().upcast::<gtk::Widget>(), engine.cloned(), self.clone());
        glib::spawn_future_local(async move {
            match edit(&widget, &engines.browser, engine.as_ref()).await {
                Ok(true) => engines.show(),
                Ok(false) => {}
                Err(e) => toast(&widget, &format!("Cannot save the search engine: {e}")),
            }
        });
    }
}

fn toast(widget: &impl IsA<gtk::Widget>, text: &str) {
    if let Some(dialog) = widget.ancestor(adw::PreferencesDialog::static_type()).and_downcast::<adw::PreferencesDialog>() {
        dialog.add_toast(plain_toast(text));
    }
}

/// The editor's boxes, Chrome's three, checked against core as they change.
struct Editor {
    content: gtk::Box,
    name: adw::EntryRow,
    keyword: adw::EntryRow,
    url: adw::EntryRow,
    problems: gtk::Label,
}

impl Editor {
    fn new(form: &EngineForm) -> Self {
        let entry = |title: &str, text: &str| adw::EntryRow::builder().title(title).text(text).activates_default(true).build();
        let name = entry("Name", &form.name);
        let keyword = entry("Shortcut", &form.keyword);
        let url = entry("URL with %s in Place of Query", &form.url);
        for row in [&keyword, &url] {
            row.set_input_hints(gtk::InputHints::NO_SPELLCHECK);
        }
        url.set_input_purpose(gtk::InputPurpose::Url);
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).css_classes(["boxed-list"]).build();
        for row in [&name, &keyword, &url] {
            list.append(row);
        }
        let problems = gtk::Label::builder().xalign(0.0).wrap(true).visible(false).css_classes(["error"]).build();
        let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).width_request(360).build();
        content.append(&list);
        content.append(&problems);
        Editor { content, name, keyword, url, problems }
    }

    fn form(&self) -> EngineForm {
        EngineForm { name: self.name.text().into(), keyword: self.keyword.text().into(), url: self.url.text().into() }
    }

    /// Marks the boxes core finds a problem with and says what it is, except for a box that is
    /// only empty. Returns whether the form can be saved.
    fn validate(&self, browser: &Browser, editing: Option<&SearchEngineId>) -> bool {
        let checked = browser.core().borrow_mut().search_engines().check(editing, &self.form());
        let (problems, text) = match checked {
            Ok(problems) => {
                let shown: Vec<String> = problems.iter().filter(|p| !p.is_blank()).map(ToString::to_string).collect();
                (problems, shown.join("\n"))
            }
            Err(e) => (Vec::new(), e.to_string()),
        };
        for (field, row) in [(FormField::Name, &self.name), (FormField::Keyword, &self.keyword), (FormField::Url, &self.url)] {
            if problems.iter().any(|p| p.field() == field && !p.is_blank()) {
                row.add_css_class("error");
            } else {
                row.remove_css_class("error");
            }
        }
        self.problems.set_label(&text);
        self.problems.set_visible(!text.is_empty());
        problems.is_empty() && text.is_empty()
    }
}

/// The Add and Edit dialog. Returns whether it saved the engine.
async fn edit(parent: &gtk::Widget, browser: &Browser, engine: Option<&SearchEngine>) -> Result<bool, vsesvit_core::Error> {
    let editing = engine.map(|e| e.id.clone());
    let editor = Rc::new(Editor::new(&engine.map(EngineForm::of).unwrap_or_default()));
    let heading = if engine.is_some() { "Edit Search Engine" } else { "Add Search Engine" };
    let dialog = adw::AlertDialog::new(Some(heading), None);
    dialog.set_extra_child(Some(&editor.content));
    dialog.add_responses(&[("cancel", "_Cancel"), ("save", if engine.is_some() { "_Save" } else { "_Add" })]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("save", editor.validate(browser, editing.as_ref()));
    let browser = browser.clone();
    for row in [&editor.name, &editor.keyword, &editor.url] {
        row.connect_changed(glib::clone!(
            #[weak]
            dialog,
            #[weak]
            editor,
            #[strong]
            browser,
            #[strong]
            editing,
            move |_| dialog.set_response_enabled("save", editor.validate(&browser, editing.as_ref()))
        ));
    }
    editor.name.grab_focus();
    if dialog.choose_future(Some(parent)).await != "save" {
        return Ok(false);
    }
    browser.core().borrow_mut().search_engines().save(editing.as_ref(), &editor.form())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::browser;

    fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut out = vec![widget.clone()];
        for child in std::iter::successors(widget.first_child(), |c| c.next_sibling()) {
            out.extend(descendants(&child));
        }
        out
    }

    fn titles(group: &adw::PreferencesGroup) -> Vec<String> {
        let rows = descendants(group.upcast_ref());
        rows.iter().filter_map(|w| w.downcast_ref::<adw::ActionRow>()).map(|r| r.title().to_string()).collect()
    }

    #[gtk::test]
    fn the_editor_marks_what_core_refuses_and_saves_through_it() {
        let browser = browser();
        let editor = Editor::new(&EngineForm::default());
        let blank = (editor.validate(&browser, None), editor.problems.is_visible(), editor.name.has_css_class("error"));
        editor.name.set_text("Crates");
        editor.keyword.set_text("w");
        editor.url.set_text("https://crates.io/");
        let refused = editor.validate(&browser, None);
        let marked = (editor.keyword.has_css_class("error"), editor.url.has_css_class("error"), editor.name.has_css_class("error"));
        let said = editor.problems.label().to_string();
        editor.keyword.set_text("cr");
        editor.url.set_text("crates.io/search?q=%s");
        let accepted = (editor.validate(&browser, None), editor.problems.is_visible());
        let id = browser.core().borrow_mut().search_engines().save(None, &editor.form()).unwrap();
        let resolved = browser.core().borrow_mut().omnibox().resolve("cr serde").unwrap().map(|t| t.url().to_string());
        browser.core().borrow_mut().search_engines().remove(&id).unwrap();

        assert_eq!(blank, (false, false, false), "empty boxes keep Add off without calling them out");
        assert!(!refused);
        assert_eq!(marked, (true, true, false));
        assert_eq!(said, "Another search engine has this shortcut\nPut %s where the search terms go");
        assert_eq!(accepted, (true, false));
        assert_eq!(resolved.as_deref(), Some("https://crates.io/search?q=serde"));
    }

    #[gtk::test]
    fn the_list_shows_every_engine_and_follows_changes() {
        let browser = browser();
        let (group, list) = with_list(&browser);
        let before = titles(&list);
        let form = EngineForm { name: "Docs".into(), keyword: "rs".into(), url: "https://docs.rs/?q=%s".into() };
        let id = browser.core().borrow_mut().search_engines().save(None, &form).unwrap();
        browser.core().borrow_mut().search_engines().set_default(&id).unwrap();
        browser.sync_applied(&vsesvit_core::sync::Changed { search_engines: true, ..Default::default() });
        let after = titles(&list);
        let combo = descendants(group.upcast_ref()).into_iter().find_map(|w| w.downcast::<adw::ComboRow>().ok());
        let selected = combo.and_then(|c| c.selected_item()).and_downcast::<gtk::StringObject>().map(|s| s.string().to_string());
        let mut profile = browser.core().borrow_mut();
        profile.search_engines().set_default(&SearchEngineId::builtin_default()).unwrap();
        profile.search_engines().remove(&id).unwrap();
        drop(profile);

        assert!(before.iter().any(|t| t == "DuckDuckGo (Default)"), "{before:?}");
        assert!(after.iter().any(|t| t == "Docs (Default)") && after.iter().any(|t| t == "DuckDuckGo"), "{after:?}");
        assert_eq!(selected.as_deref(), Some("Docs"));
    }
}
