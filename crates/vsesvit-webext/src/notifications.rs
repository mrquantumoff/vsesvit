//! `chrome.notifications`: one extension's notifications and how each is shown. Pure: the
//! runtime feeds it the shim's calls, whose images the shim has already loaded (the icon
//! comes as PNG), and shows the [`Shown`] it returns as a desktop notification the way Chrome
//! shows one through the Linux portal.

use std::collections::BTreeMap;

use serde_json::Value;

/// The GApplication action a notification's clicks invoke, `app.extension-notification`
/// in the shell, with the [`action_target`] of the click as its `(sss)` parameter.
pub const ACTION: &str = "extension-notification";

/// What every call but `getPermissionLevel` fails with while the user has the extension's
/// notifications turned off.
pub const TURNED_OFF: &str = "Notifications are turned off for this extension.";

/// Chrome's `kNotificationIdLengthLimit`, in bytes.
const MAX_ID: usize = 500;
/// Chrome shows at most two buttons and drops the others.
const MAX_BUTTONS: usize = 2;

const MISSING_PROPERTIES: &str = "Some of the required properties are missing: type, iconUrl, title and message.";
const LOW_PRIORITY: &str = "Low-priority notifications are deprecated on this platform.";
const UNUSABLE_ICON: &str = "Unable to successfully use the provided image.";
const EXTRA_IMAGE: &str = "Image resource provided for notification type != image";
const EXTRA_ITEMS: &str = "List items provided for notification type != list";
const UNEXPECTED_PROGRESS: &str = "The progress value should not be specified for non-progress notification";
const INVALID_PROGRESS: &str = "The progress value should range from 0 to 100";
const ID_TOO_LONG: &str = "The notification's ID should be 500 characters or less";

/// One extension's notifications, by id.
#[derive(Debug, Default)]
pub struct Notifications {
    live: BTreeMap<String, Notification>,
}

/// The type-specific fields sit side by side, as in Chrome: an update may change the type,
/// and what is shown depends on it.
#[derive(Clone, Debug)]
struct Notification {
    kind: Kind,
    title: String,
    message: String,
    context_message: String,
    priority: Priority,
    buttons: Vec<String>,
    items: Vec<(String, String)>,
    progress: u8,
    icon: Vec<u8>,
}

/// `notifications.TemplateType`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Kind {
    Basic,
    Image,
    List,
    Progress,
}

/// A notification as the desktop shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub title: String,
    pub body: Option<String>,
    /// PNG, at most 128 pixels a side.
    pub icon: Vec<u8>,
    pub buttons: Vec<String>,
    pub priority: Priority,
}

/// Chrome's priorities 0, 1 and 2, as the portal's normal, high and urgent. Negative ones,
/// which the portal shows as low, are refused on this platform.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Priority {
    Normal,
    High,
    Urgent,
}

/// What the user activated on a notification. [`Activation::name`] is the portal's name
/// for it, which a click's [`action_target`] carries.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Activation {
    Click,
    Button(usize),
    /// The Settings button Chrome adds to every extension notification.
    Settings,
}

impl Activation {
    pub fn parse(s: &str) -> Option<Activation> {
        match s {
            "default" => Some(Activation::Click),
            "settings" => Some(Activation::Settings),
            index => index.parse().ok().map(Activation::Button),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Activation::Click => "default".to_owned(),
            Activation::Button(index) => index.to_string(),
            Activation::Settings => "settings".to_owned(),
        }
    }
}

/// The [`ACTION`] parameter for `activation` of notification `id` of extension `ext`.
pub fn action_target(ext: &str, id: &str, activation: Activation) -> (String, String, String) {
    (ext.to_owned(), id.to_owned(), activation.name())
}

/// `notifications.PermissionLevel` for the user's switch.
pub fn permission_level(allowed: bool) -> &'static str {
    if allowed { "granted" } else { "denied" }
}

