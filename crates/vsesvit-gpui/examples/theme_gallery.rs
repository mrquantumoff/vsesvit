//! Owned by W0.4.
//! Every theme token and icon in light and dark (UA U02). Runs only under
//! `cargo xtask probe theme_gallery`.
//!
//! It draws browser chrome from the theme, applies the light pref and then the dark one, and
//! captures its own window after each (`GetImage` of the window on X11, `PrintWindow` of its
//! client area on Windows) into `theme-light.png` and `theme-dark.png` in the working directory,
//! which the launcher makes `target/probe/theme_gallery`. It exits 1 unless each capture shows the
//! mode's window background, ink in every icon and the picture avatar, and the two captures differ.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Bounds, Context, FontWeight, Hsla, IntoElement, ParentElement, Pixels, Render, SharedString,
    Styled, Window, WindowBounds, WindowHandle, WindowOptions, canvas, div, px,
};
use gpui_component::{ActiveTheme as _, Icon, Theme, ThemeColor};
use image::RgbaImage;
use vsesvit_core::prefs;
use vsesvit_core::profiles::ProfileColor;
use vsesvit_core::tab_groups::GroupColor;
use vsesvit_gpui::theme::{self, avatar::Avatar, colors, icons::VsIcon, tokens};

/// How long a capture may take to show the mode's background before the run fails.
const SETTLE: Duration = Duration::from_secs(20);
static EXIT: AtomicU8 = AtomicU8::new(1);

fn main() -> ExitCode {
    let env = vsesvit_testenv::require();
    eprintln!("theme_gallery: in {}", env.describe());
    let dir = std::env::current_dir().expect("a working directory");
    let picture = dir.join("avatar-picture.png");
    if let Err(e) = gradient().save(&picture) {
        eprintln!("theme_gallery: {}: {e}", picture.display());
        return ExitCode::FAILURE;
    }
    gpui_platform::application().with_assets(theme::icons::VsesvitAssets).run(move |cx| {
        theme::init(cx);
        let marks = Marks::default();
        let gallery = Gallery { marks: marks.clone(), picture };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, tokens::WINDOW_DEFAULT, cx))),
            focus: false,
            ..Default::default()
        };
        let window = cx.open_window(options, |_, cx| cx.new(|_| gallery)).expect("the gallery window opens");
        cx.spawn(async move |cx| {
            let code = match capture_both(window, &marks, &dir, cx).await {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("theme_gallery: FAILED: {e}");
                    1
                }
            };
            EXIT.store(code, Ordering::SeqCst);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    ExitCode::from(EXIT.load(Ordering::SeqCst))
}

/// A 72 px square for the picture avatar, unlike every profile colour.
fn gradient() -> RgbaImage {
    RgbaImage::from_fn(72, 72, |x, y| image::Rgba([255, (x * 3) as u8, (y * 3) as u8, 255]))
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum Mark {
    /// Plain window background.
    Background,
    Icon(usize),
    Picture,
}

/// Where each checked element was painted, in logical pixels; filled while the window paints.
#[derive(Clone, Default)]
struct Marks(Rc<RefCell<HashMap<Mark, Bounds<Pixels>>>>);

impl Marks {
    /// An empty element over its parent that records the parent's bounds as `mark`.
    fn track(&self, mark: Mark) -> impl IntoElement {
        let marks = self.0.clone();
        canvas(move |bounds, _, _| _ = marks.borrow_mut().insert(mark, bounds), |_, _, _, _| {}).absolute().top_0().left_0().size_full()
    }

    fn get(&self, mark: Mark) -> Result<Bounds<Pixels>, String> {
        self.0.borrow().get(&mark).copied().ok_or(format!("{mark:?} was never painted"))
    }
}

struct Gallery {
    marks: Marks,
    picture: PathBuf,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(tokens::TEXT)
            .font_family(theme.font_family.clone())
            .child(toolbar(&theme))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .min_h_0()
                    .child(sidebar(&theme))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(tab_strip(&theme))
                            .child(private_bar())
                            .child(bookmarks_bar(&theme))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .gap_4()
                                    .p_3()
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .child(heading("Icons", Some(self.marks.track(Mark::Background).into_any_element())))
                                            .child(icon_grid(&self.marks, &theme))
                                            .child(heading("Palette", None))
                                            .child(palette(&theme)),
                                    )
                                    .child(
                                        div()
                                            .w(px(300.))
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .child(heading("Tab groups", None))
                                            .child(groups(&theme))
                                            .child(heading("Profiles", None))
                                            .child(profiles(&self.marks, &self.picture))
                                            .child(heading("Type", None))
                                            .child(type_samples(&theme))
                                            .child(heading("Overlays", None))
                                            .child(overlays(&theme)),
                                    ),
                            ),
                    ),
            )
    }
}

