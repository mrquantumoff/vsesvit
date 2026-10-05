//! `chrome.contextMenus`: one extension's items and what of them a context menu shows.
//! Pure: the runtime feeds it the shim's calls and the shell's right-clicks, and renders
//! the [`Entry`] trees it returns.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use vsesvit_core::extensions::manifest::MatchPattern;

/// Chrome's `contextMenus.ACTION_MENU_TOP_LEVEL_LIMIT`.
pub const ACTION_MENU_TOP_LEVEL_LIMIT: usize = 6;

/// Chrome's `kMaxSelectionTextLength`: how much of the selection replaces `%s`.
const MAX_SELECTION: usize = 50;

/// Chrome's `kMaxExtensionItemTitleLength`.
const MAX_TITLE: usize = 75;

const ONCLICK_ERROR: &str = "Extensions using event pages or Service Workers cannot pass an onclick parameter to chrome.contextMenus.create. Instead, use the chrome.contextMenus.onClicked event.";

/// A menu item's id: the extension's string, or the integer the shim generated.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ItemId {
    Int(i64),
    Str(String),
}

/// What the user right-clicked: as much of Chrome's `ContextMenuParams` as the shell knows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    /// The tab's top document.
    pub page_url: String,
    /// The frame's document, when the click was in a subframe.
    pub frame_url: Option<String>,
    pub link_url: Option<String>,
    /// The image, video or audio element's source.
    pub src_url: Option<String>,
    pub media: Option<Media>,
    /// Empty without a selection.
    pub selection: String,
    pub editable: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Media {
    Image,
    Video,
    Audio,
}

/// One row of a menu to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A command; `checked` is `Some` for a checkbox or radio item.
    Item { id: ItemId, title: String, enabled: bool, checked: Option<bool> },
    /// An item with children shown, or the extension's own submenu.
    Submenu { title: String, enabled: bool, children: Vec<Entry> },
    Separator,
}

/// One extension's items, in creation order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Menus {
    items: Vec<Item>,
}

/// An item's siblings are the items with the same `parent`, in their order in
/// [`Menus::items`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Item {
    id: ItemId,
    parent: Option<ItemId>,
    kind: Kind,
    title: String,
    checked: bool,
    contexts: BTreeSet<Context>,
    visible: bool,
    enabled: bool,
    /// Empty matches any document.
    document_patterns: Vec<MatchPattern>,
    /// Empty matches any link or media source.
    target_patterns: Vec<MatchPattern>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Normal,
    Checkbox,
    Radio,
    Separator,
}

/// Chrome's contexts, and Firefox's `menus` ones (`tab` to `password`), which are
/// accepted so Firefox extensions load, and match nothing here.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Context {
    All,
    Page,
    Frame,
    Selection,
    Link,
    Editable,
    Image,
    Video,
    Audio,
    BrowserAction,
    PageAction,
    Action,
    Tab,
    ToolsMenu,
    Bookmark,
    Password,
}

impl Kind {
    fn checkable(self) -> bool {
        matches!(self, Kind::Checkbox | Kind::Radio)
    }
}

impl Item {
    /// Chrome's `MenuItemMatchesParams`. `page` is the least specific context: it applies
    /// only when no link, selection, editable field or media was clicked, though a click in
    /// a subframe still counts.
    fn matches(&self, t: &Target) -> bool {
        let has = |c: Context| self.contexts.contains(&c);
        let (link, selection, subframe) = (t.link_url.is_some(), !t.selection.is_empty(), t.frame_url.is_some());
        let media = t.media.is_some_and(|m| {
            has(match m {
                Media::Image => Context::Image,
                Media::Video => Context::Video,
                Media::Audio => Context::Audio,
            })
        });
        let context = has(Context::All)
            || (selection && has(Context::Selection))
            || (t.editable && has(Context::Editable))
            || (subframe && has(Context::Frame))
            || (link && has(Context::Link) && any_match(&self.target_patterns, t.link_url.as_deref()))
            || (media && any_match(&self.target_patterns, t.src_url.as_deref()))
            || (!link && !selection && !t.editable && t.media.is_none() && has(Context::Page));
        context && any_match(&self.document_patterns, Some(t.frame_url.as_deref().unwrap_or(&t.page_url)))
    }

    fn on_action(&self) -> bool {
        self.contexts.iter().any(|c| matches!(c, Context::All | Context::Action | Context::BrowserAction | Context::PageAction))
    }
}

impl Menus {
    /// `contextMenus.create`. `props` always carries an `id`, which the shim made up when
    /// `generated`. A `lazy` background (a service worker or event page) must name its
    /// items and take clicks from `onClicked`, so it may not pass an `onclick` function.
    pub fn create(&mut self, props: &Value, generated: bool, lazy: bool, onclick: bool) -> Result<(), String> {
        if lazy && generated {
            return Err("Extensions using event pages or Service Workers must pass an id parameter to chrome.contextMenus.create".into());
        }
        if lazy && onclick {
            return Err(ONCLICK_ERROR.into());
        }
        let id = props.get("id").and_then(ItemId::from_json).ok_or_else(|| invalid("id"))?;
        if self.position(&id).is_some() {
            return Err(format!("Cannot create item with duplicate id {id}"));
        }
        let fresh = Item {
            id,
            parent: None,
            kind: Kind::Normal,
            title: String::new(),
            checked: false,
            contexts: BTreeSet::from([Context::Page]),
            visible: true,
            enabled: true,
            document_patterns: Vec::new(),
            target_patterns: Vec::new(),
        };
        let item = self.edit(fresh, props, "createProperties")?;
        self.items.push(item);
        self.sanitize();
        Ok(())
    }

