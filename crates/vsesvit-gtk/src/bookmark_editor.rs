//! Editing a bookmark's name, URL and folder, in two places: the bubble the star opens
//! ("Bookmark added" for a page it just bookmarked, "Edit bookmark" for one already
//! bookmarked) and the dialog that Edit…, Rename… and Add Page… open from the bookmarks bar
//! and the Bookmarks window. Both use one form and save through core's `rename`, `set_url`
//! and `move_to`. A URL that does not parse disables saving and marks the field.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use vsesvit_core::Url;
use vsesvit_core::bookmarks::{BookmarkId, BookmarkNode, Bookmarks, InsertAt, NodeKind};
use vsesvit_core::search::classify_url;

use crate::profile::Core;
use crate::window::BrowserWindow;

/// Pixels each folder level is indented by in the folder list.
const FOLDER_INDENT: i32 = 16;
const BUBBLE_WIDTH: i32 = 340;

/// What the editor edits.
pub(crate) enum Subject {
    Existing(BookmarkNode),
    /// A link not saved yet, added to `parent` on save.
    New { parent: BookmarkId, title: String, url: String },
}

/// What the form holds when it is valid.
#[derive(Debug, PartialEq)]
struct Values {
    title: String,
    /// `None` for a folder.
    url: Option<Url>,
    parent: BookmarkId,
}

struct Form {
    grid: gtk::Grid,
    name: gtk::Entry,
    /// `None` for a folder.
    url: Option<gtk::Entry>,
    folder: gtk::DropDown,
    /// The folder list's entries, in the dropdown's order.
    folders: Vec<BookmarkId>,
}

impl Form {
    fn new(core: &Core, subject: &Subject) -> Self {
        let (title, url, parent, exclude) = match subject {
            Subject::Existing(node) => {
                let url = (node.kind == NodeKind::Url).then(|| node.url.as_ref().map(Url::to_string).unwrap_or_default());
                let exclude = (node.kind == NodeKind::Folder).then_some(node.id);
                (node.title.clone(), url, node.parent, exclude)
            }
            Subject::New { parent, title, url } => (title.clone(), Some(url.clone()), *parent, None),
        };
        let choices = folder_choices(&core.borrow_mut().bookmarks(), exclude, parent);
        let folders: Vec<BookmarkId> = choices.iter().map(|c| c.id).collect();
        let titles: Vec<&str> = choices.iter().map(|c| c.title.as_str()).collect();
        let folder = gtk::DropDown::from_strings(&titles);
        folder.set_list_factory(Some(&indented_factory(choices.iter().map(|c| c.depth).collect())));
        folder.set_selected(folders.iter().position(|&id| id == parent).unwrap_or(0) as u32);
        folder.set_hexpand(true);

        let grid = gtk::Grid::builder().row_spacing(8).column_spacing(12).build();
        let label = |text: &str, row: i32, widget: &gtk::Widget| {
            let label = gtk::Label::builder().label(text).xalign(1.0).use_underline(true).mnemonic_widget(widget).build();
            grid.attach(&label, 0, row, 1, 1);
            grid.attach(widget, 1, row, 1, 1);
        };
        let name = gtk::Entry::builder().text(&title).activates_default(true).hexpand(true).build();
        label("_Name", 0, name.upcast_ref());
        let url = url.map(|url| {
            let entry = gtk::Entry::builder()
                .text(&url)
                .activates_default(true)
                .input_purpose(gtk::InputPurpose::Url)
                .input_hints(gtk::InputHints::NO_SPELLCHECK)
                .build();
            label("_URL", 1, entry.upcast_ref());
            entry
        });
        label("_Folder", 2, folder.upcast_ref());
        let form = Form { grid, name, url, folder, folders };
        form.validate();
        form
    }

    fn read(&self) -> Option<Values> {
        let title = self.name.text().trim().to_owned();
        let url = match &self.url {
            Some(entry) => Some(parse_url(&entry.text())?),
            // A folder needs a name; a link without one shows its host.
            None if title.is_empty() => return None,
            None => None,
        };
        let parent = *self.folders.get(self.folder.selected() as usize)?;
        Some(Values { title, url, parent })
    }

    /// Marks the URL field when it does not parse. Returns whether the form can be saved.
    fn validate(&self) -> bool {
        if let Some(entry) = &self.url {
            if parse_url(&entry.text()).is_some() {
                entry.remove_css_class("error");
            } else {
                entry.add_css_class("error");
            }
        }
        self.read().is_some()
    }

    fn connect_validity(self: &Rc<Self>, f: impl Fn(bool) + 'static) {
        let f = Rc::new(f);
        for entry in std::iter::once(&self.name).chain(self.url.as_ref()) {
            let (form, f) = (Rc::downgrade(self), f.clone());
            entry.connect_changed(move |_| {
                if let Some(form) = form.upgrade() {
                    f(form.validate());
                }
            });
        }
    }

