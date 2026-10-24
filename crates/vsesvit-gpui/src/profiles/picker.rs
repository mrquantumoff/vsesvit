//! Owned by W3.4.
//! The profile picker's process (design section 3.1 step 4): GPUI with the picker alone, no engine,
//! no profile lock. A choice starts this image again with `--profile-dir <root>`; then the picker
//! exits. Until W3.4 lands the view is a stub: one row per profile, no cards, no Add.

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, size,
};
use gpui_component::ActiveTheme as _;
use vsesvit_core::profiles::ProfilesDir;

use crate::app::quit::{Processes, RelaunchArgs, relaunch};
use crate::theme;

pub fn run(dir: ProfilesDir) -> ExitCode {
    let profiles: Vec<(SharedString, PathBuf)> =
        dir.load().profiles().iter().map(|p| (SharedString::from(p.name.clone()), dir.root(&p.id))).collect();
    let chosen: Rc<RefCell<Option<PathBuf>>> = Rc::default();
    let picked = chosen.clone();
    gpui_platform::application().with_assets(theme::icons::VsesvitAssets).run(move |cx| {
        theme::init(cx);
        if let Err(e) = open(Picker { profiles, chosen: picked }, cx) {
            crate::platform::fatal_message(&format!("vsesvit: the profile picker did not open: {e}"));
            cx.quit();
        }
    });
    if let Some(root) = chosen.take() {
        relaunch(&RelaunchArgs::profile(root), &Processes);
    }
    ExitCode::SUCCESS
}

fn open(picker: Picker, cx: &mut App) -> Result<(), String> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(760.), px(520.)), cx))),
        titlebar: Some(TitlebarOptions { title: Some("Who's using Vsesvit?".into()), ..Default::default() }),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        theme::follow_desktop(window);
        window.on_window_should_close(cx, |_, cx| {
            cx.quit();
            true
        });
        cx.new(|_| picker)
    })
    .map(drop)
    .map_err(|e| e.to_string())
}

pub struct Picker {
    profiles: Vec<(SharedString, PathBuf)>,
    chosen: Rc<RefCell<Option<PathBuf>>>,
}

impl Picker {
    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some((_, root)) = self.profiles.get(index) {
            *self.chosen.borrow_mut() = Some(root.clone());
            cx.quit();
        }
    }
}

impl Render for Picker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rows = self.profiles.iter().enumerate().map(|(i, (name, _))| {
            div()
                .id(("profile", i))
                .px_4()
                .py_2()
                .rounded_md()
                .bg(theme.secondary)
                .hover(|s| s.bg(theme.secondary_hover))
                .child(name.clone())
                .on_click(cx.listener(move |picker, _, _, cx| picker.choose(i, cx)))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_8()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().text_xl().child("Who's using Vsesvit?"))
            .children(rows)
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    #[gpui::test]
    fn choosing_a_profile_records_its_root_and_quits(cx: &mut TestAppContext) {
        let chosen: Rc<RefCell<Option<PathBuf>>> = Rc::default();
        let profiles = vec![("Default".into(), PathBuf::from("/p/Default")), ("Work".into(), PathBuf::from("/p/Work"))];
        let picker = cx.new(|_| Picker { profiles, chosen: chosen.clone() });
        picker.update(cx, |picker, cx| picker.choose(7, cx));
        assert_eq!(*chosen.borrow(), None, "no such row");
        picker.update(cx, |picker, cx| picker.choose(1, cx));
        assert_eq!(*chosen.borrow(), Some(PathBuf::from("/p/Work")));
        let args = RelaunchArgs::profile(PathBuf::from("/p/Work")).command_line();
        assert_eq!(args, ["--profile-dir", "/p/Work"].map(std::ffi::OsString::from));
    }
}