    /// `contextMenus.update`. A `parentId` (`null` for the top level) makes the item its
    /// new parent's last child; `checked: true` on a radio item selects it in its run.
    pub fn update(&mut self, id: &ItemId, props: &Value, lazy: bool, onclick: bool) -> Result<(), String> {
        let mut at = self.position(id).ok_or_else(|| not_found(id))?;
        if lazy && onclick {
            return Err(ONCLICK_ERROR.into());
        }
        let item = self.edit(self.items[at].clone(), props, "updateProperties")?;
        let select = item.kind == Kind::Radio && prop(props, "checked") == Some(&Value::Bool(true));
        if props.get("parentId").is_some() {
            self.items.remove(at);
            self.items.push(item);
            at = self.items.len() - 1;
        } else {
            self.items[at] = item;
        }
        if select {
            self.select(at);
        }
        self.sanitize();
        Ok(())
    }

    /// `contextMenus.remove`: the item and everything under it.
    pub fn remove(&mut self, id: &ItemId) -> Result<(), String> {
        self.position(id).ok_or_else(|| not_found(id))?;
        let mut gone = vec![id.clone()];
        let mut i = 0;
        while let Some(parent) = gone.get(i).cloned() {
            gone.extend(self.children(Some(&parent)).map(|c| c.id.clone()));
            i += 1;
        }
        self.items.retain(|it| !gone.contains(&it.id));
        self.sanitize();
        Ok(())
    }

    pub fn remove_all(&mut self) {
        self.items.clear();
    }

    /// What this extension adds to the page's context menu for `target`: nothing, its one
    /// matching item (a lone parent as its own submenu), or its several items in a submenu
    /// titled `name`, as in Chrome.
    pub fn page_entry(&self, name: &str, target: &Target) -> Option<Entry> {
        let shown = |i: &Item| i.visible && i.matches(target);
        let mut entries = self.entries(None, &shown, &target.selection);
        match entries.len() {
            0 => None,
            1 => entries.pop(),
            _ => Some(Entry::Submenu { title: name.to_owned(), enabled: true, children: entries }),
        }
    }

    /// The toolbar action's menu items: at most [`ACTION_MENU_TOP_LEVEL_LIMIT`] at the top,
    /// not wrapped in a submenu.
    pub fn action_entries(&self) -> Vec<Entry> {
        let shown = |i: &Item| i.visible && i.on_action();
        clean(self.children(None).filter(|i| shown(i)).take(ACTION_MENU_TOP_LEVEL_LIMIT).map(|i| self.render(i, &shown, "")).collect())
    }

    /// The user chose item `id` from the page's menu (`target`) or the action's (`None`):
    /// a checkbox flips, a radio item becomes its run's checked one, and the result is the
    /// `contextMenus.OnClickData` to dispatch. `None` for an unknown item.
    pub fn click(&mut self, id: &ItemId, target: Option<&Target>) -> Option<Value> {
        let at = self.position(id)?;
        let was_checked = self.items[at].checked;
        match self.items[at].kind {
            Kind::Checkbox => self.items[at].checked = !was_checked,
            Kind::Radio => self.select(at),
            Kind::Normal | Kind::Separator => {}
        }
        let item = &self.items[at];
        let mut data = Map::new();
        data.insert("menuItemId".into(), item.id.to_json());
        if let Some(parent) = &item.parent {
            data.insert("parentMenuItemId".into(), parent.to_json());
        }
        data.insert("editable".into(), target.is_some_and(|t| t.editable).into());
        if item.kind.checkable() {
            data.insert("wasChecked".into(), was_checked.into());
            data.insert("checked".into(), item.checked.into());
        }
        if let Some(t) = target {
            data.insert("pageUrl".into(), t.page_url.clone().into());
            match &t.frame_url {
                Some(frame) => data.insert("frameUrl".into(), frame.clone().into()),
                // A subframe has no id this runtime can name, so only the top frame gets one.
                None => data.insert("frameId".into(), 0.into()),
            };
            if let Some(link) = &t.link_url {
                data.insert("linkUrl".into(), link.clone().into());
            }
            if let Some(src) = &t.src_url {
                data.insert("srcUrl".into(), src.clone().into());
            }
            if let Some(media) = t.media {
                let kind = match media {
                    Media::Image => "image",
                    Media::Video => "video",
                    Media::Audio => "audio",
                };
                data.insert("mediaType".into(), kind.into());
            }
            if !t.selection.is_empty() {
                data.insert("selectionText".into(), t.selection.clone().into());
            }
        }
        Some(Value::Object(data))
    }

    fn position(&self, id: &ItemId) -> Option<usize> {
        self.items.iter().position(|i| i.id == *id)
    }

    fn children<'a>(&'a self, parent: Option<&'a ItemId>) -> impl Iterator<Item = &'a Item> {
        self.items.iter().filter(move |i| i.parent.as_ref() == parent)
    }

    /// `item` with `props` applied, refused the way Chrome refuses bad create and update
    /// properties. `param` names the argument in schema errors.
    fn edit(&self, mut item: Item, props: &Value, param: &str) -> Result<Item, String> {
        if let Some(kind) = prop(props, "type") {
            item.kind = serde_json::from_value(kind.clone())
                .map_err(|_| format!("Error at parameter '{param}': Error at property 'type': Value must be one of checkbox, normal, radio, separator."))?;
        }
        if let Some(contexts) = contexts(props, param)? {
            item.contexts = contexts;
        }
        if let Some(visible) = boolean(props, "visible")? {
            item.visible = visible;
        }
        if let Some(enabled) = boolean(props, "enabled")? {
            item.enabled = enabled;
        }
        if let Some(title) = prop(props, "title") {
            item.title = title.as_str().ok_or_else(|| invalid("title"))?.to_owned();
        }
        if item.kind != Kind::Separator && item.title.is_empty() {
            return Err("All menu items except for separators must have a title".into());
        }
        match boolean(props, "checked")? {
            Some(true) if !item.kind.checkable() => return Err("Only items with type \"radio\" or \"checkbox\" can be checked".into()),
            // As in Chrome, unchecking a radio item does nothing: its run keeps one checked.
            Some(checked) if item.kind == Kind::Checkbox || checked => item.checked = checked,
            _ => {}
        }
        item.checked &= item.kind.checkable();
        if let Some(parent) = props.get("parentId") {
            item.parent = self.parent_for(&item.id, parent)?;
        }
        if let Some(patterns) = patterns(props, "documentUrlPatterns")? {
            item.document_patterns = patterns;
        }
        if let Some(patterns) = patterns(props, "targetUrlPatterns")? {
            item.target_patterns = patterns;
        }
        Ok(item)
    }

