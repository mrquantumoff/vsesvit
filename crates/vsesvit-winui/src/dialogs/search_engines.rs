//! Settings' search engines, on the Search page: the default engine, and every engine with its
//! shortcut and URL, which the editor flyout adds and edits and the row's menu makes the default
//! or deletes, as Chrome's "Manage search engines" does. Core checks the editor's form
//! (`SearchEngines::check`) and refuses to delete the default.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use vsesvit_core::search::{EngineForm, FormField, SearchEngine, SearchEngineId};
use windows_core::{Interface, Result};

use super::on_click;
use crate::bindings::*;
use crate::browser::Browser;
use crate::bookmark_editor::VK_RETURN;
use crate::{exec, xaml};

pub(super) const MARKUP: &str = r#"
        <ComboBox x:Name="SearchEngine" Header="Search engine used in the address bar" MinWidth="320"/>
        <StackPanel Spacing="8">
          <Grid ColumnSpacing="12">
            <Grid.ColumnDefinitions>
              <ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/>
            </Grid.ColumnDefinitions>
            <StackPanel Spacing="2">
              <TextBlock Text="Search engines" Style="{StaticResource BodyStrongTextBlockStyle}"/>
              <TextBlock TextWrapping="Wrap" Style="{StaticResource CaptionTextBlockStyle}"
                         Foreground="{ThemeResource TextFillColorSecondaryBrush}"
                         Text="Type a shortcut and a space in the address bar to search with its engine."/>
            </StackPanel>
            <Button x:Name="SearchEngineAdd" Grid.Column="1" Content="Add" VerticalAlignment="Center"/>
          </Grid>
          <StackPanel x:Name="SearchEngineList" Spacing="4"/>
        </StackPanel>"#;

const EDITOR: &str = r#"<Flyout {ns} Placement="Bottom"/>"#;

const EDITOR_CONTENT: &str = r#"
<StackPanel {ns} Width="380" Spacing="8">
  <TextBlock x:Name="EngineEditorTitle" Margin="0,0,0,4" Style="{StaticResource BodyStrongTextBlockStyle}"/>
  <TextBox x:Name="EngineName" Header="Name"/>
  <TextBlock x:Name="EngineNameProblem" TextWrapping="Wrap" Visibility="Collapsed"
             Style="{StaticResource CaptionTextBlockStyle}" Foreground="{ThemeResource SystemFillColorCriticalBrush}"/>
  <TextBox x:Name="EngineKeyword" Header="Shortcut" IsSpellCheckEnabled="False"/>
  <TextBlock x:Name="EngineKeywordProblem" TextWrapping="Wrap" Visibility="Collapsed"
             Style="{StaticResource CaptionTextBlockStyle}" Foreground="{ThemeResource SystemFillColorCriticalBrush}"/>
  <TextBox x:Name="EngineUrl" Header="URL with %s in place of query" IsSpellCheckEnabled="False"/>
  <TextBlock x:Name="EngineUrlProblem" TextWrapping="Wrap" Visibility="Collapsed"
             Style="{StaticResource CaptionTextBlockStyle}" Foreground="{ThemeResource SystemFillColorCriticalBrush}"/>
  <StackPanel Orientation="Horizontal" Spacing="8" Margin="0,4,0,0" HorizontalAlignment="Right">
    <Button x:Name="EngineSave" Style="{StaticResource AccentButtonStyle}" IsEnabled="False"/>
    <Button x:Name="EngineCancel" Content="Cancel"/>
  </StackPanel>
</StackPanel>"#;

/// The editor's boxes and the problem shown under each, in the form's order.
const FIELDS: [(FormField, &str); 3] = [
    (FormField::Name, "EngineName"),
    (FormField::Keyword, "EngineKeyword"),
    (FormField::Url, "EngineUrl"),
];

/// The `x:Name` of a row's More actions button, from its place in the list.
fn more_name(index: usize) -> String {
    format!("SearchEngineMore{index}")
}

