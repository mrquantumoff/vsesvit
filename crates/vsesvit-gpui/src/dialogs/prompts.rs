//! Owned by W2.1.
//! The three small prompts every feature asks with (UA section 2.6, dialogs section 18): a confirm, a
//! line of text, a choice. Each opens as a dialog window over its parent and resolves once: closing
//! the window any way but its accept button is a cancel. A text answer is trimmed and an empty one
//! is a cancel. A destructive confirm makes Cancel the default: Enter cancels, only a click on the
//! red button accepts.

use futures::channel::oneshot;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div,
    px, size,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Disableable as _};

use super::host::{DialogSpec, present};
use crate::theme::tokens;

/// What a text prompt's field holds, as the answer: trimmed, and `None` when nothing is left.
pub fn text_answer(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Asks to go ahead with something. `true` only for the accept button (or Enter, unless
/// `destructive`).
pub fn confirm(parent: AnyWindowHandle, heading: &str, body: &str, accept: &str, destructive: bool, cx: &mut App) -> Task<bool> {
    let ask = Ask { heading: heading.to_owned().into(), body: Some(body.to_owned().into()), accept: accept.to_owned().into(), destructive };
    let answer = open(parent, ask, Kind::Confirm, cx);
    cx.spawn(async move |_| matches!(answer.await, Ok(Answer::Yes)))
}

/// Asks for a line of text, `initial` selected in the field.
pub fn prompt_text(parent: AnyWindowHandle, heading: &str, initial: &str, accept: &str, cx: &mut App) -> Task<Option<String>> {
    let ask = Ask { heading: heading.to_owned().into(), body: None, accept: accept.to_owned().into(), destructive: false };
    let answer = open(parent, ask, Kind::Text(initial.to_owned()), cx);
    cx.spawn(async move |_| match answer.await {
        Ok(Answer::Text(text)) => text_answer(&text),
        _ => None,
    })
}

/// Asks to pick one of `options`; accept stays off until one is picked.
pub fn prompt_choice(parent: AnyWindowHandle, heading: &str, body: &str, options: Vec<String>, accept: &str, cx: &mut App) -> Task<Option<usize>> {
    let ask = Ask { heading: heading.to_owned().into(), body: Some(body.to_owned().into()), accept: accept.to_owned().into(), destructive: false };
    let answer = open(parent, ask, Kind::Choice(options), cx);
    cx.spawn(async move |_| match answer.await {
        Ok(Answer::Choice(index)) => Some(index),
        _ => None,
    })
}

struct Ask {
    heading: SharedString,
    body: Option<SharedString>,
    accept: SharedString,
    destructive: bool,
}

enum Kind {
    Confirm,
    Text(String),
    Choice(Vec<String>),
}

/// What the accept button answers. A dropped sender (Cancel, Escape, the window closed) is a cancel.
enum Answer {
    Yes,
    Text(String),
    Choice(usize),
}

fn open(parent: AnyWindowHandle, ask: Ask, kind: Kind, cx: &mut App) -> oneshot::Receiver<Answer> {
    let (tx, rx) = oneshot::channel();
    let height = match &kind {
        Kind::Confirm | Kind::Text(_) => 180.,
        Kind::Choice(options) => 180. + 32. * options.len() as f32,
    };
    let spec = DialogSpec { title: ask.heading.clone(), size: size(px(420.), px(height)) };
    present(parent, spec, move |window, cx| cx.new(|cx| PromptView::new(ask, kind, tx, window, cx)), cx);
    rx
}

/// Public for tests and the self-test, which answer it as a user would.
pub struct PromptView {
    ask: Ask,
    field: Option<Entity<InputState>>,
    options: Vec<String>,
    chosen: Option<usize>,
    reply: Option<oneshot::Sender<Answer>>,
    focus: FocusHandle,
    _watch: Option<gpui::Subscription>,
}

impl PromptView {
    fn new(ask: Ask, kind: Kind, reply: oneshot::Sender<Answer>, window: &mut Window, cx: &mut Context<Self>) -> PromptView {
        let focus = cx.focus_handle();
        let (field, options, watch) = match kind {
            Kind::Confirm => (None, Vec::new(), None),
            Kind::Choice(options) => (None, options, None),
            Kind::Text(initial) => {
                let field = cx.new(|cx| InputState::new(window, cx));
                field.update(cx, |field, cx| {
                    field.set_value(initial, window, cx);
                    field.focus(window, cx);
                    field.select_all(window, cx);
                });
                let watch = cx.subscribe_in(&field, window, |this, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.accept(window, cx);
                    }
                });
                (Some(field), Vec::new(), Some(watch))
            }
        };
        if field.is_none() {
            window.focus(&focus, cx);
        }
        PromptView { ask, field, options, chosen: None, reply: Some(reply), focus, _watch: watch }
    }

    /// The accept button, as a click on it.
    pub fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let answer = match (&self.field, self.options.is_empty()) {
            (Some(field), _) => Answer::Text(field.read(cx).value().to_string()),
            (None, true) => Answer::Yes,
            (None, false) => match self.chosen {
                Some(index) => Answer::Choice(index),
                None => return,
            },
        };
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(answer);
        }
        window.remove_window();
    }

    /// Cancel, Escape: the sender drops with the window.
    pub fn cancel(&mut self, window: &mut Window, _: &mut Context<Self>) {
        self.reply = None;
        window.remove_window();
    }

    /// Picks an option of a choice prompt, as a click on it.
    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.options.len() {
            self.chosen = Some(index);
            cx.notify();
        }
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => self.cancel(window, cx),
            "enter" if self.field.is_none() => {
                if self.ask.destructive {
                    self.cancel(window, cx);
                } else {
                    self.accept(window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for PromptView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let options = self.options.iter().enumerate().map(|(index, label)| {
            let chosen = self.chosen == Some(index);
            div()
                .id(("choice", index))
                .px_2()
                .py_1()
                .rounded(theme.radius)
                .when(chosen, |d| d.bg(theme.accent).text_color(theme.accent_foreground))
                .child(label.clone())
                .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)))
        });
        let accept = Button::new("accept")
            .label(self.ask.accept.clone())
            .disabled(!self.options.is_empty() && self.chosen.is_none())
            .on_click(cx.listener(|this, _, window, cx| this.accept(window, cx)));
        let accept = if self.ask.destructive { accept.danger() } else { accept.primary() };
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(tokens::TEXT)
            .child(div().text_size(tokens::TEXT_HEADING).child(self.ask.heading.clone()))
            .children(self.ask.body.clone().map(|body| div().text_color(theme.muted_foreground).child(body)))
            .children(self.field.as_ref().map(Input::new))
            .children(options)
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("cancel").label("Cancel").on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))))
                    .child(accept),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Render, TestAppContext, VisualContext as _, VisualTestContext, div};

    use super::*;

    #[test]
    fn a_text_answer_is_trimmed_and_an_empty_one_is_a_cancel() {
        assert_eq!(text_answer("  Work  "), Some("Work".to_owned()));
        assert_eq!(text_answer("a b"), Some("a b".to_owned()));
        assert_eq!(text_answer("   "), None);
        assert_eq!(text_answer(""), None);
    }

    fn done<T: 'static>(cx: &mut TestAppContext, answer: Task<T>) -> T {
        cx.run_until_parked();
        futures::FutureExt::now_or_never(answer).expect("the prompt answered")
    }

    struct Parent;

    impl Render for Parent {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn parent(cx: &mut TestAppContext) -> AnyWindowHandle {
        cx.update(crate::theme::init);
        let (_, vcx) = cx.add_window_view(|_, _| Parent);
        vcx.window_handle()
    }

    fn prompt(cx: &mut TestAppContext, before: usize) -> (gpui::WindowHandle<PromptView>, &mut VisualTestContext) {
        cx.run_until_parked();
        let windows = cx.windows();
        assert_eq!(windows.len(), before + 1, "the prompt opened a window");
        let handle = windows.last().unwrap().downcast::<PromptView>().expect("a prompt");
        (handle, VisualTestContext::from_window(handle.into(), cx).into_mut())
    }

    #[gpui::test]
    fn a_text_prompt_answers_trimmed_text_and_nothing_for_blank(cx: &mut TestAppContext) {
        let parent = parent(cx);
        let answer = cx.update(|cx| prompt_text(parent, "Rename folder", "  Old  ", "Rename", cx));
        let (handle, vcx) = prompt(cx, 1);
        vcx.simulate_input(" New name ");
        handle.update(vcx, |view, window, cx| view.accept(window, cx)).unwrap();
        vcx.run_until_parked();
        assert_eq!(done(cx, answer), Some("New name".to_owned()));
        assert_eq!(cx.windows().len(), 1, "the prompt closed");

        let answer = cx.update(|cx| prompt_text(parent, "Rename folder", "Old", "Rename", cx));
        let (handle, vcx) = prompt(cx, 1);
        handle.update(vcx, |view, window, cx| {
            view.field.as_ref().unwrap().update(cx, |f, cx| f.set_value("   ", window, cx));
            view.accept(window, cx);
        })
        .unwrap();
        assert_eq!(done(cx, answer), None);
    }

    #[gpui::test]
    fn a_destructive_confirm_cancels_on_enter_and_accepts_only_its_button(cx: &mut TestAppContext) {
        let parent = parent(cx);
        let answer = cx.update(|cx| confirm(parent, "Delete folder?", "Its bookmarks go too.", "Delete", true, cx));
        let (_, vcx) = prompt(cx, 1);
        vcx.simulate_keystrokes("enter");
        vcx.run_until_parked();
        assert!(!done(cx, answer));

        let answer = cx.update(|cx| confirm(parent, "Delete folder?", "Its bookmarks go too.", "Delete", true, cx));
        let (handle, vcx) = prompt(cx, 1);
        handle.update(vcx, |view, window, cx| view.accept(window, cx)).unwrap();
        assert!(done(cx, answer));

        let answer = cx.update(|cx| confirm(parent, "Open 30 tabs?", "", "Open", false, cx));
        let (_, vcx) = prompt(cx, 1);
        vcx.simulate_keystrokes("enter");
        assert!(done(cx, answer), "Enter accepts a plain confirm");
    }

    #[gpui::test]
    fn closing_a_prompt_or_its_parent_is_a_cancel(cx: &mut TestAppContext) {
        let parent = parent(cx);
        let answer = cx.update(|cx| prompt_choice(parent, "Move to", "Pick a folder", vec!["Bar".into(), "Other".into()], "Move", cx));
        let (handle, vcx) = prompt(cx, 1);
        handle.update(vcx, |view, window, cx| view.accept(window, cx)).unwrap();
        assert_eq!(vcx.windows().len(), 2, "nothing picked, so accept does nothing");
        handle.update(vcx, |view, window, cx| {
            view.choose(1, cx);
            view.accept(window, cx);
        })
        .unwrap();
        assert_eq!(done(cx, answer), Some(1));

        let answer = cx.update(|cx| prompt_text(parent, "Name", "x", "Save", cx));
        cx.run_until_parked();
        parent.update(cx, |_, window, _| window.remove_window()).unwrap();
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 0, "the prompt went with its parent");
        assert_eq!(done(cx, answer), None);
    }
}