    /// The parent `raw` names for item `id`; `null` is the top level.
    fn parent_for(&self, id: &ItemId, raw: &Value) -> Result<Option<ItemId>, String> {
        if raw.is_null() {
            return Ok(None);
        }
        let parent = ItemId::from_json(raw).ok_or_else(|| invalid("parentId"))?;
        let at = self.position(&parent).ok_or_else(|| not_found(&parent))?;
        if self.items[at].kind != Kind::Normal {
            return Err("Parent items must have type \"normal\"".into());
        }
        if std::iter::successors(Some(&parent), |p| self.position(p).and_then(|at| self.items[at].parent.as_ref())).any(|p| p == id) {
            return Err("Cannot set a menu item's parent to itself or one of its descendants".into());
        }
        Ok(Some(parent))
    }

    /// Every radio group, as indices into `items`: a maximal run of consecutive radio
    /// siblings, visible or not.
    fn runs(&self) -> Vec<Vec<usize>> {
        let parents = std::iter::once(None).chain(self.items.iter().map(|i| Some(&i.id)));
        let mut runs = Vec::new();
        for parent in parents {
            let siblings: Vec<usize> = (0..self.items.len()).filter(|&at| self.items[at].parent.as_ref() == parent).collect();
            runs.extend(siblings.split(|&at| self.items[at].kind != Kind::Radio).filter(|r| !r.is_empty()).map(<[usize]>::to_vec));
        }
        runs
    }

    /// Chrome's `SanitizeRadioListsInMenu`: each run keeps its last checked item, or checks
    /// its first when none is.
    fn sanitize(&mut self) {
        for run in self.runs() {
            let keep = run.iter().rev().copied().find(|&at| self.items[at].checked).unwrap_or(run[0]);
            for at in run {
                self.items[at].checked = at == keep;
            }
        }
    }

    /// Checks the radio item at `at` and unchecks the rest of its run.
    fn select(&mut self, at: usize) {
        if let Some(run) = self.runs().into_iter().find(|r| r.contains(&at)) {
            for i in run {
                self.items[i].checked = i == at;
            }
        }
    }

    fn entries(&self, parent: Option<&ItemId>, shown: &dyn Fn(&Item) -> bool, selection: &str) -> Vec<Entry> {
        clean(self.children(parent).filter(|i| shown(i)).map(|i| self.render(i, shown, selection)).collect())
    }

    fn render(&self, item: &Item, shown: &dyn Fn(&Item) -> bool, selection: &str) -> Entry {
        if item.kind == Kind::Separator {
            return Entry::Separator;
        }
        let title = title(&item.title, selection);
        let children = self.entries(Some(&item.id), shown, selection);
        if children.is_empty() {
            Entry::Item { id: item.id.clone(), title, enabled: item.enabled, checked: item.kind.checkable().then_some(item.checked) }
        } else {
            Entry::Submenu { title, enabled: item.enabled, children }
        }
    }
}

impl ItemId {
    /// A `menuItemId`, `id` or `parentId` from JavaScript: a string or an integer.
    pub fn from_json(v: &Value) -> Option<ItemId> {
        match v {
            Value::String(s) => Some(ItemId::Str(s.clone())),
            n => n.as_i64().map(ItemId::Int),
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            ItemId::Int(n) => (*n).into(),
            ItemId::Str(s) => s.clone().into(),
        }
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemId::Int(n) => write!(f, "{n}"),
            ItemId::Str(s) => f.write_str(s),
        }
    }
}

fn not_found(id: &ItemId) -> String {
    format!("Cannot find menu item with id {id}")
}

fn invalid(name: &str) -> String {
    format!("Error at property '{name}': Invalid type")
}

/// Property `name` of `props`; `null` counts as absent, as for any optional argument.
fn prop<'a>(props: &'a Value, name: &str) -> Option<&'a Value> {
    props.get(name).filter(|v| !v.is_null())
}

fn boolean(props: &Value, name: &str) -> Result<Option<bool>, String> {
    prop(props, name).map(|v| v.as_bool().ok_or_else(|| invalid(name))).transpose()
}

fn contexts(props: &Value, param: &str) -> Result<Option<BTreeSet<Context>>, String> {
    let Some(raw) = prop(props, "contexts") else { return Ok(None) };
    let bad = || {
        format!(
            "Error at parameter '{param}': Error at property 'contexts': Value must be one of action, all, audio, browser_action, editable, frame, image, launcher, link, page, page_action, selection, video."
        )
    };
    let list = raw.as_array().filter(|l| !l.is_empty()).ok_or_else(bad)?;
    list.iter()
        .map(|c| match c.as_str() {
            Some("launcher") => Err("Only packaged apps are allowed to use 'launcher' context".to_owned()),
            _ => serde_json::from_value(c.clone()).map_err(|_| bad()),
        })
        .collect::<Result<_, _>>()
        .map(Some)
}

fn patterns(props: &Value, name: &str) -> Result<Option<Vec<MatchPattern>>, String> {
    let Some(raw) = prop(props, name) else { return Ok(None) };
    let list = raw.as_array().ok_or_else(|| invalid(name))?;
    list.iter()
        .map(|p| {
            let p = p.as_str().ok_or_else(|| invalid(name))?;
            MatchPattern::parse(p).map_err(|_| format!("Invalid url pattern '{p}'"))
        })
        .collect::<Result<_, _>>()
        .map(Some)
}