fn icon(icon: VsIcon, size: Pixels) -> Icon {
    Icon::new(icon).size(size)
}

fn heading(text: &'static str, tracked: Option<AnyElement>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_size(tokens::TEXT_HEADING)
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
        .when_some(tracked, |row, mark| row.child(div().relative().size(px(24.)).child(mark)))
}

fn button(glyph: VsIcon, theme: &ThemeColor) -> impl IntoElement {
    div()
        .size(tokens::TOOLBAR_BUTTON)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .when(glyph == VsIcon::Reload, |b| b.bg(theme.list_hover))
        .child(icon(glyph, tokens::TOOLBAR_ICON))
}

fn toolbar(theme: &Theme) -> impl IntoElement {
    let omnibox = div()
        .flex_1()
        .max_w(tokens::OMNIBOX_COMPACT_MAX_W)
        .h(tokens::OMNIBOX_H)
        .flex()
        .items_center()
        .gap(tokens::ICON_GAP)
        .px_1()
        .rounded(tokens::OMNIBOX_RADIUS)
        .bg(theme.input_background())
        .border_1()
        .border_color(theme.input)
        .child(
            div()
                .size(tokens::OMNIBOX_ICON_BUTTON)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(VsIcon::Secure, tokens::TOOLBAR_ICON)),
        )
        .child(div().flex_1().child("example.org/gallery"))
        .child(
            div()
                .min_h(tokens::ZOOM_CHIP_MIN_H)
                .px(tokens::ZOOM_CHIP_PADDING.left)
                .flex()
                .items_center()
                .rounded(tokens::ZOOM_CHIP_RADIUS)
                .bg(theme.list_hover)
                .text_size(tokens::TEXT_SMALL)
                .child("110%"),
        )
        .child(
            div()
                .size(tokens::OMNIBOX_ICON_BUTTON)
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.primary)
                .child(icon(VsIcon::StarFilled, tokens::TOOLBAR_ICON)),
        );
    div()
        .h(tokens::TOOLBAR_H)
        .flex_none()
        .flex()
        .items_center()
        .gap(tokens::TOOLBAR_SPACING)
        .px(tokens::TOOLBAR_EDGE)
        .bg(theme.title_bar)
        .border_b_1()
        .border_color(theme.title_bar_border)
        .children([VsIcon::SidebarLeft, VsIcon::Back, VsIcon::Forward, VsIcon::Reload].map(|g| button(g, theme)))
        .child(div().flex_1().flex().justify_center().child(omnibox))
        .children([VsIcon::Download, VsIcon::Puzzle].map(|g| button(g, theme)))
        .child(Avatar::new("Demo", ProfileColor::Teal, None, false))
        .child(button(VsIcon::Menu, theme))
}

