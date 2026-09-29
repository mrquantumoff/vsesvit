//! The window's side of site permissions: the prompt bubble under the site-info button, the
//! Permissions section of the site-info popup, and the selected tab's in-use indicators (the
//! address bar's capture button and the screen sharing bar).
//!
//! One prompt shows at a time, for the selected tab; whoever takes it out of `PermissionUi`
//! decides what became of it. As in Chrome, a request is never denied because the user did
//! something else: a prompt that goes because its tab lost the selection, or because the
//! site-info popup opened, is withdrawn and comes back; only Esc, "Not now" or a click elsewhere
//! that leaves its tab selected dismiss it.
//!
//! The prompt takes the keyboard focus on its heading, never on an answer, and ignores answers
//! for a moment after it shows or changes, so a key press or click meant for something else
//! cannot answer it.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use vsesvit_core::permissions::{Answer, Origin, Permission, Prompt, Setting};
use windows_core::{Interface, Result};

use super::BrowserWindow;
use super::wiring::{click, with};
use crate::bindings::*;
use crate::permissions::{self, Choice, Row};
use crate::tab::{Tab, TabId};
use crate::{connection, exec, xaml};

/// Answers this soon after a prompt shows or changes were aimed at something else (Chrome's
/// input guard).
const INPUT_GUARD: Duration = Duration::from_millis(500);

/// A click that closes the prompt may be the press of a tab switch or of the site-info
/// button; both take the prompt before this passes.
const CLOSE_GRACE: Duration = Duration::from_millis(250);

struct ShownPrompt {
    /// Every shown prompt has its own, so a late event of a closed one is told apart.
    id: u64,
    tab: TabId,
    flyout: Flyout,
    /// When it showed, or last changed.
    since: Instant,
}

#[derive(Default)]
pub(super) struct PermissionUi {
    prompt: RefCell<Option<ShownPrompt>>,
    prompts_shown: Cell<u64>,
    /// The rows the open site-info popup lists.
    rows: RefCell<Vec<Row>>,
}

fn hide(flyout: &Flyout) {
    let _ = flyout.cast::<FlyoutBase>().and_then(|f| f.Hide());
}

fn prompt_markup(prompt: &Prompt) -> String {
    let buttons: String = prompt
        .answers
        .iter()
        .enumerate()
        .map(|(index, answer)| {
            let style = if index == 0 {
                r#" Style="{StaticResource AccentButtonStyle}""#
            } else {
                ""
            };
            format!(
                r#"<Button x:Name="Answer{answer:?}" Content="{}" HorizontalAlignment="Stretch"{style}/>"#,
                xaml::escape(answer.label())
            )
        })
        .collect();
    // The heading is the first thing that takes the focus, so the flyout never puts it on an
    // answer.
    format!(
        r#"<StackPanel {{ns}} Width="320" Spacing="12">
             <ContentControl x:Name="PromptFocus" IsTabStop="True" HorizontalContentAlignment="Stretch">
               <TextBlock x:Name="PromptHeading" Text="{}" TextWrapping="Wrap" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
             </ContentControl>
             <TextBlock Text="{}" TextWrapping="Wrap"/>
             <StackPanel Spacing="8">{buttons}</StackPanel>
           </StackPanel>"#,
        xaml::escape(&prompt.heading),
        xaml::escape(&prompt.body)
    )
}