impl Notifications {
    /// `notifications.create(id, options)` with the icon the shim loaded from `iconUrl`. An
    /// existing notification with that id is replaced in place.
    pub fn create(&mut self, id: &str, options: &Value, icon: Option<Vec<u8>>) -> Result<Shown, String> {
        if id.is_empty() {
            return Err("the id must not be empty".into());
        }
        if id.len() > MAX_ID {
            return Err(ID_TOO_LONG.into());
        }
        let options = Options::parse(options)?;
        let Some(kind) = options.kind.filter(|_| options.title.is_some() && options.message.is_some() && options.icon_url) else {
            return Err(MISSING_PROPERTIES.into());
        };
        if options.priority.is_some_and(|p| p < 0) {
            return Err(LOW_PRIORITY.into());
        }
        let icon = icon.ok_or(UNUSABLE_ICON)?;
        if options.image_url != (kind == Kind::Image) {
            return Err(EXTRA_IMAGE.into());
        }
        if options.items.is_empty() == (kind == Kind::List) {
            return Err(EXTRA_ITEMS.into());
        }
        let mut notification = Notification {
            kind,
            title: String::new(),
            message: String::new(),
            context_message: String::new(),
            priority: Priority::Normal,
            buttons: Vec::new(),
            items: Vec::new(),
            progress: 0,
            icon,
        };
        notification.apply(options)?;
        let shown = notification.shown();
        self.live.insert(id.to_owned(), notification);
        Ok(shown)
    }

    /// `notifications.update(id, options)`: the options given, and the icon when the shim
    /// loaded a new one, change the notification. `None` when there is none with that id; a
    /// refused update changes nothing.
    pub fn update(&mut self, id: &str, options: &Value, icon: Option<Vec<u8>>) -> Result<Option<Shown>, String> {
        let options = Options::parse(options)?;
        let Some(live) = self.live.get_mut(id) else { return Ok(None) };
        let mut updated = live.clone();
        updated.apply(options)?;
        if let Some(icon) = icon {
            updated.icon = icon;
        }
        *live = updated;
        Ok(Some(live.shown()))
    }

    /// `notifications.clear(id)`: whether there was one.
    pub fn clear(&mut self, id: &str) -> bool {
        self.live.remove(id).is_some()
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.live.keys().map(String::as_str)
    }

    pub fn shown(&self, id: &str) -> Option<Shown> {
        self.live.get(id).map(Notification::shown)
    }

    /// Forgets every notification and returns their ids.
    pub fn take_all(&mut self) -> Vec<String> {
        std::mem::take(&mut self.live).into_keys().collect()
    }
}

impl Notification {
    /// Chrome's `UpdateNotification` checks: an image or list items only for their type,
    /// progress only for a progress notification.
    fn apply(&mut self, options: Options) -> Result<(), String> {
        if let Some(kind) = options.kind {
            self.kind = kind;
        }
        if let Some(title) = options.title {
            self.title = title;
        }
        if let Some(message) = options.message {
            self.message = message;
        }
        if let Some(p) = options.priority {
            self.priority = priority(p)?;
        }
        if let Some(buttons) = options.buttons {
            self.buttons = buttons;
        }
        if let Some(context_message) = options.context_message {
            self.context_message = context_message;
        }
        if options.image_url && self.kind != Kind::Image {
            return Err(EXTRA_IMAGE.into());
        }
        if let Some(progress) = options.progress {
            if self.kind != Kind::Progress {
                return Err(UNEXPECTED_PROGRESS.into());
            }
            self.progress = u8::try_from(progress).ok().filter(|p| *p <= 100).ok_or(INVALID_PROGRESS)?;
        }
        if !options.items.is_empty() {
            if self.kind != Kind::List {
                return Err(EXTRA_ITEMS.into());
            }
            self.items = options.items;
        }
        Ok(())
    }

    /// Chrome's portal notification: a progress notification's title leads with the
    /// percentage; the body is the context message, the message and a list's items, one
    /// per line. The portal shows no image.
    fn shown(&self) -> Shown {
        let title = match self.kind {
            Kind::Progress => format!("{}% - {}", self.progress, self.title),
            _ => self.title.clone(),
        };
        let mut body = String::new();
        if !self.context_message.is_empty() {
            body.push_str(&self.context_message);
            body.push_str("\n\n");
        }
        if !self.message.is_empty() {
            body.push_str(&self.message);
            body.push('\n');
        }
        if self.kind == Kind::List {
            for (title, message) in &self.items {
                body.push_str(&format!("{title} - {message}\n"));
            }
        }
        let body = body.trim_matches('\n');
        Shown {
            title,
            body: (!body.is_empty()).then(|| body.to_owned()),
            icon: self.icon.clone(),
            buttons: self.buttons.clone(),
            priority: self.priority,
        }
    }
}

fn priority(p: i64) -> Result<Priority, String> {
    match p {
        p if p < 0 => Err(LOW_PRIORITY.into()),
        0 => Ok(Priority::Normal),
        1 => Ok(Priority::High),
        _ => Ok(Priority::Urgent),
    }
}

