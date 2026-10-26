//! Owned by WX.1.
//! The extension part of the engine contract (design section 5.4), frozen by WX.1 on the extension
//! spike's gate outcome A (`spike-ext-win.md`). One set of calls and events serves every install backend:
//! the External Extensions JSON on Linux, and on Windows the per-process HKCU override of Chromium's
//! registry route or, if the owner declines it (design Q31), outcome B's browser-target pipe. Which
//! backend runs is the engine's business; the shell sees only what this file names.
//!
//! The engine's half of the work: [`ExtensionStart`] in `EngineConfig` before Chromium starts,
//! `Engine::extension_call` with [`ExtensionCall`] and its [`ExtensionReply`] while it runs, and
//! `Engine::close_foreign` for the browsers Chromium opens itself. Its events are
//! `BrowserEvent::Extension` and `BrowserEvent::ForeignWindow`. FakeEngine's half is in `fake.rs`,
//! tested below.

use std::path::PathBuf;

pub use vsesvit_core::extensions::ExtensionId;

use crate::types::{Browsing, Chord, Url};

/// What the engine installs into Chromium before Chromium starts (design sections 3.1 step 7 and 5.2).
/// The shell computes it with the publish rule of section 5.5; the engine applies it in
/// `EngineFactory::initialize`, before Chromium reads its install sources.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtensionStart {
    /// Every extension the publish rule allows. Whatever the backend installed before and is not listed
    /// here is removed from Chromium's install sources, which uninstalls it with its storage. The helper
    /// is the engine's own and is never listed.
    pub published: Vec<Published>,
}

/// One extension the shell publishes into Chromium.
#[derive(Clone, Debug, PartialEq)]
pub struct Published {
    /// Its id.
    pub id: ExtensionId,
    /// The archived CRX, byte-identical to what core verified (`<root>/extension-archives`, section 5.2).
    pub archive: PathBuf,
    /// The archived version. Chromium keeps a newer one it updated to itself (section 5.5).
    pub version: String,
    /// Core's extracted directory of the same version. Only an unpacked-load backend (outcome B) uses it.
    pub unpacked: PathBuf,
    /// The desired enabled state. Before start the engine writes it where Chromium honours it; the
    /// start-up reconciliation of section 5.5 covers the rest.
    pub enabled: bool,
    /// The desired private-window permission, likewise.
    pub incognito: bool,
    /// Storage to seed before the extension first runs (section 5.9). The engine never overwrites
    /// storage Chromium already has.
    pub seed: Option<StorageSeed>,
}

/// Extension storage carried from an older profile (section 5.9): each area's keys and values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StorageSeed {
    /// `chrome.storage.local`.
    pub local: serde_json::Map<String, serde_json::Value>,
    /// `chrome.storage.sync`, which stays local (section 5.6).
    pub sync: serde_json::Map<String, serde_json::Value>,
}

