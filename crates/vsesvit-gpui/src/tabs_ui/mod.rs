//! Owned by W1.7.
//! The tab list's views: the sidebar of the left and right layouts and the strip of the top one, both
//! drawn from the same rows.

pub mod drag;
pub mod group_editor;
pub mod row;
pub mod sidebar;
pub mod strip;
pub mod tab_menu;
pub mod tab_search;

pub use row::{RowActions, TabRow};