fn row_markup(index: usize, engine: &SearchEngine, is_default: bool) -> String {
    let name = if is_default {
        format!("{} (Default)", engine.name)
    } else {
        engine.name.clone()
    };
    let make_default = if is_default {
        String::new()
    } else {
        format!(r#"<MenuFlyoutItem x:Name="SearchEngineDefault{index}" Text="Make default"/>"#)
    };
    let delete = if is_default {
        String::new()
    } else {
        format!(r#"<MenuFlyoutItem x:Name="SearchEngineDelete{index}" Text="Delete"/>"#)
    };
    format!(
        r#"<Grid {{ns}} MinHeight="52" Padding="12,6,6,6" ColumnSpacing="12" BorderThickness="1"
                CornerRadius="{{ThemeResource ControlCornerRadius}}"
                Background="{{ThemeResource CardBackgroundFillColorDefaultBrush}}"
                BorderBrush="{{ThemeResource CardStrokeColorDefaultBrush}}">
             <Grid.ColumnDefinitions>
               <ColumnDefinition Width="*"/><ColumnDefinition Width="Auto"/><ColumnDefinition Width="Auto"/>
             </Grid.ColumnDefinitions>
             <StackPanel VerticalAlignment="Center">
               <TextBlock x:Name="SearchEngineName{index}" Text="{name}" TextTrimming="CharacterEllipsis"/>
               <TextBlock Text="{url}" TextTrimming="CharacterEllipsis" Style="{{StaticResource CaptionTextBlockStyle}}"
                          Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
             </StackPanel>
             <TextBlock Grid.Column="1" Text="{keyword}" VerticalAlignment="Center"
                        Foreground="{{ThemeResource TextFillColorSecondaryBrush}}"/>
             <Button x:Name="{more}" Grid.Column="2" Width="36" Height="32" Padding="0"
                     Background="Transparent" BorderThickness="0"
                     ToolTipService.ToolTip="More actions" AutomationProperties.Name="More actions for {name}">
               <FontIcon Glyph="&#xE712;" FontSize="14"/>
               <Button.Flyout>
                 <MenuFlyout Placement="BottomEdgeAlignedRight">
                   {make_default}
                   <MenuFlyoutItem x:Name="SearchEngineEdit{index}" Text="Edit"/>
                   {delete}
                 </MenuFlyout>
               </Button.Flyout>
             </Button>
           </Grid>"#,
        name = xaml::escape(&name),
        url = xaml::escape(&EngineForm::of(engine).url),
        keyword = xaml::escape(engine.keyword.as_deref().unwrap_or_default()),
        more = more_name(index),
    )
}

/// Wires the click of the menu item `name` in `row`. A menu item is no button, so `on_click`
/// cannot take it.
fn on_item_click(row: &FrameworkElement, name: &str, handler: impl Fn() + 'static) -> Result<()> {
    xaml::find::<MenuFlyoutItem>(row, name)?
        .Click(move |_, _| handler())?
        .forget();
    Ok(())
}

/// The default's combo box and the list, refilled after every change.
pub(crate) struct Engines {
    combo: ComboBox,
    list: Panel,
    add: FrameworkElement,
    browser: Weak<Browser>,
    /// The engines the combo box lists, in its order.
    listed: RefCell<Vec<SearchEngineId>>,
    /// Set while the combo box is refilled, whose selection then is not the user's.
    filling: Cell<bool>,
    /// The editor while it is open.
    editor: RefCell<Option<Rc<Editor>>>,
    me: Weak<Engines>,
}

pub(super) fn wire(root: &FrameworkElement, browser: &Rc<Browser>) -> Result<Rc<Engines>> {
    let (combo, list, add): (ComboBox, Panel, FrameworkElement) = (
        xaml::find(root, "SearchEngine")?,
        xaml::find(root, "SearchEngineList")?,
        xaml::find(root, "SearchEngineAdd")?,
    );
    let engines = Rc::new_cyclic(|me| Engines {
        combo,
        list,
        add,
        browser: Rc::downgrade(browser),
        listed: RefCell::default(),
        filling: Cell::new(false),
        editor: RefCell::default(),
        me: me.clone(),
    });
    let selector = engines.combo.cast::<Selector>()?;
    let (me, source) = (engines.me.clone(), selector.clone());
    selector
        .SelectionChanged(move |_, _| {
            let Some(me) = me.upgrade() else { return };
            let picked = super::selected_index(&source).and_then(|i| me.listed.borrow().get(i).cloned());
            if let Some(id) = picked.filter(|_| !me.filling.get())
                && me.default_id().as_ref() != Some(&id)
            {
                me.change(|b| b.core(|p| p.search_engines().set_default(&id)));
            }
        })?
        .forget();
    let me = engines.me.clone();
    on_click(&engines.add, move || {
        if let Some(me) = me.upgrade() {
            me.open_editor(None, &me.add);
        }
    })?;
    engines.fill()?;
    Ok(engines)
}

impl Engines {
    fn default_id(&self) -> Option<SearchEngineId> {
        let browser = self.browser.upgrade()?;
        browser.core(|p| p.search_engines().default_engine().ok().map(|e| e.id))
    }

    /// Fills the combo box and the list from core.
    pub(super) fn fill(&self) -> Result<()> {
        let Some(browser) = self.browser.upgrade() else {
            return Ok(());
        };
        let (engines, default) = browser.core(|p| {
            let mut engines = p.search_engines();
            (engines.list().unwrap_or_default(), engines.default_engine().ok().map(|e| e.id))
        });
        self.filling.set(true);
        let filled = self.fill_combo(&engines, default.as_ref());
        self.filling.set(false);
        filled?;
        *self.listed.borrow_mut() = engines.iter().map(|e| e.id.clone()).collect();

        let children = self.list.Children()?;
        children.Clear()?;
        for (index, engine) in engines.iter().enumerate() {
            let is_default = default.as_ref() == Some(&engine.id);
            let row: FrameworkElement = xaml::load(&row_markup(index, engine, is_default))?;
            children.Append(&row.cast::<UIElement>()?)?;
            let more: FrameworkElement = xaml::find(&row, &more_name(index))?;
            let (me, edited) = (self.me.clone(), engine.clone());
            on_item_click(&row, &format!("SearchEngineEdit{index}"), move || {
                let (me, edited, more) = (me.clone(), edited.clone(), more.clone());
                // Once the menu has closed, which would take a flyout opened during its click along.
                exec::spawn(async move {
                    if let Some(me) = me.upgrade() {
                        me.open_editor(Some(&edited), &more);
                    }
                });
            })?;
            if is_default {
                continue;
            }
            let (me, id) = (self.me.clone(), engine.id.clone());
            on_item_click(&row, &format!("SearchEngineDefault{index}"), move || {
                if let Some(me) = me.upgrade() {
                    me.change(|b| b.core(|p| p.search_engines().set_default(&id)));
                }
            })?;
            let (me, id) = (self.me.clone(), engine.id.clone());
            on_item_click(&row, &format!("SearchEngineDelete{index}"), move || {
                if let Some(me) = me.upgrade() {
                    me.change(|b| b.core(|p| p.search_engines().remove(&id)));
                }
            })?;
        }
        Ok(())
    }

    fn fill_combo(&self, engines: &[SearchEngine], default: Option<&SearchEngineId>) -> Result<()> {
        let items = self.combo.cast::<ItemsControl>()?.Items()?;
        items.Clear()?;
        for engine in engines {
            items.Append(&xaml::boxed(&engine.name)?)?;
        }
        let selected = engines.iter().position(|e| Some(&e.id) == default);
        self.combo
            .cast::<Selector>()?
            .SetSelectedIndex(selected.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1))
    }

    /// Runs `change`, then shows the engines again, after the click that asked for it has
    /// finished with the row it came from.
    fn change(&self, change: impl FnOnce(&Browser) -> std::result::Result<(), vsesvit_core::Error>) {
        let Some(browser) = self.browser.upgrade() else {
            return;
        };
        if let Err(e) = change(&browser) {
            log::warn!("search engines: {e}");
        }
        let me = self.me.clone();
        exec::spawn(async move {
            if let Some(me) = me.upgrade()
                && let Err(e) = me.fill()
            {
                log::warn!("search engines page: {e}");
            }
        });
    }

    /// Opens the editor under `anchor`, for `engine` or for a new one.
    fn open_editor(&self, engine: Option<&SearchEngine>, anchor: &FrameworkElement) {
        let opened = Editor::open(self, engine, anchor);
        match opened {
            Ok(editor) => *self.editor.borrow_mut() = Some(editor),
            Err(e) => log::warn!("search engine editor: {e}"),
        }
    }

    /// The editor once its flyout has opened.
    pub(crate) fn editor(&self) -> Option<Rc<Editor>> {
        self.editor.borrow().clone().filter(|e| e.opened.get())
    }

    /// Each row's name as it shows, and its More actions button, for the scripted runs.
    pub(crate) fn rows(&self) -> Result<Vec<(String, Button)>> {
        let children = self.list.Children()?;
        let mut rows = Vec::new();
        for index in 0..children.Size()? {
            let row = children.GetAt(index)?.cast::<FrameworkElement>()?;
            let index = index as usize;
            let name = xaml::find::<TextBlock>(&row, &format!("SearchEngineName{index}"))?.Text()?;
            rows.push((name.to_string(), xaml::find(&row, &more_name(index))?));
        }
        Ok(rows)
    }
}

/// The flyout that adds or edits an engine.
pub(crate) struct Editor {
    flyout: FlyoutBase,
    /// Shown, with its boxes loaded: text put in them before then raises no `TextChanged`.
    opened: Cell<bool>,
    boxes: [(FormField, TextBox, TextBlock); 3],
    save: Control,
    /// `None` for a new engine.
    editing: Option<SearchEngineId>,
    engines: Weak<Engines>,
}

impl Editor {
    fn open(engines: &Engines, engine: Option<&SearchEngine>, anchor: &FrameworkElement) -> Result<Rc<Self>> {
        let flyout: Flyout = xaml::load(EDITOR)?;
        let content: FrameworkElement = xaml::load(EDITOR_CONTENT)?;
        if let Some(browser) = engines.browser.upgrade() {
            content.SetRequestedTheme(super::element_theme(browser.theme()))?;
        }
        flyout.SetContent(&content)?;
        let field = |(kind, name): (FormField, &str)| -> Result<(FormField, TextBox, TextBlock)> {
            Ok((kind, xaml::find(&content, name)?, xaml::find(&content, &format!("{name}Problem"))?))
        };
        let [name, keyword, url] = FIELDS;
        let editor = Rc::new(Editor {
            flyout: flyout.cast()?,
            opened: Cell::new(false),
            boxes: [field(name)?, field(keyword)?, field(url)?],
            save: xaml::find(&content, "EngineSave")?,
            editing: engine.map(|e| e.id.clone()),
            engines: engines.me.clone(),
        });
        let (title, save) = if engine.is_some() { ("Edit search engine", "Save") } else { ("Add search engine", "Add") };
        xaml::find::<TextBlock>(&content, "EngineEditorTitle")?.SetText(title)?;
        editor.save.cast::<IContentControl>()?.SetContent(&xaml::boxed(save)?)?;
        let form = engine.map(EngineForm::of).unwrap_or_default();
        for ((_, text_box, _), text) in editor.boxes.iter().zip([&form.name, &form.keyword, &form.url]) {
            text_box.SetText(text)?;
            let me = Rc::downgrade(&editor);
            text_box
                .TextChanged(move |_, _| {
                    if let Some(me) = me.upgrade() {
                        me.validate();
                    }
                })?
                .forget();
        }
        editor.validate();

        let me = Rc::downgrade(&editor);
        on_click(&editor.save, move || {
            if let Some(me) = me.upgrade() {
                me.save();
            }
        })?;
        let me = Rc::downgrade(&editor);
        on_click(&xaml::find::<Button>(&content, "EngineCancel")?, move || {
            if let Some(me) = me.upgrade() {
                me.close();
            }
        })?;
        let me = Rc::downgrade(&editor);
        content
            .cast::<UIElement>()?
            .KeyDown(move |_, args| {
                let (Some(me), Some(args)) = (me.upgrade(), args.as_ref()) else {
                    return;
                };
                if args.Key().is_ok_and(|k| k.0 == VK_RETURN) {
                    let _ = args.SetHandled(true);
                    me.save();
                }
            })?
            .forget();
        let me = Rc::downgrade(&editor);
        editor
            .flyout
            .Opened(move |_, _| {
                if let Some(me) = me.upgrade() {
                    me.opened.set(true);
                }
            })?
            .forget();
        let me = engines.me.clone();
        editor
            .flyout
            .Closed(move |_, _| {
                if let Some(me) = me.upgrade() {
                    me.editor.borrow_mut().take();
                }
            })?
            .forget();

        editor.flyout.ShowAt(anchor)?;
        let first = editor.boxes[0].1.clone();
        exec::spawn(async move {
            let _ = first.cast::<UIElement>().and_then(|b| b.Focus(FocusState::Programmatic));
        });
        Ok(editor)
    }

    fn form(&self) -> EngineForm {
        let [name, keyword, url] = self.boxes.each_ref().map(|(_, text_box, _)| text_box.Text().map(|t| t.to_string()).unwrap_or_default());
        EngineForm { name, keyword, url }
    }

    /// Says under each box what core finds wrong with it, except for a box that is only empty,
    /// and enables saving only when nothing is. Returns whether it can save.
    fn validate(&self) -> bool {
        let Some(browser) = self.engines.upgrade().and_then(|e| e.browser.upgrade()) else {
            return false;
        };
        let checked = browser.core(|p| p.search_engines().check(self.editing.as_ref(), &self.form()));
        let problems = checked.unwrap_or_else(|e| {
            log::warn!("search engine editor: {e}");
            Vec::new()
        });
        for (field, _, problem) in &self.boxes {
            let shown = problems.iter().find(|p| p.field() == *field && !p.is_blank());
            let _ = problem.SetText(&shown.map(ToString::to_string).unwrap_or_default());
            let _ = xaml::set_visible(problem, shown.is_some());
        }
        let ok = problems.is_empty();
        let _ = self.save.SetIsEnabled(ok);
        ok
    }

    /// Saves the form and closes, when core takes it.
    pub(crate) fn save(&self) {
        let Some(engines) = self.engines.upgrade() else { return };
        if !self.validate() {
            return;
        }
        let form = self.form();
        let editing = self.editing.clone();
        self.close();
        engines.change(|b| b.core(|p| p.search_engines().save(editing.as_ref(), &form).map(|_| ())));
    }

    pub(crate) fn close(&self) {
        let _ = self.flyout.Hide();
    }

    /// Types `text` into the box for `field`, for the scripted runs.
    pub(crate) fn fill(&self, field: FormField, text: &str) -> Result<()> {
        let (_, text_box, _) = self.boxes.iter().find(|(f, _, _)| *f == field).ok_or_else(windows_core::Error::empty)?;
        text_box.SetText(text)
    }

    /// The problem shown under the box for `field`, if any, and whether saving is enabled.
    pub(crate) fn shown(&self, field: FormField) -> (Option<String>, bool) {
        let problem = self
            .boxes
            .iter()
            .find(|(f, _, _)| *f == field)
            .filter(|(_, _, p)| xaml::is_visible(p))
            .and_then(|(_, _, p)| p.Text().ok())
            .map(|t| t.to_string());
        (problem, self.save.IsEnabled().unwrap_or(false))
    }
}
