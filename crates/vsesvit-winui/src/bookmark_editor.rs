//! The bookmark editor: the star's "Bookmark added" / "Edit bookmark" bubble, which the bookmarks
//! bar's Edit…, Rename…, Add page… and Add folder… open too. It edits a name, a URL (links
//! only) and the folder, and saves them to core on Done or Enter; Remove deletes the bookmark.
//! Escape, the close button or a click elsewhere closes it and keeps what core has.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use vsesvit_core::Url;
use vsesvit_core::bookmarks::editor::{Edit, Target, parse_url};
use vsesvit_core::bookmarks::{BookmarkId, NodeKind};
use windows_core::{Interface, Result};

use crate::bindings::*;
use crate::window::BrowserWindow;
use crate::{dialogs, exec, xaml};

const FLYOUT: &str = r#"
<Flyout {ns} Placement="BottomEdgeAlignedRight">
  <Flyout.FlyoutPresenterStyle>
    <Style TargetType="FlyoutPresenter" BasedOn="{StaticResource DefaultFlyoutPresenterStyle}">
      <Setter Property="Padding" Value="20,14,20,20"/>
    </Style>
  </Flyout.FlyoutPresenterStyle>
</Flyout>"#;

/// The editor's content, loaded as its own root so that its names resolve before it is shown.
const CONTENT: &str = r#"
  <StackPanel {ns} Width="320" Spacing="6">
    <Grid>
      <TextBlock x:Name="EditorTitle" Style="{StaticResource SubtitleTextBlockStyle}" FontSize="18"
                 VerticalAlignment="Center"/>
      <Button x:Name="EditorClose" HorizontalAlignment="Right" Width="32" Height="32" Padding="0"
              Background="Transparent" BorderThickness="0" Margin="0,0,-10,0"
              ToolTipService.ToolTip="Close" AutomationProperties.Name="Close">
        <FontIcon Glyph="&#xE711;" FontSize="12"/>
      </Button>
    </Grid>
    <TextBlock Text="Name" Margin="0,6,0,0"/>
    <TextBox x:Name="EditorName" AutomationProperties.Name="Name"/>
    <StackPanel x:Name="EditorUrlPart" Spacing="6">
      <TextBlock Text="URL" Margin="0,4,0,0"/>
      <Grid>
        <TextBox x:Name="EditorUrl" AutomationProperties.Name="URL"/>
        <Border x:Name="EditorUrlInvalid" BorderThickness="2" CornerRadius="4" IsHitTestVisible="False"
                BorderBrush="{ThemeResource SystemFillColorCriticalBrush}" Visibility="Collapsed"/>
      </Grid>
      <TextBlock x:Name="EditorUrlHint" Text="Enter a web address, such as https://example.com"
                 Style="{StaticResource CaptionTextBlockStyle}" Visibility="Collapsed"
                 Foreground="{ThemeResource SystemFillColorCriticalBrush}"/>
    </StackPanel>
    <TextBlock Text="Folder" Margin="0,4,0,0"/>
    <ComboBox x:Name="EditorFolder" HorizontalAlignment="Stretch" AutomationProperties.Name="Folder"/>
    <StackPanel Orientation="Horizontal" HorizontalAlignment="Right" Spacing="8" Margin="0,12,0,0">
      <Button x:Name="EditorRemove" Content="Remove" MinWidth="96"/>
      <Button x:Name="EditorDone" Content="Done" MinWidth="96" Style="{StaticResource AccentButtonStyle}"/>
    </StackPanel>
  </StackPanel>"#;

pub(crate) const VK_RETURN: i32 = 0x0D;

/// An open editor.
pub(crate) struct Editor {
    window: Weak<BrowserWindow>,
    target: Target,
    flyout: FlyoutBase,
    root: FrameworkElement,
    name: TextBox,
    url: TextBox,
    folder: ComboBox,
    done: Control,
    folders: Vec<BookmarkId>,
    /// Whether the editor edits a link, and so has a URL to check.
    has_url: bool,
}