/// Chrome's `ExtensionPatternMatch`: no patterns match anything, and a missing or
/// unparsable URL matches no pattern.
fn any_match(patterns: &[MatchPattern], url: Option<&str>) -> bool {
    patterns.is_empty() || url.and_then(|u| url::Url::parse(u).ok()).is_some_and(|u| patterns.iter().any(|p| p.matches(&u)))
}

/// Chrome's `TitleWithReplacement`: `%s` is the selection. The selection and then the
/// whole title are cut to their limits, the ellipsis counted in, as `gfx::TruncateString`
/// does.
fn title(template: &str, selection: &str) -> String {
    truncate(&template.replace("%s", &truncate(selection, MAX_SELECTION)), MAX_TITLE)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_owned() } else { s.chars().take(max - 1).chain(['…']).collect() }
}

/// No leading, trailing or doubled separators.
fn clean(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::with_capacity(entries.len());
    for e in entries {
        if e == Entry::Separator && matches!(out.last(), None | Some(Entry::Separator)) {
            continue;
        }
        out.push(e);
    }
    if out.last() == Some(&Entry::Separator) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn id(s: &str) -> ItemId {
        ItemId::Str(s.into())
    }

    fn built(items: &[Value]) -> Menus {
        let mut menus = Menus::default();
        for props in items {
            menus.create(props, false, false, false).unwrap();
        }
        menus
    }

    fn item<'a>(menus: &'a Menus, name: &str) -> &'a Item {
        &menus.items[menus.position(&id(name)).unwrap()]
    }

    fn order(menus: &Menus) -> Vec<String> {
        menus.items.iter().map(|i| i.id.to_string()).collect()
    }

    fn checked(menus: &Menus) -> Vec<String> {
        menus.items.iter().filter(|i| i.checked).map(|i| i.id.to_string()).collect()
    }

    fn page(url: &str) -> Target {
        Target { page_url: url.into(), ..Target::default() }
    }

    fn command(name: &str, title: &str) -> Entry {
        Entry::Item { id: id(name), title: title.into(), enabled: true, checked: None }
    }

    /// The items that match `target`, whatever their visibility.
    fn matching(menus: &Menus, target: &Target) -> Vec<String> {
        menus.items.iter().filter(|i| i.matches(target)).map(|i| i.id.to_string()).collect()
    }

    /// One item per context, named after it.
    fn one_per_context() -> Menus {
        let contexts = ["all", "page", "frame", "selection", "link", "editable", "image", "video", "audio", "action"];
        built(&contexts.map(|c| json!({ "id": c, "title": c, "contexts": [c] })))
    }

    #[test]
    fn item_ids_are_json_strings_or_integers() {
        assert_eq!(ItemId::from_json(&json!("a")), Some(id("a")));
        assert_eq!(ItemId::from_json(&json!(7)), Some(ItemId::Int(7)));
        assert_eq!(ItemId::from_json(&json!(1.5)), None);
        assert_eq!(ItemId::from_json(&json!(null)), None);
        assert_eq!(ItemId::from_json(&json!(["a"])), None);
        assert_eq!(id("a").to_json(), json!("a"));
        assert_eq!(ItemId::Int(-3).to_json(), json!(-3));
        assert_eq!(format!("{} {}", id("x y"), ItemId::Int(42)), "x y 42");
    }

    #[test]
    fn create_fills_in_chromes_defaults() {
        let menus = built(&[json!({ "id": "a", "title": "A" }), json!({ "id": "s", "type": "separator" })]);
        let a = item(&menus, "a");
        assert_eq!((a.kind, a.parent.as_ref(), a.checked, a.visible, a.enabled), (Kind::Normal, None, false, true, true));
        assert_eq!(a.contexts, BTreeSet::from([Context::Page]));
        assert!(a.document_patterns.is_empty() && a.target_patterns.is_empty());
        assert_eq!(item(&menus, "s").kind, Kind::Separator, "a separator needs no title");
    }

    #[test]
    fn lazy_backgrounds_must_name_their_items_and_take_clicks_from_on_clicked() {
        let mut menus = Menus::default();
        let generated = json!({ "id": 1, "title": "A" });
        let err = menus.create(&generated, true, true, true).unwrap_err();
        assert_eq!(err, "Extensions using event pages or Service Workers must pass an id parameter to chrome.contextMenus.create");
        assert_eq!(menus.create(&json!({ "id": "a", "title": "A" }), false, true, true), Err(ONCLICK_ERROR.into()));
        assert!(menus.items.is_empty());
        menus.create(&generated, true, false, true).unwrap();
        menus.create(&json!({ "id": "a", "title": "A" }), false, true, false).unwrap();
        assert_eq!(order(&menus), ["1", "a"]);
        assert_eq!(menus.update(&id("a"), &json!({}), true, true), Err(ONCLICK_ERROR.into()));
        assert!(menus.update(&id("a"), &json!({}), false, true).is_ok(), "a persistent background may pass onclick");
    }

    #[test]
    fn create_refuses_duplicate_ids_but_tells_strings_from_integers() {
        let mut menus = built(&[json!({ "id": "a", "title": "A" }), json!({ "id": 1, "title": "One" })]);
        assert_eq!(menus.create(&json!({ "id": "a", "title": "B" }), false, false, false), Err("Cannot create item with duplicate id a".into()));
        assert_eq!(menus.create(&json!({ "id": 1, "title": "B" }), true, false, false), Err("Cannot create item with duplicate id 1".into()));
        menus.create(&json!({ "id": "1", "title": "B" }), false, false, false).unwrap();
        assert_eq!(menus.items.len(), 3);
    }

    #[test]
    fn create_refuses_each_bad_property_with_chromes_error() {
        let mut menus = built(&[json!({ "id": "parent", "title": "P" }), json!({ "id": "box", "title": "B", "type": "checkbox" })]);
        let before = menus.clone();
        let contexts_error = "Error at parameter 'createProperties': Error at property 'contexts': Value must be one of action, all, audio, browser_action, editable, frame, image, launcher, link, page, page_action, selection, video.";
        let cases = [
            (json!({ "type": "button", "title": "x" }), "Error at parameter 'createProperties': Error at property 'type': Value must be one of checkbox, normal, radio, separator."),
            (json!({}), "All menu items except for separators must have a title"),
            (json!({ "type": "checkbox", "title": "" }), "All menu items except for separators must have a title"),
            (json!({ "title": 5 }), "Error at property 'title': Invalid type"),
            (json!({ "title": "x", "checked": true }), "Only items with type \"radio\" or \"checkbox\" can be checked"),
            (json!({ "title": "x", "checked": "yes", "type": "checkbox" }), "Error at property 'checked': Invalid type"),
            (json!({ "title": "x", "parentId": "nobody" }), "Cannot find menu item with id nobody"),
            (json!({ "title": "x", "parentId": 9 }), "Cannot find menu item with id 9"),
            (json!({ "title": "x", "parentId": "box" }), "Parent items must have type \"normal\""),
            (json!({ "title": "x", "documentUrlPatterns": ["https://ok.test/*", "nonsense"] }), "Invalid url pattern 'nonsense'"),
            (json!({ "title": "x", "targetUrlPatterns": ["*"] }), "Invalid url pattern '*'"),
            (json!({ "title": "x", "contexts": ["launcher"] }), "Only packaged apps are allowed to use 'launcher' context"),
            (json!({ "title": "x", "contexts": ["page", "toolbar"] }), contexts_error),
            (json!({ "title": "x", "contexts": [] }), contexts_error),
            (json!({ "title": "x", "contexts": "page" }), contexts_error),
            (json!({ "title": "x", "visible": 1 }), "Error at property 'visible': Invalid type"),
        ];
        for (mut props, error) in cases {
            props["id"] = json!("new");
            assert_eq!(menus.create(&props, false, false, false), Err(error.into()), "{props}");
        }
        assert_eq!(menus, before);
        menus.create(&json!({ "id": "new", "title": "x", "parentId": "parent", "unknown": 1, "documentUrlPatterns": null }), false, false, false).unwrap();
    }

    #[test]
    fn firefox_menus_contexts_load_and_match_nothing() {
        let menus = built(&[json!({ "id": "f", "title": "F", "contexts": ["tab", "tools_menu", "bookmark", "password"] })]);
        let everything = Target { link_url: Some("https://x.test/".into()), selection: "s".into(), editable: true, ..page("https://x.test/") };
        assert!(matching(&menus, &page("https://x.test/")).is_empty());
        assert!(matching(&menus, &everything).is_empty());
        assert!(menus.action_entries().is_empty());
    }

    #[test]
    fn update_changes_title_checked_contexts_and_more() {
        let mut menus = built(&[json!({ "id": "a", "title": "A" }), json!({ "id": "b", "title": "B", "type": "checkbox" })]);
        menus.update(&id("a"), &json!({ "title": "Renamed", "contexts": ["link"], "enabled": false, "visible": false }), false, false).unwrap();
        let a = item(&menus, "a");
        assert_eq!((a.title.as_str(), a.enabled, a.visible), ("Renamed", false, false));
        assert_eq!(a.contexts, BTreeSet::from([Context::Link]));
        menus.update(&id("b"), &json!({ "checked": true, "targetUrlPatterns": ["https://*/*"] }), false, false).unwrap();
        assert!(item(&menus, "b").checked);
        assert_eq!(item(&menus, "b").target_patterns, [MatchPattern::parse("https://*/*").unwrap()]);
        menus.update(&id("b"), &json!({ "type": "normal" }), false, false).unwrap();
        assert!(!item(&menus, "b").checked, "a normal item is never checked");
        menus.update(&id("a"), &json!({ "type": "checkbox", "checked": true }), false, false).unwrap();
        assert!(item(&menus, "a").checked, "the type applies before checked");
        assert_eq!(order(&menus), ["a", "b"], "an update without parentId keeps the item's place");
    }

    #[test]
    fn a_failed_update_changes_nothing() {
        let mut menus = built(&[
            json!({ "id": "top", "title": "Top" }),
            json!({ "id": "child", "title": "Child", "parentId": "top" }),
            json!({ "id": "sep", "type": "separator" }),
        ]);
        let before = menus.clone();
        let cycle = "Cannot set a menu item's parent to itself or one of its descendants";
        let cases = [
            ("ghost", json!({ "title": "x" }), "Cannot find menu item with id ghost"),
            ("top", json!({ "title": "changed", "checked": true }), "Only items with type \"radio\" or \"checkbox\" can be checked"),
            ("top", json!({ "title": "" }), "All menu items except for separators must have a title"),
            ("sep", json!({ "type": "normal" }), "All menu items except for separators must have a title"),
            ("top", json!({ "type": "menu" }), "Error at parameter 'updateProperties': Error at property 'type': Value must be one of checkbox, normal, radio, separator."),
            ("top", json!({ "title": "changed", "parentId": "top" }), cycle),
            ("top", json!({ "parentId": "child" }), cycle),
            ("child", json!({ "parentId": "sep" }), "Parent items must have type \"normal\""),
            ("child", json!({ "enabled": false, "documentUrlPatterns": ["bad"] }), "Invalid url pattern 'bad'"),
        ];
        for (name, props, error) in cases {
            assert_eq!(menus.update(&id(name), &props, false, false), Err(error.into()), "{name} {props}");
        }
        assert_eq!(menus, before);
    }

    #[test]
    fn reparenting_makes_an_item_its_new_parents_last_child() {
        let mut menus = built(&[
            json!({ "id": "a", "title": "A" }),
            json!({ "id": "a1", "title": "A1", "parentId": "a" }),
            json!({ "id": "x", "title": "X" }),
            json!({ "id": "x1", "title": "X1", "parentId": "x" }),
            json!({ "id": "x2", "title": "X2", "parentId": "x" }),
            json!({ "id": "a2", "title": "A2", "parentId": "a" }),
        ]);
        let children = |m: &Menus, parent: Option<&str>| m.children(parent.map(id).as_ref()).map(|i| i.id.to_string()).collect::<Vec<_>>();
        menus.update(&id("x"), &json!({ "parentId": "a" }), false, false).unwrap();
        assert_eq!(order(&menus), ["a", "a1", "x1", "x2", "a2", "x"]);
        assert_eq!(children(&menus, Some("a")), ["a1", "a2", "x"]);
        assert_eq!(children(&menus, Some("x")), ["x1", "x2"]);
        menus.update(&id("a1"), &json!({ "parentId": null }), false, false).unwrap();
        assert_eq!(children(&menus, None), ["a", "a1"]);
    }

    #[test]
    fn remove_takes_the_items_descendants() {
        let mut menus = built(&[
            json!({ "id": "a", "title": "A" }),
            json!({ "id": "b", "title": "B", "parentId": "a" }),
            json!({ "id": "keep", "title": "Keep" }),
            json!({ "id": "c", "title": "C", "parentId": "b" }),
        ]);
        // A parent that moved behind its children still takes them along.
        menus.update(&id("a"), &json!({ "parentId": null }), false, false).unwrap();
        assert_eq!(menus.remove(&id("ghost")), Err("Cannot find menu item with id ghost".into()));
        menus.remove(&id("a")).unwrap();
        assert_eq!(order(&menus), ["keep"]);
        menus.remove_all();
        assert!(menus.items.is_empty());
    }

    #[test]
    fn a_radio_run_keeps_its_last_checked_item_or_checks_its_first() {
        let radio = |name: &str, checked: bool| json!({ "id": name, "title": name, "type": "radio", "checked": checked });
        let mut menus = built(&[radio("r1", false), radio("r2", false)]);
        assert_eq!(checked(&menus), ["r1"]);
        menus.create(&radio("r3", true), false, false, false).unwrap();
        assert_eq!(checked(&menus), ["r3"]);
        menus.create(&radio("r4", false), false, false, false).unwrap();
        assert_eq!(checked(&menus), ["r3"]);
        menus.create(&json!({ "id": "sep", "type": "separator" }), false, false, false).unwrap();
        menus.create(&radio("s1", false), false, false, false).unwrap();
        menus.create(&json!({ "id": "p", "title": "P" }), false, false, false).unwrap();
        menus.create(&json!({ "id": "c1", "title": "C1", "type": "radio", "parentId": "p" }), false, false, false).unwrap();
        assert_eq!(checked(&menus), ["r3", "s1", "c1"], "a separator, a normal item and another parent start new runs");
        menus.update(&id("r1"), &json!({ "visible": false }), false, false).unwrap();
        menus.remove(&id("r3")).unwrap();
        assert_eq!(checked(&menus), ["r1", "s1", "c1"], "runs ignore visibility");
        menus.remove(&id("sep")).unwrap();
        assert_eq!(checked(&menus), ["s1", "c1"], "joined runs keep their last checked item");
    }

    #[test]
    fn clicking_or_checking_a_radio_item_unchecks_the_rest_of_its_run() {
        let mut menus = built(&[
            json!({ "id": "r1", "title": "R1", "type": "radio" }),
            json!({ "id": "r2", "title": "R2", "type": "radio" }),
            json!({ "id": "r3", "title": "R3", "type": "radio" }),
        ]);
        let data = menus.click(&id("r2"), None).unwrap();
        assert_eq!((&data["wasChecked"], &data["checked"]), (&json!(false), &json!(true)));
        assert_eq!(checked(&menus), ["r2"]);
        assert_eq!(menus.click(&id("r2"), None).unwrap()["wasChecked"], json!(true));
        assert_eq!(checked(&menus), ["r2"]);
        menus.update(&id("r1"), &json!({ "checked": true }), false, false).unwrap();
        assert_eq!(checked(&menus), ["r1"]);
        menus.update(&id("r1"), &json!({ "checked": false }), false, false).unwrap();
        assert_eq!(checked(&menus), ["r1"], "unchecking a radio item does nothing, as in Chrome");
        menus.update(&id("r1"), &json!({ "type": "normal" }), false, false).unwrap();
        assert_eq!(checked(&menus), ["r2"]);
    }

    #[test]
    fn clicking_a_checkbox_flips_it_and_reports_both_states() {
        let mut menus = built(&[json!({ "id": "p", "title": "P" }), json!({ "id": "c", "title": "C", "type": "checkbox", "parentId": "p" })]);
        assert_eq!(menus.click(&id("c"), None), Some(json!({ "menuItemId": "c", "parentMenuItemId": "p", "editable": false, "wasChecked": false, "checked": true })));
        assert!(item(&menus, "c").checked);
        assert_eq!(menus.click(&id("c"), None).unwrap()["checked"], json!(false));
        assert_eq!(menus.click(&id("p"), None), Some(json!({ "menuItemId": "p", "editable": false })), "a normal item has no checked state");
        assert_eq!(menus.click(&id("ghost"), None), None);
    }

    #[test]
    fn click_data_describes_what_was_clicked() {
        let mut menus = built(&[json!({ "id": 3, "title": "Any", "contexts": ["all"] })]);
        let three = ItemId::Int(3);
        let link_in_frame = Target { frame_url: Some("https://ads.test/frame".into()), link_url: Some("https://dest.test/".into()), ..page("https://site.test/") };
        assert_eq!(
            menus.click(&three, Some(&link_in_frame)),
            Some(json!({ "menuItemId": 3, "editable": false, "pageUrl": "https://site.test/", "frameUrl": "https://ads.test/frame", "linkUrl": "https://dest.test/" }))
        );
        let image = Target { src_url: Some("https://site.test/a.png".into()), media: Some(Media::Image), ..page("https://site.test/") };
        assert_eq!(
            menus.click(&three, Some(&image)),
            Some(json!({ "menuItemId": 3, "editable": false, "pageUrl": "https://site.test/", "frameId": 0, "srcUrl": "https://site.test/a.png", "mediaType": "image" }))
        );
        let selection = Target { selection: "some words".into(), editable: true, ..page("https://site.test/") };
        assert_eq!(
            menus.click(&three, Some(&selection)),
            Some(json!({ "menuItemId": 3, "editable": true, "pageUrl": "https://site.test/", "frameId": 0, "selectionText": "some words" }))
        );
        let video = Target { src_url: Some("https://site.test/v.webm".into()), media: Some(Media::Video), ..page("https://site.test/") };
        assert_eq!(menus.click(&three, Some(&video)).unwrap()["mediaType"], json!("video"));
        assert_eq!(menus.click(&three, None), Some(json!({ "menuItemId": 3, "editable": false })), "the action menu has no page");
    }

    #[test]
    fn page_is_suppressed_by_links_selections_editables_and_media() {
        let menus = one_per_context();
        let url = "https://site.test/";
        assert_eq!(matching(&menus, &page(url)), ["all", "page"]);
        assert_eq!(matching(&menus, &Target { link_url: Some("https://dest.test/".into()), ..page(url) }), ["all", "link"]);
        assert_eq!(matching(&menus, &Target { selection: "s".into(), ..page(url) }), ["all", "selection"]);
        assert_eq!(matching(&menus, &Target { editable: true, ..page(url) }), ["all", "editable"]);
        for (media, name) in [(Media::Image, "image"), (Media::Video, "video"), (Media::Audio, "audio")] {
            let target = Target { media: Some(media), src_url: Some("https://site.test/m".into()), ..page(url) };
            assert_eq!(matching(&menus, &target), ["all", name]);
        }
        let linked_image = Target { link_url: Some("https://dest.test/".into()), media: Some(Media::Image), src_url: Some("https://site.test/i.png".into()), ..page(url) };
        assert_eq!(matching(&menus, &linked_image), ["all", "link", "image"]);
    }

    #[test]
    fn frame_matches_only_in_subframes_where_page_still_does() {
        let menus = one_per_context();
        let subframe = Target { frame_url: Some("https://frame.test/".into()), ..page("https://site.test/") };
        assert_eq!(matching(&menus, &subframe), ["all", "page", "frame"]);
        let link_in_subframe = Target { link_url: Some("https://dest.test/".into()), ..subframe };
        assert_eq!(matching(&menus, &link_in_subframe), ["all", "frame", "link"]);
    }

    #[test]
    fn document_patterns_test_the_frame_when_there_is_one() {
        let menus = built(&[json!({ "id": "a", "title": "A", "contexts": ["all"], "documentUrlPatterns": ["https://frame.test/*"] })]);
        let subframe = |top: &str, frame: &str| Target { frame_url: Some(frame.into()), ..page(top) };
        assert!(matching(&menus, &page("https://site.test/")).is_empty());
        assert_eq!(matching(&menus, &page("https://frame.test/x")), ["a"]);
        assert_eq!(matching(&menus, &subframe("https://site.test/", "https://frame.test/inner")), ["a"]);
        assert!(matching(&menus, &subframe("https://frame.test/", "https://site.test/")).is_empty());
        assert!(matching(&menus, &page("not a url")).is_empty());
    }

    #[test]
    fn target_patterns_filter_links_and_media_sources() {
        let menus = built(&[
            json!({ "id": "link", "title": "L", "contexts": ["link"], "targetUrlPatterns": ["https://*.dest.test/*"] }),
            json!({ "id": "image", "title": "I", "contexts": ["image"], "targetUrlPatterns": ["*://*/*.png"] }),
            json!({ "id": "audio", "title": "A", "contexts": ["audio"], "targetUrlPatterns": ["https://*/*"] }),
        ]);
        let link = |url: &str| Target { link_url: Some(url.into()), ..page("https://site.test/") };
        assert_eq!(matching(&menus, &link("https://www.dest.test/a")), ["link"]);
        assert!(matching(&menus, &link("https://elsewhere.test/")).is_empty());
        let image = |src: Option<&str>| Target { media: Some(Media::Image), src_url: src.map(Into::into), ..page("https://site.test/") };
        assert_eq!(matching(&menus, &image(Some("http://cdn.test/a.png"))), ["image"]);
        assert!(matching(&menus, &image(Some("http://cdn.test/a.jpg"))).is_empty());
        assert!(matching(&menus, &image(None)).is_empty(), "no source matches no pattern");
        let audio = Target { media: Some(Media::Audio), src_url: Some("http://cdn.test/a.mp3".into()), ..page("https://site.test/") };
        assert!(matching(&menus, &audio).is_empty());
    }

    #[test]
    fn page_entry_is_nothing_a_lone_item_or_the_items_under_the_extensions_name() {
        let target = page("https://site.test/");
        assert_eq!(Menus::default().page_entry("Ext", &target), None);
        let mut menus = built(&[json!({ "id": "a", "title": "A" }), json!({ "id": "linky", "title": "L", "contexts": ["link"] })]);
        assert_eq!(menus.page_entry("Ext", &target), Some(command("a", "A")));
        menus.create(&json!({ "id": "a1", "title": "A1", "parentId": "a" }), false, false, false).unwrap();
        menus.create(&json!({ "id": "a2", "title": "A2", "parentId": "a", "type": "checkbox", "checked": true }), false, false, false).unwrap();
        let a2 = Entry::Item { id: id("a2"), title: "A2".into(), enabled: true, checked: Some(true) };
        let submenu = Entry::Submenu { title: "A".into(), enabled: true, children: vec![command("a1", "A1"), a2] };
        assert_eq!(menus.page_entry("Ext", &target), Some(submenu.clone()), "a lone parent is its own submenu");
        menus.create(&json!({ "id": "b", "title": "B", "enabled": false }), false, false, false).unwrap();
        let b = Entry::Item { id: id("b"), title: "B".into(), enabled: false, checked: None };
        assert_eq!(menus.page_entry("Ext", &target), Some(Entry::Submenu { title: "Ext".into(), enabled: true, children: vec![submenu, b] }));
    }

    #[test]
    fn invisible_items_and_their_children_are_hidden() {
        let mut menus = built(&[
            json!({ "id": "a", "title": "A" }),
            json!({ "id": "a1", "title": "A1", "parentId": "a" }),
            json!({ "id": "a2", "title": "A2", "parentId": "a", "visible": false }),
            json!({ "id": "a3", "title": "A3", "parentId": "a", "contexts": ["link"] }),
            json!({ "id": "b", "title": "B", "visible": false }),
            json!({ "id": "b1", "title": "B1", "parentId": "b" }),
        ]);
        let target = page("https://site.test/");
        assert_eq!(menus.page_entry("Ext", &target), Some(Entry::Submenu { title: "A".into(), enabled: true, children: vec![command("a1", "A1")] }));
        menus.update(&id("a1"), &json!({ "visible": false }), false, false).unwrap();
        assert_eq!(menus.page_entry("Ext", &target), Some(command("a", "A")), "a parent with nothing to show is a plain item");
    }

    #[test]
    fn separators_are_cleaned_at_every_level() {
        let sep = |name: &str, parent: Option<&str>| json!({ "id": name, "type": "separator", "parentId": parent });
        let menus = built(&[
            sep("s0", None),
            json!({ "id": "a", "title": "A" }),
            json!({ "id": "a1", "title": "A1", "parentId": "a" }),
            sep("as1", Some("a")),
            sep("as2", Some("a")),
            json!({ "id": "a2", "title": "A2", "parentId": "a" }),
            sep("as3", Some("a")),
            sep("s1", None),
            json!({ "id": "hidden", "title": "H", "visible": false }),
            sep("s2", None),
            json!({ "id": "b", "title": "B" }),
            sep("s3", None),
        ]);
        let target = page("https://site.test/");
        let a = Entry::Submenu { title: "A".into(), enabled: true, children: vec![command("a1", "A1"), Entry::Separator, command("a2", "A2")] };
        assert_eq!(menus.page_entry("Ext", &target), Some(Entry::Submenu { title: "Ext".into(), enabled: true, children: vec![a, Entry::Separator, command("b", "B")] }));
        assert_eq!(built(&[sep("s", None), json!({ "id": "a", "title": "A" }), sep("t", None)]).page_entry("Ext", &target), Some(command("a", "A")));
        assert_eq!(built(&[sep("s", None)]).page_entry("Ext", &target), None);
    }

    #[test]
    fn titles_take_the_selection_and_are_cut_to_chromes_limits() {
        let menus = built(&[json!({ "id": "s", "title": "Search \"%s\" for %s", "contexts": ["selection"] })]);
        let shown = |selection: &str| match menus.page_entry("Ext", &Target { selection: selection.into(), ..page("https://site.test/") }) {
            Some(Entry::Item { title, .. }) => title,
            other => panic!("{other:?}"),
        };
        assert_eq!(shown("cats"), "Search \"cats\" for cats");
        let fifty = "ж".repeat(50);
        assert_eq!(shown(&fifty), format!("Search \"{fifty}\" for {}…", "ж".repeat(10)), "the selection fits; the title is cut to 75 characters");
        let cut = format!("{}…", "x".repeat(49));
        assert_eq!(shown(&"x".repeat(60)), format!("Search \"{cut}\" for {}…", "x".repeat(10)));
        assert_eq!(title("Find %s", &"y".repeat(51)), format!("Find {}…", "y".repeat(49)));
        assert_eq!(title(&"t".repeat(75), ""), "t".repeat(75));
        assert_eq!(title(&"t".repeat(76), ""), format!("{}…", "t".repeat(74)));
        let action = built(&[json!({ "id": "a", "title": "Open %s", "contexts": ["action"] })]);
        assert_eq!(action.action_entries(), [command("a", "Open ")], "the action menu has no selection");
    }

    #[test]
    fn the_action_menu_shows_the_first_six_action_items() {
        let mut items = vec![
            json!({ "id": "page", "title": "Page" }),
            json!({ "id": "hidden", "title": "Hidden", "contexts": ["action"], "visible": false }),
            json!({ "id": "all", "title": "All", "contexts": ["all"] }),
            json!({ "id": "browser", "title": "Browser", "contexts": ["browser_action"] }),
            json!({ "id": "pageaction", "title": "PageAction", "contexts": ["page_action"] }),
            json!({ "id": "child", "title": "Child", "contexts": ["action"], "parentId": "all" }),
            json!({ "id": "pagechild", "title": "PageChild", "parentId": "all" }),
        ];
        items.extend((1..=5).map(|n| json!({ "id": format!("a{n}"), "title": format!("A{n}"), "contexts": ["action", "page"] })));
        let all = Entry::Submenu { title: "All".into(), enabled: true, children: vec![command("child", "Child")] };
        assert_eq!(
            built(&items).action_entries(),
            [all, command("browser", "Browser"), command("pageaction", "PageAction"), command("a1", "A1"), command("a2", "A2"), command("a3", "A3")]
        );
    }

    #[test]
    fn menus_round_trip_through_json() {
        let menus = built(&[
            json!({ "id": "a", "title": "A", "contexts": ["link", "browser_action"], "documentUrlPatterns": ["https://*/*"], "targetUrlPatterns": ["<all_urls>"] }),
            json!({ "id": 2, "title": "Two", "type": "radio", "parentId": "a", "enabled": false }),
            json!({ "id": "s", "type": "separator", "visible": false, "contexts": ["tools_menu"] }),
        ]);
        let text = serde_json::to_string(&menus).unwrap();
        assert_eq!(serde_json::from_str::<Menus>(&text).unwrap(), menus);
        assert!(text.contains("\"browser_action\"") && text.contains("\"https://*/*\""), "contexts and patterns persist under their API names: {text}");
    }
}