fn section_markup(rows: &[Row], stored: bool) -> String {
    let rows: String = rows
        .iter()
        .map(|row| {
            let key = format!("{:?}", row.permission);
            let selected = row.choices.iter().position(|c| *c == row.current).unwrap_or(0);
            let items: String = row
                .choices
                .iter()
                .map(|c| format!(r#"<ComboBoxItem Content="{}"/>"#, c.label()))
                .collect();
            let (in_use, stop) = if row.live {
                (
                    r#"<TextBlock Text="In use" Style="{StaticResource CaptionTextBlockStyle}"
                                  Foreground="{ThemeResource TextFillColorSecondaryBrush}"/>"#
                        .to_owned(),
                    format!(
                        r#"<Button x:Name="PermissionStop{key}" Grid.Column="2" Content="Stop" VerticalAlignment="Center"/>"#
                    ),
                )
            } else {
                (String::new(), String::new())
            };
            format!(
                r#"<Grid x:Name="PermissionRow{key}" ColumnSpacing="8">
                     <Grid.ColumnDefinitions>
                       <ColumnDefinition Width="Auto"/><ColumnDefinition Width="*"/>
                       <ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/>
                     </Grid.ColumnDefinitions>
                     <FontIcon Glyph="{glyph}" FontSize="16" VerticalAlignment="Center"/>
                     <StackPanel Grid.Column="1" VerticalAlignment="Center">
                       <TextBlock Text="{label}" TextTrimming="CharacterEllipsis"/>
                       {in_use}
                     </StackPanel>
                     {stop}
                     <ComboBox x:Name="PermissionChoice{key}" Grid.Column="3" Width="168" SelectedIndex="{selected}"
                               AutomationProperties.Name="{label}">{items}</ComboBox>
                   </Grid>"#,
                glyph = permissions::glyph(row.permission),
                label = row.permission.label(),
            )
        })
        .collect();
    let reset = if stored {
        r#"<Button x:Name="ResetPermissions" Content="Reset permissions"/>"#
    } else {
        ""
    };
    format!(
        r#"<StackPanel {{ns}} Spacing="8">
             <Border Height="1" Margin="0,4" Background="{{ThemeResource DividerStrokeColorDefaultBrush}}"/>
             <TextBlock Text="Permissions" Style="{{StaticResource BodyStrongTextBlockStyle}}"/>
             {rows}
             {reset}
           </StackPanel>"#
    )
}

impl BrowserWindow {
    pub(super) fn wire_permissions(&self) -> Result<()> {
        let w = self.me.clone();
        click(&self.ui.capture_button, move || {
            with(&w, |w| {
                if let Err(e) = w.show_connection() {
                    log::warn!("connection popup: {e}");
                }
            });
        })?;
        let w = self.me.clone();
        click(&self.ui.share_stop, move || {
            with(&w, |w| {
                if let Some(tab) = w.active_tab() {
                    tab.stop_capture(Permission::ScreenShare);
                }
            });
        })
    }

    /// Shows the prompt of the selected tab's first waiting requests, or updates the shown one
    /// when a request joined it. A prompt of another tab goes back to waiting until its tab is
    /// selected again, and none shows over the site-info popup.
    pub fn show_permission_prompt(&self) {
        let active = self.active_tab();
        let shown = self.permissions.prompt.borrow().as_ref().map(|p| p.tab);
        if shown.is_some() && shown != active.as_ref().map(|t| t.id) {
            self.withdraw_prompt();
        }
        let (Some(tab), Some(browser)) = (active, self.browser()) else {
            return;
        };
        if self.connection_popup().is_some() {
            return;
        }
        let next = tab.permissions().next_prompt(&browser);
        tab.watch_capture();
        let shown = self
            .permissions
            .prompt
            .borrow()
            .as_ref()
            .map(|p| (p.id, p.flyout.clone()));
        let result = match (next, shown) {
            (None, None) | (Some((_, false)), Some(_)) => Ok(()),
            (None, Some(_)) => {
                if let Some(shown) = self.permissions.prompt.take() {
                    hide(&shown.flyout);
                }
                Ok(())
            }
            (Some((prompt, _)), Some((id, flyout))) => self
                .prompt_content(id, &prompt)
                .and_then(|content| flyout.SetContent(&content.cast::<UIElement>()?))
                .map(|()| {
                    if let Some(shown) = self.permissions.prompt.borrow_mut().as_mut() {
                        shown.since = Instant::now();
                    }
                }),
            (Some((prompt, _)), None) => self.open_prompt(tab.id, &prompt).map(|shown| {
                *self.permissions.prompt.borrow_mut() = Some(shown);
            }),
        };
        if let Err(e) = result {
            log::warn!("permission prompt: {e}");
            if let Some(shown) = self.permissions.prompt.take() {
                hide(&shown.flyout);
            }
            tab.permissions().answer(&browser, Answer::Dismiss);
        }
    }

    /// The prompt's words and buttons; a button answers prompt `id`.
    fn prompt_content(&self, id: u64, prompt: &Prompt) -> Result<FrameworkElement> {
        let content: FrameworkElement = xaml::load(&prompt_markup(prompt))?;
        for &answer in &prompt.answers {
            let button: Button = xaml::find(&content, &format!("Answer{answer:?}"))?;
            let w = self.me.clone();
            click(&button, move || with(&w, |w| w.prompt_button(id, answer)))?;
        }
        Ok(content)
    }

    fn open_prompt(&self, tab: TabId, prompt: &Prompt) -> Result<ShownPrompt> {
        let id = self.permissions.prompts_shown.get() + 1;
        self.permissions.prompts_shown.set(id);
        let content = self.prompt_content(id, prompt)?;
        self.close_suggestions();
        let flyout = connection::flyout(&content)?;
        let base = flyout.cast::<FlyoutBase>()?;
        // Clicks on the tabs and the address bar reach them while the prompt is open.
        base.SetOverlayInputPassThroughElement(&self.ui.root.cast::<DependencyObject>()?)?;
        let w = self.me.clone();
        base.Closed(move |_, _| {
            let (w, closed) = (w.clone(), Instant::now());
            exec::spawn(async move {
                exec::sleep(CLOSE_GRACE).await;
                with(&w, |w| w.prompt_closed(id, closed));
            });
        })?
        .forget();
        let options = FlyoutShowOptions::new()?;
        options.SetShowMode(if self.is_foreground() {
            FlyoutShowMode::Standard
        } else {
            FlyoutShowMode::Transient
        })?;
        base.ShowAtWithOptions(&self.ui.site_button.cast::<FrameworkElement>()?, &options)?;
        Ok(ShownPrompt {
            id,
            tab,
            flyout,
            since: Instant::now(),
        })
    }

    /// An answer button of prompt `id`; ignored within the input guard.
    fn prompt_button(&self, id: u64, answer: Answer) {
        let guarded = match self.permissions.prompt.borrow().as_ref() {
            Some(shown) if shown.id == id => shown.since.elapsed() < INPUT_GUARD,
            _ => return,
        };
        if guarded {
            log::info!("permission prompt: {answer:?} ignored, too soon after the prompt showed");
            return;
        }
        self.settle_prompt(answer);
    }

    /// Prompt `id` closed at `closed` without a button, and the grace passed: Esc or a click
    /// elsewhere dismiss it if its tab is still the one shown. A close within the input guard
    /// withdraws it instead, to show again.
    fn prompt_closed(&self, id: u64, closed: Instant) {
        let (tab, since) = match self.permissions.prompt.borrow().as_ref() {
            Some(shown) if shown.id == id => (shown.tab, shown.since),
            _ => return,
        };
        let selected = self.active_tab().is_some_and(|t| t.id == tab);
        if selected && closed.duration_since(since) >= INPUT_GUARD {
            self.settle_prompt(Answer::Dismiss);
        } else {
            self.withdraw_prompt();
            self.show_next_prompt();
        }
    }

    fn settle_prompt(&self, answer: Answer) {
        let Some(shown) = self.permissions.prompt.take() else {
            return;
        };
        hide(&shown.flyout);
        if let (Some(tab), Some(browser)) = (self.tab(shown.tab), self.browser()) {
            log::info!("tab {}: permission prompt answered {answer:?}", tab.id);
            tab.permissions().answer(&browser, answer);
            permissions::settings_changed(&browser);
        }
        self.show_next_prompt();
    }

    /// Hides the prompt and keeps its requests waiting.
    fn withdraw_prompt(&self) {
        if let Some(shown) = self.permissions.prompt.take() {
            if let Some(tab) = self.tab(shown.tab) {
                tab.permissions().prompt_hidden();
            }
            hide(&shown.flyout);
        }
    }

    /// The site-info popup is about to show as `flyout`: the prompt makes way for it and
    /// comes back when it closes.
    pub(super) fn prompt_yields_to(&self, flyout: &Flyout) -> Result<()> {
        self.withdraw_prompt();
        let w = self.me.clone();
        flyout
            .cast::<FlyoutBase>()?
            .Closed(move |_, _| with(&w, BrowserWindow::show_next_prompt))?
            .forget();
        Ok(())
    }

    /// The tab's shown prompt no longer has requests (it navigated away).
    pub fn permission_prompt_gone(&self, tab: TabId) {
        let shown = self.permissions.prompt.borrow().as_ref().map(|p| p.tab);
        if shown != Some(tab) {
            return;
        }
        if let Some(shown) = self.permissions.prompt.take() {
            hide(&shown.flyout);
        }
        self.show_next_prompt();
    }

    /// On the next turn: the flyout that just closed may still be on its way out.
    pub(crate) fn show_next_prompt(&self) {
        let w = self.me.clone();
        exec::spawn(async move { with(&w, BrowserWindow::show_permission_prompt) });
    }

    /// The capture button and the screen sharing bar, for the selected tab; and the open
    /// site-info popup's Permissions section, when what it would list changed.
    pub(super) fn show_permissions_state(&self) {
        let tab = self.active_tab();
        let capturing = tab
            .as_ref()
            .map(|t| t.permissions().capturing())
            .unwrap_or_default();
        let description = capturing.description();
        let _ = xaml::set_visible(&self.ui.capture_button, description.is_some());
        if let Some(text) = &description {
            let glyph = if capturing.camera {
                "\u{E714}"
            } else if capturing.microphone {
                "\u{E720}"
            } else {
                "\u{E7F4}"
            };
            let _ = self.ui.capture_glyph.SetGlyph(glyph);
            let _ = xaml::boxed(text)
                .and_then(|tip| ToolTipService::SetToolTip(&self.ui.capture_button, &tip));
        }
        if capturing.screen {
            let host = tab
                .as_ref()
                .and_then(|t| t.origin())
                .map_or_else(|| "this page".to_owned(), |o| o.host_for_display());
            let _ = self
                .ui
                .share_bar
                .SetTitle(&format!("Sharing your screen with {host}"));
        }
        if capturing.screen && !self.ui.share_bar.IsOpen().unwrap_or(false) {
            self.close_suggestions();
        }
        let _ = self.ui.share_bar.SetIsOpen(capturing.screen);
        if let Some(tab) = tab
            && self.connection_popup().is_some()
            && self.site_rows(&tab) != *self.permissions.rows.borrow()
            && let Err(e) = self.fill_site_permissions(&tab)
        {
            log::warn!("site permissions: {e}");
        }
    }

    fn site_rows(&self, tab: &Tab) -> Vec<Row> {
        let origin = tab.origin();
        let stored = match (&origin, self.browser()) {
            (Some(origin), Some(browser)) => {
                browser.core(|p| p.site_permissions().for_site(origin))
            }
            _ => Vec::new(),
        };
        permissions::site_rows(
            origin.is_some(),
            &stored,
            &tab.permissions().grants(),
            tab.permissions().capturing(),
        )
    }

    /// Fills the open site-info popup's Permissions section for `tab`.
    pub(super) fn fill_site_permissions(&self, tab: &Rc<Tab>) -> Result<()> {
        let Some(popup) = self.connection_popup() else {
            return Ok(());
        };
        let panel: Panel = xaml::find(&popup, "SitePermissions")?;
        let rows = self.site_rows(tab);
        let children = panel.Children()?;
        children.Clear()?;
        *self.permissions.rows.borrow_mut() = rows.clone();
        xaml::set_visible(&panel, !rows.is_empty())?;
        if rows.is_empty() {
            return Ok(());
        }
        let origin = tab.origin();
        let stored = rows
            .iter()
            .any(|r| matches!(r.current, Choice::Allow | Choice::Block));
        let section: FrameworkElement = xaml::load(&section_markup(&rows, stored))?;
        children.Append(&section.cast::<UIElement>()?)?;
        for row in rows {
            let key = format!("{:?}", row.permission);
            let choice: ComboBox = xaml::find(&section, &format!("PermissionChoice{key}"))?;
            let selector = choice.cast::<Selector>()?;
            let (source, w, t) = (selector.clone(), self.me.clone(), Rc::downgrade(tab));
            let (origin, current, choices) = (origin.clone(), row.current, row.choices.clone());
            selector
                .SelectionChanged(move |_, _| {
                    let picked = source
                        .SelectedIndex()
                        .ok()
                        .and_then(|i| usize::try_from(i).ok())
                        .and_then(|i| choices.get(i).copied());
                    // A combo box raises this for its initial selection too.
                    if let Some(picked) = picked.filter(|p| *p != current) {
                        site_choice(&w, &t, origin.as_ref(), row.permission, picked);
                    }
                })?
                .forget();
            if row.live {
                let stop: Button = xaml::find(&section, &format!("PermissionStop{key}"))?;
                let t = Rc::downgrade(tab);
                click(&stop, move || {
                    if let Some(tab) = t.upgrade() {
                        tab.stop_capture(row.permission);
                    }
                })?;
            }
        }
        if stored && let Some(origin) = origin {
            let reset: Button = xaml::find(&section, "ResetPermissions")?;
            let (w, t) = (self.me.clone(), Rc::downgrade(tab));
            click(&reset, move || reset_site(&w, &t, &origin))?;
        }
        Ok(())
    }

    fn refill_site_permissions(window: &Weak<BrowserWindow>, tab: &Weak<Tab>) {
        let (window, tab) = (window.clone(), tab.clone());
        // Not from inside the event of a control the refill replaces.
        exec::spawn(async move {
            if let (Some(window), Some(tab)) = (window.upgrade(), tab.upgrade())
                && let Err(e) = window.fill_site_permissions(&tab)
            {
                log::warn!("site permissions: {e}");
            }
        });
    }
}

/// A choice in the site-info popup. Block (or back to Ask) also ends a grant this tab had,
/// and Block ends the capture it governs.
fn site_choice(
    window: &Weak<BrowserWindow>,
    tab: &Weak<Tab>,
    origin: Option<&Origin>,
    permission: Permission,
    choice: Choice,
) {
    let (Some(w), Some(t)) = (window.upgrade(), tab.upgrade()) else {
        return;
    };
    let Some(browser) = w.browser() else { return };
    if choice == Choice::AllowedThisTime {
        return;
    }
    if let Some(origin) = origin
        && let Err(e) = browser.core(|p| {
            p.site_permissions()
                .set(origin, permission, choice.setting())
        })
    {
        log::warn!("site permission {permission:?}: {e}");
    }
    if choice != Choice::Allow {
        t.permissions().revoke(permission);
    }
    if origin.is_none() && choice.setting() == Some(Setting::Block) {
        t.stop_capture(permission);
    }
    permissions::settings_changed(&browser);
    BrowserWindow::refill_site_permissions(window, tab);
}

/// "Reset permissions": every setting of the site back to Ask, this tab's grants ended, and
/// what the site captured under a setting stopped.
fn reset_site(window: &Weak<BrowserWindow>, tab: &Weak<Tab>, origin: &Origin) {
    let (Some(w), Some(t)) = (window.upgrade(), tab.upgrade()) else {
        return;
    };
    let Some(browser) = w.browser() else { return };
    let stored: Vec<Permission> = browser.core(|p| {
        let mut site = p.site_permissions();
        let stored = site.for_site(origin).into_iter().map(|(p, _)| p).collect();
        if let Err(e) = site.reset_site(origin) {
            log::warn!("reset site permissions: {e}");
        }
        stored
    });
    for permission in t.permissions().grants() {
        t.permissions().revoke(permission);
    }
    permissions::stop_captures(&browser, origin, &stored);
    permissions::settings_changed(&browser);
    BrowserWindow::refill_site_permissions(window, tab);
}

/// What scripted runs read back and press.
impl BrowserWindow {
    /// How many permission prompts this window has shown.
    pub fn permission_prompts_shown(&self) -> u64 {
        self.permissions.prompts_shown.get()
    }

    /// The permission prompt while it is open.
    pub fn permission_prompt(&self) -> Option<FrameworkElement> {
        let prompt = self.permissions.prompt.borrow();
        let flyout = &prompt.as_ref()?.flyout;
        let open = flyout
            .cast::<FlyoutBase>()
            .and_then(|f| f.IsOpen())
            .unwrap_or(false);
        open.then(|| flyout.Content().ok()?.cast().ok()).flatten()
    }

    /// What the address bar's capture button says, while it shows.
    pub fn capture_button_shown(&self) -> Option<String> {
        xaml::is_visible(&self.ui.capture_button)
            .then(|| self.active_tab()?.permissions().capturing().description())
            .flatten()
    }

    /// The screen sharing bar's title, while it is open.
    pub fn share_bar_shown(&self) -> Option<String> {
        let bar = &self.ui.share_bar;
        bar.IsOpen()
            .unwrap_or(false)
            .then(|| bar.Title().ok().map(|t| t.to_string()))
            .flatten()
    }

    /// Closes the prompt's flyout as Esc or a click elsewhere does, leaving what that means to
    /// the prompt.
    pub fn close_prompt_flyout(&self) {
        if let Some(shown) = self.permissions.prompt.borrow().as_ref() {
            hide(&shown.flyout);
        }
    }

    pub fn in_foreground(&self) -> bool {
        self.is_foreground()
    }

    /// The `x:Name` of the element with the keyboard focus.
    pub fn focused_name(&self) -> Option<String> {
        let focused = FocusManager::GetFocusedElementWithRoot(&self.xaml_root().ok()?).ok()?;
        let name = focused.cast::<FrameworkElement>().ok()?.Name().ok()?;
        Some(name.to_string())
    }

    /// The sharing bar's "Stop sharing".
    pub fn stop_sharing_button(&self) -> Button {
        self.ui.share_stop.clone()
    }

    /// Whether the tab's entry in the tab list shows its in-use icon.
    pub fn tab_capture_shown(&self, tab: TabId) -> bool {
        self.strip().capture_shown(tab)
    }
}
