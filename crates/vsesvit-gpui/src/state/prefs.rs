//! Owned by W1.5.
//! Preferences, read from core and written through it. A write tells every view which key changed
//! (`BrowserStateEvent::PrefsChanged`); the features that apply a pref subscribe to it.

use gpui::{App, Context};
use serde::Serialize;
use serde::de::DeserializeOwned;
use vsesvit_core::prefs::Pref;

use super::{BrowserState, BrowserStateEvent};
use crate::app::shell::Shell;

impl BrowserState {
    pub fn pref<T: DeserializeOwned>(&self, pref: &Pref<T>, cx: &App) -> T {
        Shell::core(cx).borrow_mut().prefs().get(pref)
    }

    pub fn set_pref<T: Serialize>(&mut self, pref: &Pref<T>, value: &T, cx: &mut Context<Self>) {
        let written = Shell::core(cx).borrow_mut().prefs().set(pref, value);
        match written {
            Ok(()) => cx.emit(BrowserStateEvent::PrefsChanged(vec![pref.key.to_owned()])),
            Err(e) => log::warn!("cannot save the {} preference: {e}", pref.key),
        }
    }
}