fn tab_row(glyph: VsIcon, title: &'static str, selected: bool, group: Option<Hsla>, theme: &Theme) -> impl IntoElement {
    div()
        .relative()
        .min_h(tokens::TAB_ROW_MIN_H)
        .pt(tokens::TAB_ROW_PADDING.top)
        .pr(tokens::TAB_ROW_PADDING.right)
        .pb(tokens::TAB_ROW_PADDING.bottom)
        .pl(tokens::TAB_ROW_PADDING.left)
        .mx_1()
        .flex()
        .items_center()
        .gap(tokens::TAB_ROW_SPACING)
        .rounded(px(6.))
        .when(selected, |row| row.bg(theme.sidebar_accent))
        .when_some(group, |row, color| {
            row.child(div().absolute().left_0().top(px(4.)).bottom(px(4.)).w(tokens::GROUP_BAR_W).rounded(px(2.)).bg(color))
        })
        .child(icon(glyph, tokens::FAVICON))
        .child(div().flex_1().truncate().child(title))
        .child(
            div()
                .size(tokens::TAB_CLOSE)
                .flex()
                .items_center()
                .justify_center()
                .opacity(tokens::TAB_ROW_CLOSE_OPACITY)
                .child(icon(VsIcon::Close, tokens::FAVICON)),
        )
}

fn chip(label: Option<&'static str>, color: Hsla, dark: bool) -> AnyElement {
    match label {
        Some(label) => div()
            .px(tokens::GROUP_CHIP_PADDING.left)
            .py(tokens::GROUP_CHIP_PADDING.top)
            .rounded(tokens::GROUP_CHIP_RADIUS)
            .bg(color)
            .text_color(colors::group_chip_text(dark))
            .text_size(tokens::TEXT_SMALL)
            .font_weight(FontWeight::BOLD)
            .child(label)
            .into_any_element(),
        None => div().size(tokens::GROUP_DOT_UNTITLED).rounded_full().bg(color).into_any_element(),
    }
}

fn sidebar(theme: &Theme) -> impl IntoElement {
    let dark = theme.is_dark();
    let research = colors::group(GroupColor::Blue, dark);
    div()
        .w(tokens::sidebar_width(tokens::WINDOW_DEFAULT.width).expect("the default window has a sidebar"))
        .flex_none()
        .flex()
        .flex_col()
        .gap_0p5()
        .py_1()
        .bg(theme.sidebar)
        .text_color(theme.sidebar_foreground)
        .border_r_1()
        .border_color(theme.border)
        .child(tab_row(VsIcon::Globe, "Theme gallery", true, None, theme))
        .child(tab_row(VsIcon::Volume, "Playing audio", false, None, theme))
        .child(
            div()
                .mt(tokens::GROUP_HEADER_MARGIN.top)
                .mx(tokens::GROUP_HEADER_MARGIN.left)
                .min_h(tokens::GROUP_HEADER_MIN_H)
                .pl(tokens::GROUP_HEADER_PADDING.left)
                .flex()
                .items_center()
                .gap_2()
                .child(chip(Some("Research"), research, dark))
                .child(icon(VsIcon::ChevronDown, tokens::FAVICON)),
        )
        .child(tab_row(VsIcon::Bookmark, "Grouped tab", false, Some(research), theme))
        .child(tab_row(VsIcon::Pin, "Pinned tab", false, Some(research), theme))
        .child(div().flex_1())
        .child(
            div()
                .m(tokens::SIDEBAR_BUTTON_MARGIN)
                .h(tokens::SIDEBAR_BUTTON_H)
                .flex()
                .items_center()
                .justify_center()
                .gap_2()
                .rounded(px(6.))
                .bg(theme.list_hover)
                .child(icon(VsIcon::NewTab, tokens::FAVICON))
                .child("New tab"),
        )
}

fn tab_strip(theme: &Theme) -> impl IntoElement {
    let tab = |title: &'static str, active: bool| {
        div()
            .h(tokens::TAB_H)
            .min_w(tokens::TAB_MIN_W)
            .w(px(180.))
            .max_w(tokens::TAB_MAX_W)
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(6.))
            .text_color(if active { theme.tab_active_foreground } else { theme.tab_foreground })
            .when(active, |t| t.bg(theme.tab_active))
            .child(icon(VsIcon::Globe, tokens::FAVICON))
            .child(div().flex_1().truncate().child(title))
            .child(icon(VsIcon::Close, tokens::FAVICON))
    };
    div()
        .h(tokens::TAB_STRIP_H)
        .flex_none()
        .flex()
        .items_center()
        .gap(tokens::TAB_GAP)
        .px_1()
        .bg(theme.tab_bar)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .w(tokens::TAB_PINNED_W)
                .h(tokens::TAB_H)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(VsIcon::Pin, tokens::FAVICON)),
        )
        .child(tab("Active tab", true))
        .child(tab("Background tab", false))
        .child(button(VsIcon::NewTab, theme))
}

