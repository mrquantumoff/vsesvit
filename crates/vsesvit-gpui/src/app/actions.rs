//! Owned by W0.1.
//! Every command of the shell as a GPUI action, declared once (UA section 1.9): core's
//! `shortcuts::Command` list, the shell's own commands, and the parameterized actions. Handlers live
//! with the views that run them (`window/actions.rs` and the feature modules).

use gpui::{Action, actions};
use vsesvit_core::Url;
use vsesvit_core::extensions::ExtensionId;
use vsesvit_core::profiles::ProfileId;
use vsesvit_core::shortcuts::Command;
use vsesvit_core::tab_groups::GroupId;
use vsesvit_engine::TabId;

use crate::window::actions::{GroupMenuOp, TabMenuOp};

actions!(
    vsesvit,
    [
        NewTab,
        CloseTab,
        ReopenClosedTab,
        SearchTabs,
        NextTab,
        PreviousTab,
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectLastTab,
        ToggleTabList,
        NewWindow,
        NewPrivateWindow,
        Quit,
        FocusAddress,
        Back,
        Forward,
        Reload,
        ReloadBypassCache,
        SavePage,
        Print,
        Find,
        FindNext,
        FindPrevious,
        BookmarkPage,
        CopyCleanLink,
        CopyLink,
        ViewSource,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        Fullscreen,
        ToggleBookmarksBar,
        ShowBookmarks,
        ShowHistory,
        ShowDownloads,
        ShowSettings,
        ShowShortcuts,
        DeveloperTools,
        JavaScriptConsole,
        // The shell's own commands, with no entry in core's keymap.
        Stop,
        Home,
        ShowExtensions,
        ShowSiteInfo,
        About,
        Welcome,
        AddProfile,
        ManageProfiles,
        ApplyUpdate,
        MoveTabLeft,
        MoveTabRight,
    ]
);

/// The action a core command runs. The match is exhaustive, so a command added to core does not
/// compile until it has an action here.
pub fn for_command(command: Command) -> Box<dyn Action> {
    match command {
        Command::NewTab => Box::new(NewTab),
        Command::CloseTab => Box::new(CloseTab),
        Command::ReopenClosedTab => Box::new(ReopenClosedTab),
        Command::SearchTabs => Box::new(SearchTabs),
        Command::NextTab => Box::new(NextTab),
        Command::PreviousTab => Box::new(PreviousTab),
        Command::SelectTab1 => Box::new(SelectTab1),
        Command::SelectTab2 => Box::new(SelectTab2),
        Command::SelectTab3 => Box::new(SelectTab3),
        Command::SelectTab4 => Box::new(SelectTab4),
        Command::SelectTab5 => Box::new(SelectTab5),
        Command::SelectTab6 => Box::new(SelectTab6),
        Command::SelectTab7 => Box::new(SelectTab7),
        Command::SelectTab8 => Box::new(SelectTab8),
        Command::SelectLastTab => Box::new(SelectLastTab),
        Command::ToggleTabList => Box::new(ToggleTabList),
        Command::NewWindow => Box::new(NewWindow),
        Command::NewPrivateWindow => Box::new(NewPrivateWindow),
        Command::Quit => Box::new(Quit),
        Command::FocusAddress => Box::new(FocusAddress),
        Command::Back => Box::new(Back),
        Command::Forward => Box::new(Forward),
        Command::Reload => Box::new(Reload),
        Command::ReloadBypassCache => Box::new(ReloadBypassCache),
        Command::SavePage => Box::new(SavePage),
        Command::Print => Box::new(Print),
        Command::Find => Box::new(Find),
        Command::FindNext => Box::new(FindNext),
        Command::FindPrevious => Box::new(FindPrevious),
        Command::BookmarkPage => Box::new(BookmarkPage),
        Command::CopyCleanLink => Box::new(CopyCleanLink),
        Command::CopyLink => Box::new(CopyLink),
        Command::ViewSource => Box::new(ViewSource),
        Command::ZoomIn => Box::new(ZoomIn),
        Command::ZoomOut => Box::new(ZoomOut),
        Command::ZoomReset => Box::new(ZoomReset),
        Command::Fullscreen => Box::new(Fullscreen),
        Command::ToggleBookmarksBar => Box::new(ToggleBookmarksBar),
        Command::ShowBookmarks => Box::new(ShowBookmarks),
        Command::ShowHistory => Box::new(ShowHistory),
        Command::ShowDownloads => Box::new(ShowDownloads),
        Command::ShowSettings => Box::new(ShowSettings),
        Command::ShowShortcuts => Box::new(ShowShortcuts),
        Command::DeveloperTools => Box::new(DeveloperTools),
        Command::JavaScriptConsole => Box::new(JavaScriptConsole),
    }
}

/// Opens a profile in its own process.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = vsesvit, no_json)]
pub struct OpenProfile {
    pub id: ProfileId,
}

/// Opens a bookmark in the current tab.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = vsesvit, no_json)]
pub struct OpenBookmark {
    pub url: Url,
}

/// Opens a URL in a new tab of the current window.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = vsesvit, no_json)]
pub struct OpenInNewTab {
    pub url: Url,
}

/// Pins an extension's button to the toolbar, or unpins it.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = vsesvit, no_json)]
pub struct ExtensionPin {
    pub id: ExtensionId,
    pub pinned: bool,
}

/// Moves a pinned extension's button to an index among the pinned buttons.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = vsesvit, no_json)]
pub struct ExtensionMove {
    pub id: ExtensionId,
    pub to: usize,
}

/// A tab context menu command on one tab.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = vsesvit, no_json)]
pub struct TabOp {
    pub tab: TabId,
    pub op: TabMenuOp,
}

/// A group menu or editor command on one group.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = vsesvit, no_json)]
pub struct GroupOp {
    pub group: GroupId,
    pub op: GroupMenuOp,
}

#[cfg(test)]
mod tests {
    use vsesvit_core::shortcuts::Command;

    #[test]
    fn every_command_has_its_own_action() {
        let mut names: Vec<&str> = Command::ALL.iter().map(|&c| super::for_command(c).name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "two commands share an action");
        assert!(names.iter().all(|n| n.starts_with("vsesvit::")), "{names:?}");
    }
}