/// `NotificationOptions` as far as this runtime uses them. The image URLs are only present
/// or not: the shim has loaded them, and only the icon is shown.
struct Options {
    kind: Option<Kind>,
    title: Option<String>,
    message: Option<String>,
    context_message: Option<String>,
    icon_url: bool,
    image_url: bool,
    priority: Option<i64>,
    /// At most [`MAX_BUTTONS`].
    buttons: Option<Vec<String>>,
    items: Vec<(String, String)>,
    progress: Option<i64>,
}

impl Options {
    fn parse(options: &Value) -> Result<Options, String> {
        let kind = prop(options, "type")
            .map(|v| match v.as_str() {
                Some("basic") => Ok(Kind::Basic),
                Some("image") => Ok(Kind::Image),
                Some("list") => Ok(Kind::List),
                Some("progress") => Ok(Kind::Progress),
                _ => Err("Error at parameter 'options': Error at property 'type': Value must be one of basic, image, list, progress.".to_owned()),
            })
            .transpose()?;
        let buttons = match prop(options, "buttons") {
            Some(v) => {
                let mut titles = v.as_array().ok_or_else(|| invalid("buttons"))?.iter().map(|b| required(b, "title")).collect::<Result<Vec<_>, _>>()?;
                titles.truncate(MAX_BUTTONS);
                Some(titles)
            }
            None => None,
        };
        let items = match prop(options, "items") {
            Some(v) => v.as_array().ok_or_else(|| invalid("items"))?.iter().map(|item| Ok((required(item, "title")?, required(item, "message")?))).collect::<Result<Vec<_>, String>>()?,
            None => Vec::new(),
        };
        Ok(Options {
            kind,
            title: string(options, "title")?,
            message: string(options, "message")?,
            context_message: string(options, "contextMessage")?,
            icon_url: string(options, "iconUrl")?.is_some(),
            image_url: string(options, "imageUrl")?.is_some(),
            priority: integer(options, "priority")?,
            buttons,
            items,
            progress: integer(options, "progress")?,
        })
    }
}

fn invalid(name: &str) -> String {
    format!("Error at property '{name}': Invalid type")
}

/// Property `name` of `object`; `null` counts as absent, as for any optional property.
fn prop<'a>(object: &'a Value, name: &str) -> Option<&'a Value> {
    object.get(name).filter(|v| !v.is_null())
}

fn string(object: &Value, name: &str) -> Result<Option<String>, String> {
    prop(object, name).map(|v| v.as_str().map(str::to_owned).ok_or_else(|| invalid(name))).transpose()
}

fn required(object: &Value, name: &str) -> Result<String, String> {
    string(object, name)?.ok_or_else(|| invalid(name))
}