fn private_bar() -> impl IntoElement {
    let private = theme::private_chrome();
    let half = |background: Hsla, label: &'static str| {
        div().flex_1().h_full().flex().items_center().gap_2().px_3().bg(background).child(icon(VsIcon::Private, tokens::TOOLBAR_ICON)).child(label)
    };
    div()
        .h(tokens::BANNER_H)
        .flex_none()
        .flex()
        .text_color(private.foreground)
        .border_b_1()
        .border_color(private.border)
        .child(half(private.background, "Private window"))
        .child(half(private.inactive_background, "Private window, inactive"))
}

fn bookmarks_bar(theme: &Theme) -> impl IntoElement {
    let item = |glyph: VsIcon, label: &'static str| {
        div()
            .min_h(tokens::BOOKMARK_BUTTON_MIN_H)
            .px(tokens::BOOKMARK_BUTTON_PADDING.left)
            .flex()
            .items_center()
            .gap_1()
            .rounded(px(6.))
            .child(icon(glyph, tokens::FAVICON))
            .child(label)
    };
    div()
        .min_h(tokens::BOOKMARKS_BAR_MIN_H)
        .flex_none()
        .px(tokens::BOOKMARKS_BAR_PADDING.left)
        .py(tokens::BOOKMARKS_BAR_PADDING.top)
        .flex()
        .items_center()
        .gap(tokens::BOOKMARK_SPACING)
        .bg(theme.title_bar)
        .border_b_1()
        .border_color(theme.border)
        .child(item(VsIcon::Folder, "Reading"))
        .child(item(VsIcon::Globe, "Example"))
        .child(div().h(tokens::BOOKMARK_BUTTON_MIN_H).w(tokens::BOOKMARK_DROP_LINE).bg(theme.primary))
        .child(item(VsIcon::Globe, "Docs"))
}

fn icon_grid(marks: &Marks, theme: &Theme) -> impl IntoElement {
    div().flex().flex_wrap().gap_1().children(VsIcon::ALL.iter().enumerate().map(|(i, &glyph)| {
        let name = match glyph {
            VsIcon::File(kind) => format!("File {kind:?}"),
            other => format!("{other:?}"),
        };
        div()
            .w(px(82.))
            .h(px(42.))
            .whitespace_nowrap()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .child(div().relative().size(px(16.)).child(icon(glyph, px(16.))).child(marks.track(Mark::Icon(i))))
            .child(div().text_size(px(10.)).text_color(theme.muted_foreground).child(SharedString::from(name)))
    }))
}

fn palette(theme: &Theme) -> impl IntoElement {
    let keys: [(&str, Hsla); 20] = [
        ("background", theme.background),
        ("foreground", theme.foreground),
        ("title_bar", theme.title_bar),
        ("sidebar", theme.sidebar),
        ("sidebar.accent", theme.sidebar_accent),
        ("tab_bar", theme.tab_bar),
        ("tab.active", theme.tab_active),
        ("popover", theme.popover),
        ("primary", theme.primary),
        ("link", theme.link),
        ("muted.fg", theme.muted_foreground),
        ("border", theme.border),
        ("input.border", theme.input),
        ("ring", theme.ring),
        ("danger", theme.danger),
        ("success", theme.success),
        ("warning", theme.warning),
        ("list.hover", theme.list_hover),
        ("overlay", theme.overlay),
        ("window.border", theme.window_border),
    ];
    div().flex().flex_wrap().gap_2().children(keys.map(|(name, color)| {
        div()
            .w(px(84.))
            .flex()
            .flex_col()
            .gap_0p5()
            .child(div().h(px(26.)).rounded(px(6.)).border_1().border_color(theme.border).bg(color))
            .child(div().text_size(tokens::TEXT_CAPTION).child(name))
            .child(div().text_size(px(10.)).text_color(theme.muted_foreground).child(format!("#{:08x}", colors::to_hex(color))))
    }))
}