impl Editor {
    /// Opens the editor for `target` under `anchor`. `focus` gives it the keyboard focus, with
    /// the name selected, as a click does; scripted runs pass false.
    pub fn open(
        window: &Rc<BrowserWindow>,
        anchor: &FrameworkElement,
        target: Target,
        focus: bool,
    ) -> Result<Rc<Self>> {
        let browser = window.browser().ok_or_else(windows_core::Error::empty)?;
        let existing = target.existing().and_then(|id| browser.bookmark(id));
        let kind = existing.as_ref().map_or(
            match target {
                Target::NewFolder => NodeKind::Folder,
                _ => NodeKind::Url,
            },
            |n| n.kind,
        );
        let look = target.look(kind);
        let (name, url, parent) = match (&target, &existing) {
            (_, Some(node)) => (
                node.title.clone(),
                node.url.as_ref().map(Url::to_string).unwrap_or_default(),
                node.parent,
            ),
            (Target::NewPage { title, url }, None) => {
                (title.clone(), url.clone(), BookmarkId::TOOLBAR)
            }
            _ => ("New folder".to_owned(), String::new(), BookmarkId::TOOLBAR),
        };
        let exclude = (kind == NodeKind::Folder)
            .then(|| target.existing())
            .flatten();
        let choices = browser.bookmark_folders(exclude);

        let flyout: Flyout = xaml::load(FLYOUT)?;
        let root: FrameworkElement = xaml::load(CONTENT)?;
        flyout.SetContent(&root)?;
        let find = |name: &str| xaml::find::<FrameworkElement>(&root, name);
        find("EditorTitle")?
            .cast::<TextBlock>()?
            .SetText(look.title)?;
        xaml::set_visible(&find("EditorUrlPart")?, look.url)?;
        xaml::set_visible(&find("EditorRemove")?, look.remove)?;
        let editor = Rc::new(Self {
            window: Rc::downgrade(window),
            target,
            flyout: flyout.cast()?,
            name: find("EditorName")?.cast()?,
            url: find("EditorUrl")?.cast()?,
            folder: find("EditorFolder")?.cast()?,
            done: find("EditorDone")?.cast()?,
            root: root.clone(),
            folders: choices.iter().map(|(id, _)| *id).collect(),
            has_url: look.url,
        });
        editor.name.SetText(&name)?;
        editor.url.SetText(&url)?;
        let items = editor.folder.cast::<ItemsControl>()?.Items()?;
        for (_, label) in &choices {
            items.Append(&xaml::boxed(label)?)?;
        }
        let selected = choices
            .iter()
            .position(|(id, _)| *id == parent)
            .unwrap_or(0);
        editor
            .folder
            .cast::<Selector>()?
            .SetSelectedIndex(i32::try_from(selected).unwrap_or(0))?;
        editor.wire(&find)?;
        editor.validate();

        let options = FlyoutShowOptions::new()?;
        options.SetShowMode(if focus {
            FlyoutShowMode::Standard
        } else {
            FlyoutShowMode::Transient
        })?;
        editor.flyout.ShowAtWithOptions(anchor, &options)?;
        if focus {
            let name = editor.name.clone();
            exec::spawn(async move {
                let _ = name
                    .cast::<UIElement>()
                    .and_then(|n| n.Focus(FocusState::Programmatic));
                let _ = name.SelectAll();
            });
        }
        Ok(editor)
    }

    fn wire(self: &Rc<Self>, find: &dyn Fn(&str) -> Result<FrameworkElement>) -> Result<()> {
        let button = |name: &str, action: fn(&Editor)| -> Result<()> {
            let me = Rc::downgrade(self);
            find(name)?
                .cast::<ButtonBase>()?
                .Click(move |_, _| {
                    if let Some(me) = me.upgrade() {
                        action(&me);
                    }
                })?
                .forget();
            Ok(())
        };
        button("EditorDone", Editor::done)?;
        button("EditorRemove", Editor::remove)?;
        button("EditorClose", Editor::close)?;
        let me = Rc::downgrade(self);
        self.url
            .TextChanged(move |_, _| {
                if let Some(me) = me.upgrade() {
                    me.validate();
                }
            })?
            .forget();
        let me = Rc::downgrade(self);
        self.root
            .cast::<UIElement>()?
            .KeyDown(move |_, args| {
                let (Some(me), Some(args)) = (me.upgrade(), args.as_ref()) else {
                    return;
                };
                if args.Key().is_ok_and(|k| k.0 == VK_RETURN) {
                    let _ = args.SetHandled(true);
                    if me.done.IsEnabled().unwrap_or(false) {
                        me.done();
                    }
                }
            })?
            .forget();
        // The editor lives as long as its flyout is open.
        let keep = RefCell::new(Some(self.clone()));
        self.flyout
            .Closed(move |_, _| drop(keep.borrow_mut().take()))?
            .forget();
        Ok(())
    }

    fn url_text(&self) -> String {
        self.url.Text().map(|t| t.to_string()).unwrap_or_default()
    }

    /// An address that does not parse marks the URL field and disables Done.
    fn validate(&self) {
        let valid = !self.has_url || parse_url(&self.url_text()).is_some();
        let _ = self.done.SetIsEnabled(valid);
        for part in ["EditorUrlInvalid", "EditorUrlHint"] {
            if let Some(part) = self.part::<UIElement>(part) {
                let _ = xaml::set_visible(&part, !valid);
            }
        }
    }

    /// What Done would save now; `None` while the URL does not parse.
    pub fn edit(&self) -> Option<Edit> {
        let url = if self.has_url {
            Some(parse_url(&self.url_text())?)
        } else {
            None
        };
        let index = dialogs::selected_index(&self.folder);
        let folder = index
            .and_then(|i| self.folders.get(i).copied())
            .unwrap_or(BookmarkId::TOOLBAR);
        Some(Edit {
            target: self.target.clone(),
            name: self
                .name
                .Text()
                .map(|t| t.to_string().trim().to_owned())
                .unwrap_or_default(),
            url,
            folder,
        })
    }

    pub fn done(&self) {
        let (Some(edit), Some(browser)) =
            (self.edit(), self.window.upgrade().and_then(|w| w.browser()))
        else {
            return;
        };
        if let Err(e) = browser.save_bookmark(&edit) {
            log::warn!("saving a bookmark: {e}");
        }
        self.close();
    }

    pub fn remove(&self) {
        if let (Some(id), Some(browser)) = (
            self.target.existing(),
            self.window.upgrade().and_then(|w| w.browser()),
        ) {
            browser.remove_bookmark(id);
        }
        self.close();
    }

    pub fn close(&self) {
        let _ = self.flyout.Hide();
    }

    pub fn is_open(&self) -> bool {
        self.flyout.IsOpen().unwrap_or(false)
    }

    /// A named part of the editor.
    pub fn part<T: Interface>(&self, name: &str) -> Option<T> {
        self.root.FindName(name).ok()?.cast().ok()
    }
}