    fn focus_name(&self) {
        self.name.grab_focus();
        self.name.select_region(0, -1);
    }
}

/// The URL field's text as a bookmark URL, if it is one: what the address bar would open as a
/// URL, so a bare host gains `https://`.
fn parse_url(text: &str) -> Option<Url> {
    classify_url(text).map(|target| target.url().clone())
}

/// Writes `values` over `subject`. Returns the bookmark's id.
fn save(core: &Core, subject: &Subject, values: &Values) -> Result<BookmarkId, vsesvit_core::Error> {
    let mut profile = core.borrow_mut();
    let mut bookmarks = profile.bookmarks();
    match subject {
        Subject::Existing(node) => {
            if values.title != node.title {
                bookmarks.rename(node.id, &values.title)?;
            }
            if let Some(url) = &values.url
                && node.url.as_ref() != Some(url)
            {
                bookmarks.set_url(node.id, url)?;
            }
            if values.parent != node.parent {
                bookmarks.move_to(node.id, values.parent, InsertAt::End)?;
            }
            Ok(node.id)
        }
        Subject::New { .. } => {
            let Some(url) = &values.url else { unreachable!("a new bookmark's form has a URL field") };
            bookmarks.add_url(values.parent, InsertAt::End, &values.title, url)
        }
    }
}

/// A folder the form can put the bookmark in, `depth` levels below a root.
struct Choice {
    id: BookmarkId,
    depth: usize,
    title: String,
}

/// Every folder in tree order, leaving out `exclude`'s subtree (a folder cannot move into
/// itself). Mobile bookmarks only when it has anything or is where the bookmark is.
fn folder_choices(bookmarks: &Bookmarks<'_>, exclude: Option<BookmarkId>, current: BookmarkId) -> Vec<Choice> {
    fn walk(bookmarks: &Bookmarks<'_>, node: &BookmarkNode, depth: usize, exclude: Option<BookmarkId>, out: &mut Vec<Choice>) {
        if node.kind != NodeKind::Folder || Some(node.id) == exclude {
            return;
        }
        out.push(Choice { id: node.id, depth, title: node.title.clone() });
        for child in bookmarks.children(node.id) {
            walk(bookmarks, &child, depth + 1, exclude, out);
        }
    }
    let mut out = Vec::new();
    for root in bookmarks.children(BookmarkId::ROOT) {
        let hidden = root.id == BookmarkId::MOBILE && bookmarks.children(root.id).is_empty() && current != root.id;
        if !hidden {
            walk(bookmarks, &root, 0, exclude, &mut out);
        }
    }
    out
}

fn indented_factory(depths: Vec<usize>) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            item.set_child(Some(&gtk::Label::builder().xalign(0.0).build()));
        }
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let (Some(label), Some(text)) = (item.child().and_downcast::<gtk::Label>(), item.item().and_downcast::<gtk::StringObject>()) else {
            return;
        };
        label.set_label(&text.string());
        let depth = depths.get(item.position() as usize).copied().unwrap_or(0);
        label.set_margin_start(i32::try_from(depth).unwrap_or(0) * FOLDER_INDENT);
    });
    factory
}

/// The star's bubble for `node`: Done (or Enter) saves, Remove deletes every bookmark of the
/// page, and closing it any other way keeps the bookmark as it was saved.
pub(crate) fn bubble(window: &BrowserWindow, node: BookmarkNode, added: bool) -> gtk::Popover {
    let core = window.browser().core().clone();
    let subject = Subject::Existing(node);
    let form = Rc::new(Form::new(&core, &subject));

    let heading = gtk::Label::builder()
        .label(if added { "Bookmark added" } else { "Edit bookmark" })
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["heading"])
        .build();
    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text("Close")
        .css_classes(["flat", "circular"])
        .build();
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    top.append(&heading);
    top.append(&close);
    let remove = gtk::Button::with_mnemonic("_Remove");
    let done = gtk::Button::builder().use_underline(true).label("_Done").css_classes(["suggested-action"]).build();
    let buttons = gtk::Box::builder().spacing(8).halign(gtk::Align::End).build();
    buttons.append(&remove);
    buttons.append(&done);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(6)
        .margin_end(6)
        .margin_top(6)
        .margin_bottom(6)
        .width_request(BUBBLE_WIDTH)
        .build();
    content.append(&top);
    content.append(&form.grid);
    content.append(&buttons);

    let popover = gtk::Popover::builder().child(&content).position(gtk::PositionType::Bottom).build();
    popover.add_css_class("bookmark-bubble");
    popover.set_default_widget(Some(&done));
    form.connect_validity(glib::clone!(
        #[weak]
        done,
        move |valid| done.set_sensitive(valid)
    ));
    done.set_sensitive(form.validate());
    close.connect_clicked(glib::clone!(
        #[weak]
        popover,
        move |_| popover.popdown()
    ));
    let window = window.downgrade();
    let Subject::Existing(node) = &subject else { unreachable!("the bubble edits a saved bookmark") };
    remove.connect_clicked({
        let (window, url, popover) = (window.clone(), node.url.clone(), popover.downgrade());
        move |_| {
            let Some(window) = window.upgrade() else { return };
            let removed = url.as_ref().map_or(Ok(()), |url| {
                let mut profile = window.browser().core().borrow_mut();
                let mut bookmarks = profile.bookmarks();
                bookmarks.find_by_url(url).iter().try_for_each(|node| bookmarks.remove(node.id))
            });
            if let Err(e) = removed {
                window.toast(adw::Toast::new(&format!("Cannot remove the bookmark: {e}")));
            }
            window.browser().bookmarks_changed();
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        }
    });
    let name = form.name.clone();
    done.connect_clicked({
        let (form, popover) = (form.clone(), popover.downgrade());
        move |_| {
            let (Some(window), Some(values)) = (window.upgrade(), form.read()) else { return };
            if let Err(e) = save(&core, &subject, &values) {
                window.toast(adw::Toast::new(&format!("Cannot change the bookmark: {e}")));
            }
            window.browser().bookmarks_changed();
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        }
    });
    // After the popover has taken the focus, so the selection is not replaced.
    popover.connect_map(move |_| {
        let name = name.clone();
        glib::idle_add_local_once(move || {
            name.grab_focus();
            name.select_region(0, -1);
        });
    });
    popover
}