fn groups(theme: &Theme) -> impl IntoElement {
    let dark = theme.is_dark();
    div()
        .flex()
        .flex_wrap()
        .gap_1()
        .children(GroupColor::ALL.map(|c| chip(Some(c.label()), colors::group(c, dark), dark)))
        .child(chip(None, colors::group(GroupColor::Grey, dark), dark))
}

fn profiles(marks: &Marks, picture: &Path) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().flex().gap_1().children(ProfileColor::ALL.map(|c| Avatar::new(c.label(), c, None, false))))
        .child(
            div()
                .flex()
                .gap_3()
                .child(Avatar::new("Work", ProfileColor::Purple, None, true))
                .child(
                    div()
                        .relative()
                        .size(tokens::AVATAR_LARGE)
                        .child(Avatar::new("Picture", ProfileColor::Blue, Some(picture.to_owned()), true))
                        .child(marks.track(Mark::Picture)),
                )
                .child(Avatar::new("Broken picture", ProfileColor::Green, Some(picture.with_extension("missing")), true)),
        )
}

fn type_samples(theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .child(div().text_size(tokens::TEXT_TITLE).child("Who's browsing?"))
        .child(div().text_size(tokens::TEXT_HEADING).font_weight(FontWeight::SEMIBOLD).child("Heading, 15 semibold"))
        .child(div().child("Body text, 14. ").child(div().text_color(theme.link).child("A link")))
        .child(div().text_size(tokens::TEXT_CAPTION).text_color(theme.muted_foreground).child("Caption, 12, dim"))
}

fn overlays(theme: &Theme) -> impl IntoElement {
    let row = |glyph: VsIcon, label: &'static str, hover: bool| {
        div()
            .min_h(tokens::MENU_ROW_MIN_H)
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(4.))
            .when(hover, |r| r.bg(theme.list_hover))
            .child(icon(glyph, tokens::FAVICON))
            .child(label)
    };
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .p_1()
                .rounded(theme.radius_lg)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .shadow_md()
                .child(row(VsIcon::Settings, "Settings", false))
                .child(row(VsIcon::History, "History", true))
                .child(row(VsIcon::Download, "Downloads", false)),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .px(tokens::TOAST_PADDING.left)
                        .py(tokens::TOAST_PADDING.top)
                        .rounded(tokens::TOAST_RADIUS)
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.border)
                        .child("Toast"),
                )
                .child(
                    div()
                        .px(tokens::FULLSCREEN_NOTICE_PADDING.left)
                        .py(tokens::FULLSCREEN_NOTICE_PADDING.top)
                        .rounded(tokens::FULLSCREEN_NOTICE_RADIUS)
                        .bg(theme.overlay)
                        .text_color(gpui::white())
                        .child("Fullscreen"),
                )
                .child(
                    div()
                        .min_w(tokens::EXTENSION_BADGE_MIN.width)
                        .min_h(tokens::EXTENSION_BADGE_MIN.height)
                        .px(tokens::EXTENSION_BADGE_PADDING.left)
                        .rounded(tokens::EXTENSION_BADGE_RADIUS)
                        .bg(theme.accent)
                        .text_color(theme.accent_foreground)
                        .text_size(tokens::EXTENSION_BADGE_TEXT)
                        .font_weight(FontWeight::BOLD)
                        .child("12"),
                ),
        )
}