/// A call into Chromium's extension system. Every call answers with one `RequestResult::Extension`.
#[derive(Clone, Debug, PartialEq)]
pub enum ExtensionCall {
    /// What Chromium runs: [`ExtensionReply::List`]. Through the helper.
    List,
    /// The running manifest's text: [`ExtensionReply::Manifest`].
    Manifest {
        /// The extension.
        id: ExtensionId,
    },
    /// Private-window access and commands of every extension: [`ExtensionReply::Details`]. Load errors
    /// found on the way arrive as [`ExtensionEvent::LoadFailed`].
    Details,
    /// Enables or disables. `gesture` is true only for the user's own click: re-enabling an extension
    /// Chromium disabled for a permission increase needs it, and then Chromium shows its own re-enable
    /// dialog, whose answer is the approval (section 5.5).
    SetEnabled {
        /// The extension.
        id: ExtensionId,
        /// On or off.
        enabled: bool,
        /// Whether the user's click asked for it.
        gesture: bool,
    },
    /// Allows or forbids private windows.
    SetIncognito {
        /// The extension.
        id: ExtensionId,
        /// Allowed or not.
        allowed: bool,
    },
    /// Binds a command's shortcut; `None` removes Chromium's binding.
    SetCommand {
        /// The extension.
        id: ExtensionId,
        /// The command's name in the manifest.
        command: String,
        /// The new chord.
        chord: Option<Chord>,
    },
    /// Blocks or allows the extension's notifications (section 5.7).
    SetNotificationsBlocked {
        /// The extension.
        id: ExtensionId,
        /// Blocked or not.
        blocked: bool,
    },
    /// Asks Chromium to check for an update now; results arrive as `Installed` events.
    RequestUpdateCheck {
        /// The extension.
        id: ExtensionId,
    },
    /// Installs while running. [`ExtensionReply::Published`] says whether it runs now or from the next
    /// start, which depends on the backend.
    Publish(Published),
    /// Uninstalls without a Chromium dialog and removes the extension from Chromium's install sources.
    /// It answers when `ExtensionEvent::Uninstalled` arrives for its id, or fails after 20 s.
    Uninstall {
        /// The extension.
        id: ExtensionId,
    },
    /// Writes the keys Chromium does not have yet into each storage area and leaves the others (the
    /// fallback import of section 5.9).
    ImportStorage {
        /// The extension.
        id: ExtensionId,
        /// What to import.
        seed: StorageSeed,
    },
}

/// The answer to an [`ExtensionCall`].
#[derive(Clone, Debug, PartialEq)]
pub enum ExtensionReply {
    /// `List`.
    List(Vec<RuntimeInfo>),
    /// `Manifest`: the manifest's JSON text, as Chromium runs it.
    Manifest(String),
    /// `Details`.
    Details(Vec<ExtensionDetails>),
    /// `Publish`: whether the extension runs now (`true`) or from the next start.
    Published {
        /// Installed while running.
        live: bool,
    },
    /// Every other call.
    Done,
}

/// One extension as Chromium runs it: the runtime record of design section 5.1.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeInfo {
    /// Its id.
    pub id: ExtensionId,
    /// The running version.
    pub version: String,
    /// Whether it runs.
    pub enabled: bool,
    /// Why Chromium disabled it, when it is disabled.
    pub disabled_reason: Option<DisableReason>,
    /// The granted API permissions.
    pub permissions: Vec<String>,
    /// The granted host permissions.
    pub host_permissions: Vec<String>,
    /// How it was installed.
    pub install_type: InstallType,
}

/// Why Chromium disabled an extension (`chrome.management`'s `disabledReason`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DisableReason {
    /// An update asks for more permissions; only the user's click and Chromium's dialog re-enable it.
    PermissionsIncrease,
    /// Anything else: the user, an install not yet acknowledged, a corrupted install.
    Other,
}

/// How an extension came to Chromium (`chrome.management`'s `installType`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InstallType {
    /// By policy; nothing can remove it.
    Admin,
    /// Unpacked, loaded for this session (section 5.2 step 5).
    Development,
    /// From a store, by Chromium's own install flow.
    Normal,
    /// From an external install source: the shell's, or another program's (section 5.1).
    Sideload,
    /// Anything else.
    Other,
}

/// What `Details` reports per extension.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionDetails {
    /// The extension.
    pub id: ExtensionId,
    /// Whether it may run in private windows.
    pub incognito: bool,
    /// Its commands.
    pub commands: Vec<ExtensionCommand>,
}

/// One command of an extension.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtensionCommand {
    /// Its name in the manifest (`_execute_action` included).
    pub name: String,
    /// Its description.
    pub description: String,
    /// The active binding, Chromium's text (`"Ctrl+Shift+Y"`); empty when unbound.
    pub keybinding: String,
    /// The same binding as a chord; `None` when unbound or a key no chord names (media keys), which
    /// cannot clash with Vsesvit's keymap.
    pub chord: Option<Chord>,
}