fn integer(object: &Value, name: &str) -> Result<Option<i64>, String> {
    prop(object, name).map(|v| v.as_i64().ok_or_else(|| invalid(name))).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PNG: &[u8] = b"\x89PNG icon";

    fn basic() -> Value {
        json!({ "type": "basic", "iconUrl": "icon.png", "title": "Hello", "message": "World" })
    }

    fn with(mut options: Value, extra: Value) -> Value {
        for (k, v) in extra.as_object().unwrap() {
            options[k] = v.clone();
        }
        options
    }

    fn created(id: &str, options: Value) -> (Notifications, Shown) {
        let mut n = Notifications::default();
        let shown = n.create(id, &options, Some(PNG.to_vec())).unwrap();
        (n, shown)
    }

    fn create_error(options: Value) -> String {
        Notifications::default().create("n", &options, Some(PNG.to_vec())).unwrap_err()
    }

    #[test]
    fn create_needs_type_icon_title_and_message() {
        for missing in ["type", "iconUrl", "title", "message"] {
            let mut options = basic();
            options.as_object_mut().unwrap().remove(missing);
            assert_eq!(create_error(options), MISSING_PROPERTIES, "without {missing}");
        }
        assert_eq!(create_error(with(basic(), json!({ "title": null }))), MISSING_PROPERTIES, "null is absent");
        let (_, shown) = created("n", with(basic(), json!({ "message": "" })));
        assert_eq!(shown.body, None, "an empty message is given");
    }

    #[test]
    fn create_refuses_what_chrome_refuses() {
        assert_eq!(create_error(with(basic(), json!({ "priority": -1 }))), LOW_PRIORITY);
        assert_eq!(create_error(with(basic(), json!({ "imageUrl": "big.png" }))), EXTRA_IMAGE);
        assert_eq!(create_error(with(basic(), json!({ "type": "image" }))), EXTRA_IMAGE, "an image notification without an image");
        assert_eq!(create_error(with(basic(), json!({ "items": [{ "title": "a", "message": "b" }] }))), EXTRA_ITEMS);
        assert_eq!(create_error(with(basic(), json!({ "type": "list" }))), EXTRA_ITEMS, "a list without items");
        assert_eq!(create_error(with(basic(), json!({ "type": "list", "items": [] }))), EXTRA_ITEMS);
        assert_eq!(create_error(with(basic(), json!({ "progress": 10 }))), UNEXPECTED_PROGRESS);
        assert_eq!(create_error(with(basic(), json!({ "type": "progress", "progress": 150 }))), INVALID_PROGRESS);
        assert_eq!(create_error(with(basic(), json!({ "type": "progress", "progress": -1 }))), INVALID_PROGRESS);
        assert!(create_error(with(basic(), json!({ "type": "fancy" }))).contains("Value must be one of basic, image, list, progress."));
        assert_eq!(create_error(with(basic(), json!({ "title": 5 }))), "Error at property 'title': Invalid type");
        assert_eq!(create_error(with(basic(), json!({ "priority": 1.5 }))), "Error at property 'priority': Invalid type");
        assert_eq!(Notifications::default().create("n", &basic(), None).unwrap_err(), UNUSABLE_ICON);
        assert_eq!(Notifications::default().create(&"x".repeat(501), &basic(), Some(PNG.to_vec())).unwrap_err(), ID_TOO_LONG);
        assert!(Notifications::default().create(&"x".repeat(500), &basic(), Some(PNG.to_vec())).is_ok());
        assert!(Notifications::default().create("", &basic(), Some(PNG.to_vec())).is_err());
    }

    #[test]
    fn a_failed_create_keeps_what_was_there() {
        let (mut n, before) = created("n", basic());
        assert!(n.create("n", &with(basic(), json!({ "priority": -2 })), Some(PNG.to_vec())).is_err());
        assert_eq!(n.shown("n"), Some(before));
    }

    #[test]
    fn create_with_a_known_id_replaces_it() {
        let (mut n, _) = created("n", basic());
        n.create("n", &with(basic(), json!({ "title": "Again" })), Some(PNG.to_vec())).unwrap();
        assert_eq!(n.ids().collect::<Vec<_>>(), ["n"]);
        assert_eq!(n.shown("n").unwrap().title, "Again");
    }

    #[test]
    fn a_basic_notification_shows_its_context_message_message_and_two_buttons() {
        let options = with(basic(), json!({ "contextMessage": "Mail", "priority": 2, "buttons": [{ "title": "Yes" }, { "title": "No", "iconUrl": "no.png" }, { "title": "Dropped" }] }));
        let (_, shown) = created("n", options);
        assert_eq!(
            shown,
            Shown { title: "Hello".into(), body: Some("Mail\n\nWorld".into()), icon: PNG.to_vec(), buttons: vec!["Yes".into(), "No".into()], priority: Priority::Urgent }
        );
        assert_eq!(created("n", with(basic(), json!({ "contextMessage": "Mail", "message": "" }))).1.body.as_deref(), Some("Mail"));
        assert_eq!(created("n", with(basic(), json!({ "message": "\nWorld\n\n" }))).1.body.as_deref(), Some("World"));
    }

    #[test]
    fn priorities_map_to_the_portals() {
        let priority_of = |p: Value| created("n", with(basic(), json!({ "priority": p }))).1.priority;
        assert_eq!(created("n", basic()).1.priority, Priority::Normal);
        assert_eq!(priority_of(json!(0)), Priority::Normal);
        assert_eq!(priority_of(json!(1)), Priority::High);
        assert_eq!(priority_of(json!(2)), Priority::Urgent);
        assert_eq!(priority_of(json!(7)), Priority::Urgent);
    }

    #[test]
    fn a_list_shows_its_items_and_an_image_shows_no_image() {
        let list = with(basic(), json!({ "type": "list", "message": "", "items": [{ "title": "Ann", "message": "Lunch?" }, { "title": "Bo", "message": "Done" }] }));
        assert_eq!(created("n", list).1.body.as_deref(), Some("Ann - Lunch?\nBo - Done"));
        let list = with(basic(), json!({ "type": "list", "items": [{ "title": "Ann", "message": "Lunch?" }] }));
        assert_eq!(created("n", list).1.body.as_deref(), Some("World\nAnn - Lunch?"));
        let (_, image) = created("n", with(basic(), json!({ "type": "image", "imageUrl": "big.png" })));
        assert_eq!((image.title.as_str(), image.body.as_deref()), ("Hello", Some("World")));
    }

    #[test]
    fn a_progress_title_leads_with_the_percentage() {
        let (_, shown) = created("n", with(basic(), json!({ "type": "progress", "title": "Copying", "progress": 40 })));
        assert_eq!(shown.title, "40% - Copying");
        assert_eq!(created("n", with(basic(), json!({ "type": "progress" }))).1.title, "0% - Hello");
    }

    #[test]
    fn update_merges_the_options_given() {
        let (mut n, _) = created("n", with(basic(), json!({ "buttons": [{ "title": "Yes" }] })));
        let shown = n.update("n", &json!({ "type": "progress", "progress": 40 }), None).unwrap().unwrap();
        assert_eq!((shown.title.as_str(), shown.body.as_deref(), shown.buttons.as_slice()), ("40% - Hello", Some("World"), ["Yes".to_owned()].as_slice()));
        let shown = n.update("n", &json!({ "message": "Done", "priority": 1, "buttons": [] }), Some(b"new".to_vec())).unwrap().unwrap();
        assert_eq!(shown, Shown { title: "40% - Hello".into(), body: Some("Done".into()), icon: b"new".to_vec(), buttons: vec![], priority: Priority::High });
        assert_eq!(n.shown("n"), Some(shown));
    }

    #[test]
    fn update_of_an_unknown_id_is_none_and_a_refused_one_changes_nothing() {
        let (mut n, before) = created("n", basic());
        assert_eq!(n.update("other", &json!({ "title": "x" }), None), Ok(None));
        assert_eq!(n.update("n", &json!({ "title": "Changed", "progress": 5 }), Some(b"new".to_vec())), Err(UNEXPECTED_PROGRESS.to_owned()));
        assert_eq!(n.update("n", &json!({ "title": "Changed", "priority": -1 }), None), Err(LOW_PRIORITY.to_owned()));
        assert_eq!(n.update("n", &json!({ "title": "Changed", "imageUrl": "big.png" }), None), Err(EXTRA_IMAGE.to_owned()));
        assert_eq!(n.update("n", &json!({ "items": [{ "title": "a", "message": "b" }] }), None), Err(EXTRA_ITEMS.to_owned()));
        assert_eq!(n.update("n", &json!({ "type": "progress", "progress": 101 }), None), Err(INVALID_PROGRESS.to_owned()));
        assert_eq!(n.shown("n"), Some(before));
        assert!(n.update("n", &json!({ "type": "image", "imageUrl": "big.png" }), None).unwrap().is_some(), "checked against the new type");
        assert!(n.update("n", &json!({ "type": "list", "items": [] }), None).unwrap().is_some(), "empty items are not given");
    }

    #[test]
    fn clear_ids_and_take_all() {
        let (mut n, _) = created("b", basic());
        n.create("a", &basic(), Some(PNG.to_vec())).unwrap();
        assert_eq!(n.ids().collect::<Vec<_>>(), ["a", "b"]);
        assert!(n.clear("a"));
        assert!(!n.clear("a"));
        assert_eq!(n.shown("a"), None);
        n.create("c", &basic(), Some(PNG.to_vec())).unwrap();
        assert_eq!(n.take_all(), ["b", "c"]);
        assert_eq!(n.ids().count(), 0);
    }

    #[test]
    fn activations_round_trip_through_their_portal_names() {
        for (activation, name) in [(Activation::Click, "default"), (Activation::Button(0), "0"), (Activation::Button(1), "1"), (Activation::Settings, "settings")] {
            assert_eq!(activation.name(), name);
            assert_eq!(Activation::parse(name), Some(activation));
        }
        assert_eq!(Activation::parse("close"), None);
        assert_eq!(action_target("ext@x", "n", Activation::Button(1)), ("ext@x".to_owned(), "n".to_owned(), "1".to_owned()));
        assert_eq!((permission_level(true), permission_level(false)), ("granted", "denied"));
    }
}