/// Light, then dark: apply the pref and capture until the capture passes [`check`] (the window
/// repaints, and the picture loads, a moment later), then write the PNG. Then the two must differ.
async fn capture_both(window: WindowHandle<Gallery>, marks: &Marks, dir: &Path, cx: &mut gpui::AsyncApp) -> Result<(), String> {
    let mut shots = Vec::new();
    for (pref, name, background) in
        [(prefs::Theme::Light, "light", [0xfa, 0xfa, 0xfb]), (prefs::Theme::Dark, "dark", [0x22, 0x22, 0x26])]
    {
        cx.update(|cx| theme::apply(pref, cx));
        let started = Instant::now();
        let (shot, scale) = loop {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let (shot, scale) = window
                .update(cx, |_, window, _| Ok::<_, String>((capture(window)?, window.scale_factor())))
                .map_err(|e| e.to_string())??;
            match check(&shot, scale, marks, background) {
                Ok(()) => break (shot, scale),
                Err(e) if started.elapsed() > SETTLE => {
                    let path = dir.join(format!("theme-{name}-failed.png"));
                    _ = shot.save(&path);
                    return Err(format!("{name}: {e} (last capture: {})", path.display()));
                }
                Err(_) => {}
            }
        };
        let path = dir.join(format!("theme-{name}.png"));
        shot.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("theme_gallery: {name}: {}x{} at scale {scale} -> {}", shot.width(), shot.height(), path.display());
        shots.push((name, path));
    }
    let [(_, light), (_, dark)] = &shots[..] else { unreachable!() };
    let (light, dark) = (read(light)?, read(dark)?);
    if light.dimensions() != dark.dimensions() {
        return Err(format!("the captures differ in size: {:?} and {:?}", light.dimensions(), dark.dimensions()));
    }
    let differ = light.pixels().zip(dark.pixels()).filter(|(a, b)| a != b).count();
    let share = differ as f64 / f64::from(light.width() * light.height());
    eprintln!("theme_gallery: light and dark differ in {:.1} % of pixels", share * 100.);
    if share < 0.5 {
        return Err(format!("light and dark differ in only {:.1} % of pixels", share * 100.));
    }
    Ok(())
}

/// One capture: the mode's window background, not blank, ink in every icon, the picture avatar
/// shown.
fn check(shot: &RgbaImage, scale: f32, marks: &Marks, background: [u8; 3]) -> Result<(), String> {
    let plain = average(shot, marks.get(Mark::Background)?, scale);
    if !near(plain, background, 3) {
        return Err(format!("the window background is {plain:?}, not {background:?}"));
    }
    let colours: std::collections::HashSet<[u8; 4]> = shot.pixels().map(|p| p.0).collect();
    if colours.len() < 64 {
        return Err(format!("only {} distinct colours; the capture is blank", colours.len()));
    }
    let least_ink = (6. * scale * scale) as usize;
    for (i, glyph) in VsIcon::ALL.iter().enumerate() {
        let ink = count(shot, marks.get(Mark::Icon(i))?, scale, |p| !near(p, background, 40));
        if ink < least_ink {
            return Err(format!("{glyph:?} drew {ink} pixels"));
        }
    }
    let picture = marks.get(Mark::Picture)?;
    let red = count(shot, picture, scale, |[r, g, b]| r > 200 && g < 230 && b < 230 && !near([r, g, b], background, 40));
    let area = (picture.size.width.as_f32() * scale * picture.size.height.as_f32() * scale) as usize;
    if red * 2 < area {
        return Err(format!("the picture avatar shows in {red} of {area} pixels of {picture:?}"));
    }
    Ok(())
}

fn read(path: &Path) -> Result<RgbaImage, String> {
    Ok(image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8())
}

fn near(a: [u8; 3], b: [u8; 3], slack: u8) -> bool {
    a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= slack)
}