/// What Chromium's extension system tells the shell.
#[derive(Clone, Debug, PartialEq)]
pub enum ExtensionEvent {
    /// The helper answers; calls through it work from now on.
    HelperReady,
    /// The helper did not answer within 5 s; extension management is unavailable this session.
    HelperUnavailable,
    /// An extension was installed or updated. The shell tells which by comparing with its last `List`.
    Installed(RuntimeInfo),
    /// An extension was removed.
    Uninstalled {
        /// The extension.
        id: ExtensionId,
    },
    /// An extension was enabled.
    Enabled {
        /// The extension.
        id: ExtensionId,
    },
    /// An extension was disabled.
    Disabled {
        /// The extension.
        id: ExtensionId,
        /// Why.
        reason: DisableReason,
    },
    /// An extension failed to load or reported an error.
    LoadFailed {
        /// The extension.
        id: ExtensionId,
        /// Chromium's message.
        error: String,
    },
}

/// A browser Chromium opened itself (`tabs.create`, `windows.create`, `openOptionsPage`), hidden and
/// held by the engine until the shell has reopened its page as a tab (section 5.7).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ForeignId(pub u64);

/// The life of a foreign browser, in order: `Opened`, then `Ready`; or `Refused` at any point.
#[derive(Clone, Debug, PartialEq)]
pub enum ForeignWindowEvent {
    /// It was hidden in time. `browsing` is `Normal` for the global request context and `Private` for any
    /// other, so a page of a context the shell did not create never reopens in the normal one.
    Opened {
        /// Its id.
        id: ForeignId,
        /// Where the shell reopens it.
        browsing: Browsing,
    },
    /// It has a URL to reopen. `None`: it stayed blank, and the shell opens its new tab page. The shell
    /// opens the tab, waits for its `TabEvent::Created`, then calls `Engine::close_foreign`.
    Ready {
        /// Its id.
        id: ForeignId,
        /// Its first committed URL that is not blank.
        url: Option<Url>,
    },
    /// Hiding it failed, so the engine closed it; nothing is reopened.
    Refused {
        /// Its id.
        id: ForeignId,
        /// Why, for the log.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    //! FakeEngine's extension half: calls, events and foreign browsers.

    use futures_channel::mpsc::UnboundedReceiver;

    use super::*;
    use crate::event::BrowserEvent;
    use crate::fake::{FakeCommand, FakeEngine};
    use crate::hooks::{NeutralizeFailed, ShellHooks};
    use crate::ids::{NativeHandle, RequestId};
    use crate::types::{NavigationDecision, NavigationRequest, PopupDecision, PopupRequest, RequestResult};
    use crate::{Engine, FakePhase};

    fn drain(events: &mut UnboundedReceiver<BrowserEvent>) -> Vec<BrowserEvent> {
        std::iter::from_fn(|| events.try_recv().ok()).collect()
    }

    fn id(letter: char) -> ExtensionId {
        ExtensionId::parse(&letter.to_string().repeat(32)).expect("a valid id")
    }

    fn finished(id: RequestId, reply: Result<ExtensionReply, String>) -> BrowserEvent {
        BrowserEvent::RequestFinished { id, result: RequestResult::Extension(reply) }
    }

    struct Hooks {
        neutralize: Result<(), NeutralizeFailed>,
    }

    impl ShellHooks for Hooks {
        fn navigation_policy(&self, _request: &NavigationRequest) -> NavigationDecision {
            NavigationDecision::Allow
        }

        fn popup_policy(&self, _request: &PopupRequest) -> PopupDecision {
            PopupDecision::Block
        }

        fn neutralize_foreign(&self, _window: NativeHandle) -> Result<(), NeutralizeFailed> {
            self.neutralize.clone()
        }
    }

    fn with_hooks(neutralize: Result<(), NeutralizeFailed>) -> (std::rc::Rc<FakeEngine>, UnboundedReceiver<BrowserEvent>) {
        let (engine, events) = FakeEngine::new();
        engine.set_hooks(std::rc::Rc::new(Hooks { neutralize }));
        (engine, events)
    }

    #[test]
    fn calls_are_recorded_and_answered_with_scripted_replies_or_wait() {
        let (engine, mut events) = FakeEngine::new();
        engine.script_result(RequestResult::Extension(Ok(ExtensionReply::List(Vec::new()))));
        let list = engine.extension_call(ExtensionCall::List);
        let set = ExtensionCall::SetEnabled { id: id('a'), enabled: false, gesture: false };
        let pending = engine.extension_call(set.clone());

        assert_eq!(engine.commands(), [FakeCommand::ExtensionCall(ExtensionCall::List, list), FakeCommand::ExtensionCall(set, pending)]);
        assert_eq!(drain(&mut events), [finished(list, Ok(ExtensionReply::List(Vec::new())))]);
        assert_eq!(engine.pending_requests(), [pending]);
        assert!(engine.finish_request(pending, RequestResult::Extension(Ok(ExtensionReply::Done))));
        assert_eq!(drain(&mut events), [finished(pending, Ok(ExtensionReply::Done))]);
    }

    #[test]
    fn uninstall_answers_only_when_its_extension_is_uninstalled() {
        let (engine, mut events) = FakeEngine::new();
        engine.script_result(RequestResult::Extension(Ok(ExtensionReply::Done)));
        let call = engine.extension_call(ExtensionCall::Uninstall { id: id('a') });
        assert_eq!(drain(&mut events), [], "a scripted reply is not taken by Uninstall");
        let next = engine.extension_call(ExtensionCall::Details);
        assert_eq!(drain(&mut events), [finished(next, Ok(ExtensionReply::Done))], "the next call takes it");

        engine.extension_event(ExtensionEvent::Uninstalled { id: id('b') });
        assert_eq!(drain(&mut events), [BrowserEvent::Extension(ExtensionEvent::Uninstalled { id: id('b') })]);
        assert_eq!(engine.pending_requests(), [call], "another id's removal does not answer it");

        engine.extension_event(ExtensionEvent::Uninstalled { id: id('a') });
        assert_eq!(
            drain(&mut events),
            [BrowserEvent::Extension(ExtensionEvent::Uninstalled { id: id('a') }), finished(call, Ok(ExtensionReply::Done))]
        );
        assert_eq!(engine.pending_requests(), []);
    }

    #[test]
    fn an_uninstall_that_never_completes_times_out_through_finish_request() {
        let (engine, mut events) = FakeEngine::new();
        let call = engine.extension_call(ExtensionCall::Uninstall { id: id('a') });
        let timeout = RequestResult::Extension(Err("timed out".to_owned()));
        assert!(engine.finish_request(call, timeout));
        assert_eq!(drain(&mut events), [finished(call, Err("timed out".to_owned()))]);
        engine.extension_event(ExtensionEvent::Uninstalled { id: id('a') });
        assert_eq!(drain(&mut events), [BrowserEvent::Extension(ExtensionEvent::Uninstalled { id: id('a') })], "answered once");
    }

    #[test]
    fn calls_fail_at_once_after_shutdown_began() {
        let (engine, mut events) = FakeEngine::new();
        let early = engine.extension_call(ExtensionCall::Uninstall { id: id('a') });
        engine.begin_shutdown();
        drain(&mut events);
        let late = engine.extension_call(ExtensionCall::List);
        assert_eq!(drain(&mut events), [finished(late, Err("shutting down".to_owned()))]);
        assert_eq!(engine.pending_requests(), [early], "a call made while running still waits for its answer");
    }

    #[test]
    fn extension_events_reach_the_shell_in_order() {
        let (engine, mut events) = FakeEngine::new();
        let info = RuntimeInfo {
            id: id('a'),
            version: "2.0".to_owned(),
            enabled: false,
            disabled_reason: Some(DisableReason::PermissionsIncrease),
            permissions: vec!["tabs".to_owned()],
            host_permissions: Vec::new(),
            install_type: InstallType::Sideload,
        };
        let sent = [
            ExtensionEvent::HelperReady,
            ExtensionEvent::Installed(info),
            ExtensionEvent::Disabled { id: id('a'), reason: DisableReason::PermissionsIncrease },
            ExtensionEvent::Enabled { id: id('a') },
            ExtensionEvent::LoadFailed { id: id('b'), error: "Manifest is not valid JSON.".to_owned() },
        ];
        for event in sent.clone() {
            engine.extension_event(event);
        }
        assert_eq!(drain(&mut events), sent.map(BrowserEvent::Extension));
    }

    #[test]
    fn a_neutralized_foreign_browser_is_reopened_then_closed_by_the_shell() {
        let (engine, mut events) = with_hooks(Ok(()));
        let base = engine.live_user_browsers();
        let foreign = engine.open_foreign(Browsing::Private);
        assert_eq!(drain(&mut events), [BrowserEvent::ForeignWindow(ForeignWindowEvent::Opened { id: foreign, browsing: Browsing::Private })]);
        assert_eq!((engine.foreign_browsers(), engine.live_user_browsers()), (vec![foreign], base + 1), "counted while it lives");

        let url = Url::parse("https://example.com/").expect("url");
        engine.foreign_ready(foreign, Some(url.clone()));
        assert_eq!(drain(&mut events), [BrowserEvent::ForeignWindow(ForeignWindowEvent::Ready { id: foreign, url: Some(url) })]);

        engine.close_foreign(foreign);
        engine.close_foreign(ForeignId(999));
        assert_eq!(engine.commands().last(), Some(&FakeCommand::CloseForeign(ForeignId(999))));
        assert_eq!((engine.foreign_browsers(), engine.live_user_browsers()), (vec![], base));
        assert_eq!(drain(&mut events), []);
    }

    #[test]
    fn a_foreign_browser_that_cannot_be_hidden_is_refused_and_closed() {
        let failed = NeutralizeFailed { reason: "no such window".to_owned() };
        let (engine, mut events) = with_hooks(Err(failed));
        let foreign = engine.open_foreign(Browsing::Normal);
        assert_eq!(
            drain(&mut events),
            [BrowserEvent::ForeignWindow(ForeignWindowEvent::Refused { id: foreign, reason: "no such window".to_owned() })]
        );
        assert_eq!(engine.foreign_browsers(), []);

        let (engine, mut events) = FakeEngine::new();
        let foreign = engine.open_foreign(Browsing::Normal);
        assert!(
            matches!(&drain(&mut events)[..], [BrowserEvent::ForeignWindow(ForeignWindowEvent::Refused { id, .. })] if *id == foreign),
            "without hooks nothing can hide it"
        );
    }

    #[test]
    fn foreign_browsers_close_with_the_user_browsers_and_late_ones_are_closed_unannounced() {
        let (engine, mut events) = with_hooks(Ok(()));
        let foreign = engine.open_foreign(Browsing::Normal);
        engine.begin_shutdown();
        assert_eq!(engine.foreign_browsers(), []);
        assert_eq!(
            drain(&mut events),
            [
                BrowserEvent::ForeignWindow(ForeignWindowEvent::Opened { id: foreign, browsing: Browsing::Normal }),
                BrowserEvent::UserBrowsersClosed
            ]
        );

        let late = engine.open_foreign(Browsing::Normal);
        assert_eq!(drain(&mut events), [], "no Opened after Running");
        assert!(!engine.foreign_browsers().contains(&late));
        engine.close_internal();
        assert_eq!(engine.phase(), FakePhase::Drained);
    }
}