/// The Edit…, Rename… and Add Page… dialog. Returns whether it saved anything.
pub(crate) async fn edit(parent: &impl IsA<gtk::Widget>, core: &Core, subject: Subject) -> Result<bool, vsesvit_core::Error> {
    let form = Rc::new(Form::new(core, &subject));
    let heading = match &subject {
        Subject::Existing(node) if node.kind == NodeKind::Folder => "Rename Folder",
        Subject::Existing(_) => "Edit Bookmark",
        Subject::New { .. } => "Add Page",
    };
    let dialog = adw::AlertDialog::new(Some(heading), None);
    form.grid.set_width_request(360);
    dialog.set_extra_child(Some(&form.grid));
    dialog.add_responses(&[("cancel", "_Cancel"), ("save", "_Save")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("save", form.validate());
    form.connect_validity(glib::clone!(
        #[weak]
        dialog,
        move |valid| dialog.set_response_enabled("save", valid)
    ));
    form.focus_name();
    let response = dialog.choose_future(Some(parent)).await;
    match form.read() {
        Some(values) if response == "save" => save(core, &subject, &values).map(|_| true),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::browser;

    #[test]
    fn urls_must_parse() {
        assert!(parse_url("https://example.com/").is_some());
        assert!(parse_url("  https://example.com/a  ").is_some());
        assert!(parse_url("about:blank").is_some());
        // Fixed up as the address bar and the Windows shell's editor do.
        assert_eq!(parse_url("example.com").map(|u| u.to_string()).as_deref(), Some("https://example.com/"));
        assert!(parse_url("not a url").is_none());
        assert!(parse_url("").is_none());
    }

    #[gtk::test]
    fn the_form_saves_name_url_and_folder_through_core() {
        let browser = browser();
        let core = browser.core().clone();
        let (folder, id) = {
            let mut profile = core.borrow_mut();
            let mut bookmarks = profile.bookmarks();
            let folder = bookmarks.add_folder(BookmarkId::OTHER, InsertAt::End, "Editor test").unwrap();
            let url = Url::parse("https://before.example/").unwrap();
            let id = bookmarks.add_url(BookmarkId::TOOLBAR, InsertAt::End, "Before", &url).unwrap();
            (folder, id)
        };
        let node = core.borrow_mut().bookmarks().get(id).unwrap();
        let subject = Subject::Existing(node);
        let form = Form::new(&core, &subject);
        let listed = form.folders.contains(&folder) && form.folders[form.folder.selected() as usize] == BookmarkId::TOOLBAR;

        let url_entry = form.url.clone().unwrap();
        url_entry.set_text("not a url");
        let invalid = (form.validate(), url_entry.has_css_class("error"));
        form.name.set_text("After");
        url_entry.set_text("https://after.example/");
        form.folder.set_selected(form.folders.iter().position(|&f| f == folder).unwrap() as u32);
        let valid = form.validate();
        let values = form.read().unwrap();
        save(&core, &subject, &values).unwrap();
        let saved = core.borrow_mut().bookmarks().get(id).unwrap();
        core.borrow_mut().bookmarks().remove(folder).unwrap();

        assert!(listed, "the folder list has every folder, the bookmark's selected");
        assert_eq!(invalid, (false, true));
        assert!(valid);
        assert_eq!((saved.title.as_str(), saved.url.as_ref().map(Url::as_str), saved.parent), ("After", Some("https://after.example/"), folder));
    }
}