/// The physical pixels inside `at`.
fn pixels(shot: &RgbaImage, at: Bounds<Pixels>, scale: f32) -> impl Iterator<Item = [u8; 3]> + '_ {
    let x0 = (at.origin.x.as_f32() * scale).round() as u32;
    let y0 = (at.origin.y.as_f32() * scale).round() as u32;
    let x1 = ((at.origin.x + at.size.width).as_f32() * scale).round() as u32;
    let y1 = ((at.origin.y + at.size.height).as_f32() * scale).round() as u32;
    (y0..y1.min(shot.height()))
        .flat_map(move |y| (x0..x1.min(shot.width())).map(move |x| (x, y)))
        .map(|(x, y)| {
            let [r, g, b, _] = shot.get_pixel(x, y).0;
            [r, g, b]
        })
}

fn count(shot: &RgbaImage, at: Bounds<Pixels>, scale: f32, test: impl Fn([u8; 3]) -> bool) -> usize {
    pixels(shot, at, scale).filter(|&p| test(p)).count()
}

fn average(shot: &RgbaImage, at: Bounds<Pixels>, scale: f32) -> [u8; 3] {
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for p in pixels(shot, at, scale) {
        for (s, c) in sum.iter_mut().zip(p) {
            *s += u64::from(c);
        }
        n += 1;
    }
    sum.map(|s| (s / n.max(1)) as u8)
}

/// The window's own pixels, never the screen's.
fn capture(window: &Window) -> Result<RgbaImage, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?.as_raw();
    match handle {
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(h) => x11::capture(h.window.get()),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(h) => x11::capture(h.window as u32),
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => win32::capture(h.hwnd.get() as _),
        other => Err(format!("no capture for {other:?}")),
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use image::RgbaImage;
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat};

    /// `GetImage` of `window` on the private display `DISPLAY` names (the guard checked it).
    pub fn capture(window: u32) -> Result<RgbaImage, String> {
        let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let geometry = conn.get_geometry(window).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let (w, h) = (geometry.width, geometry.height);
        let image = conn
            .get_image(ImageFormat::Z_PIXMAP, window, 0, 0, w, h, !0)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        if image.data.len() != usize::from(w) * usize::from(h) * 4 || !conn.setup().image_byte_order.eq(&x11rb::protocol::xproto::ImageOrder::LSB_FIRST) {
            return Err(format!("unexpected image layout: {} bytes for {w}x{h}, depth {}", image.data.len(), image.depth));
        }
        let rgba = image.data.as_chunks::<4>().0.iter().flat_map(|&[b, g, r, _]| [r, g, b, 255]).collect();
        RgbaImage::from_raw(u32::from(w), u32::from(h), rgba).ok_or("bad image size".into())
    }
}

#[cfg(windows)]
mod win32 {
    use image::RgbaImage;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject,
        GdiFlush, HDC, SelectObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

    // windows-sys files it under `Win32_Storage_Xps`, which this crate does not enable.
    #[link(name = "user32")]
    unsafe extern "system" {
        fn PrintWindow(hwnd: HWND, hdc: HDC, flags: u32) -> i32;
    }
    const PW_CLIENTONLY: u32 = 1;
    /// DWM's composition of the window, which a DirectComposition window needs.
    const PW_RENDERFULLCONTENT: u32 = 2;

    /// `PrintWindow` of this process's own `hwnd`'s client area.
    pub fn capture(hwnd: HWND) -> Result<RgbaImage, String> {
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
            return Err("GetClientRect failed".into());
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 {
            return Err(format!("an empty client area, {w}x{h}"));
        }
        let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..unsafe { std::mem::zeroed() }
        };
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            if bitmap.is_null() {
                DeleteDC(dc);
                return Err("CreateDIBSection failed".into());
            }
            let old = SelectObject(dc, bitmap);
            let printed = PrintWindow(hwnd, dc, PW_CLIENTONLY | PW_RENDERFULLCONTENT);
            GdiFlush();
            let bgra = std::slice::from_raw_parts(bits.cast::<u8>(), (w * h * 4) as usize);
            let rgba: Vec<u8> = bgra.as_chunks::<4>().0.iter().flat_map(|&[b, g, r, _]| [r, g, b, 255]).collect();
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
            if printed == 0 {
                return Err("PrintWindow failed".into());
            }
            RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or("bad image size".into())
        }
    }
}
