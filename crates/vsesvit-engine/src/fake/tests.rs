//! Owned by W0.2.
//! FakeEngine tests: the prompt protocol of design section 1.5 for every kind, the two shutdown phases
//! of section 3.6, `detach_tab` (section 4.2), and the page model and hooks of UA section 4.5.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use futures_channel::mpsc::UnboundedReceiver;

use super::*;
use crate::prompt::{AuthChallenger, FileChooserMode, JsDialogKind, MenuModel, PromptKind};
use crate::types::{ContextMenuParams, OpenDisposition, PermissionAnswer, PermissionRequest};
use crate::{KeyboardOwner, MenuCommandId};

fn url(text: &str) -> Url {
    Url::parse(text).expect("a valid URL")
}

fn drain(events: &mut UnboundedReceiver<BrowserEvent>) -> Vec<BrowserEvent> {
    std::iter::from_fn(|| events.try_recv().ok()).collect()
}

fn spec(tab: TabId, page: InitialPage) -> TabSpec {
    TabSpec {
        tab,
        kind: TabKind::Page,
        browsing: Browsing::Normal,
        parent: NativeHandle::from_raw(7),
        bounds: PhysRect { x: 0, y: 0, width: 800, height: 600 },
        scale_factor: 1.0,
        visible: true,
        page,
        opener: None,
    }
}

fn open_tab(engine: &FakeEngine) -> TabId {
    let tab = TabId::next();
    engine.create_tab(spec(tab, InitialPage::Blank)).expect("running");
    tab
}

/// A request of each kind for `tab`, its right answer and an answer of another kind.
fn sample(kind: PromptKind, tab: TabId) -> (PromptRequest, PromptAnswer, PromptAnswer) {
    let wrong = PromptAnswer::BeforeUnload { leave: true };
    match kind {
        PromptKind::ContextMenu => (
            PromptRequest::ContextMenu {
                tab,
                at: PagePoint { x: 5, y: 6 },
                params: ContextMenuParams::default(),
                model: MenuModel::default(),
            },
            PromptAnswer::Menu(Some(MenuCommandId(49000))),
            wrong,
        ),
        PromptKind::JsDialog => (
            PromptRequest::JsDialog { tab, origin: None, kind: JsDialogKind::Confirm, message: "Sure?".to_owned() },
            PromptAnswer::JsDialog { accept: true, text: None },
            wrong,
        ),
        PromptKind::BeforeUnload => (
            PromptRequest::BeforeUnload { tab, is_reload: false },
            PromptAnswer::BeforeUnload { leave: false },
            PromptAnswer::Files(None),
        ),
        PromptKind::FileChooser => (
            PromptRequest::FileChooser {
                tab,
                mode: FileChooserMode::Open,
                title: None,
                default_path: None,
                accept: Vec::new(),
            },
            PromptAnswer::Files(Some(vec![PathBuf::from("a.txt")])),
            wrong,
        ),
        PromptKind::Auth => (
            PromptRequest::Auth {
                tab: Some(tab),
                requester: Some(url("https://example.com/")),
                challenger: AuthChallenger { host: "proxy.example".to_owned(), port: 3128, is_proxy: true },
                realm: Some("corp".to_owned()),
                scheme: Some("basic".to_owned()),
            },
            PromptAnswer::Auth(Some(("me".to_owned(), "pw".to_owned()))),
            wrong,
        ),
        PromptKind::Permission => (
            PromptRequest::Permission(PermissionRequest {
                tab,
                origin: Origin::parse("https://meet.example"),
                permissions: vec![Permission::Camera],
            }),
            PromptAnswer::Permission(PermissionAnswer::Allow),
            wrong,
        ),
    }
}

#[test]
fn every_kind_is_answered_exactly_once() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    for kind in PromptKind::ALL {
        let (request, answer, _) = sample(kind, tab);
        drain(&mut events);
        let id = engine.push_prompt(request.clone()).expect("admitted");
        assert_eq!(drain(&mut events), [BrowserEvent::Prompt { id, request }], "{kind:?}");
        assert_eq!(engine.answer_prompt(id, answer.clone()), Ok(()), "{kind:?}");
        assert_eq!(engine.answer_prompt(id, answer.clone()), Err(PromptError::AlreadyEnded), "{kind:?}: duplicate");
        assert_eq!(engine.end_prompt(id, PromptEnd::Navigated), Err(PromptError::AlreadyEnded), "{kind:?}: then ended");
        let outcomes: Vec<PromptOutcome> =
            engine.prompt_outcomes().into_iter().filter(|(of, _)| *of == id).map(|(_, o)| o).collect();
        assert_eq!(outcomes, [PromptOutcome::Answered(answer)], "{kind:?}: one action, the answer's");
        assert!(drain(&mut events).is_empty(), "{kind:?}: an end after the answer emits nothing");
    }
}

#[test]
fn every_kind_ended_first_refuses_the_answer() {
    let ends = [
        PromptEnd::Navigated,
        PromptEnd::TabClosed,
        PromptEnd::Withdrawn,
        PromptEnd::EngineReset,
        PromptEnd::MenuDismissed,
        PromptEnd::Shutdown,
    ];
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    for kind in PromptKind::ALL {
        for why in ends {
            let (request, answer, _) = sample(kind, tab);
            let id = engine.push_prompt(request).expect("admitted");
            drain(&mut events);
            assert_eq!(engine.end_prompt(id, why), Ok(()), "{kind:?} {why:?}");
            assert_eq!(drain(&mut events), [BrowserEvent::PromptEnded { id, why }], "{kind:?} {why:?}");
            assert_eq!(engine.answer_prompt(id, answer), Err(PromptError::AlreadyEnded), "{kind:?} {why:?}");
            let callback = match why {
                PromptEnd::Withdrawn | PromptEnd::EngineReset | PromptEnd::MenuDismissed => PromptOutcome::Dropped(why),
                _ => PromptOutcome::Cancelled(why),
            };
            assert_eq!(engine.prompt_outcomes().last(), Some(&(id, callback)), "{kind:?} {why:?}");
        }
    }
    assert_eq!(engine.prompt_outcomes().len(), PromptKind::ALL.len() * ends.len(), "one action per prompt");
}

#[test]
fn every_kind_refuses_a_wrong_kind_and_then_takes_the_right_one() {
    let (engine, _events) = FakeEngine::new();
    let tab = open_tab(&engine);
    for kind in PromptKind::ALL {
        let (request, answer, wrong) = sample(kind, tab);
        assert_ne!(wrong.kind(), kind);
        let id = engine.push_prompt(request).expect("admitted");
        assert_eq!(engine.answer_prompt(id, wrong), Err(PromptError::WrongKind), "{kind:?}");
        assert_eq!(engine.open_prompts(), [id], "{kind:?}: still open");
        assert_eq!(engine.answer_prompt(id, answer), Ok(()), "{kind:?}");
    }
}

#[test]
fn an_unknown_id_is_unknown_for_every_kind() {
    let (engine, _events) = FakeEngine::new();
    let tab = open_tab(&engine);
    for kind in PromptKind::ALL {
        let (_, answer, _) = sample(kind, tab);
        assert_eq!(engine.answer_prompt(PromptId(999), answer), Err(PromptError::Unknown), "{kind:?}");
    }
    assert_eq!(engine.answer_prompt(PromptId(0), PromptAnswer::Menu(None)), Err(PromptError::Unknown));
    assert_eq!(engine.end_prompt(PromptId(999), PromptEnd::Withdrawn), Err(PromptError::Unknown));
}

#[test]
fn a_dismissed_menu_drops_its_callback_and_refuses_a_later_answer() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    let (request, answer, _) = sample(PromptKind::ContextMenu, tab);
    let id = engine.push_prompt(request).expect("admitted");
    drain(&mut events);
    engine.end_prompt(id, PromptEnd::MenuDismissed).expect("open");
    assert_eq!(drain(&mut events), [BrowserEvent::PromptEnded { id, why: PromptEnd::MenuDismissed }]);
    assert_eq!(engine.answer_prompt(id, answer), Err(PromptError::AlreadyEnded));
    assert_eq!(engine.prompt_outcomes(), [(id, PromptOutcome::Dropped(PromptEnd::MenuDismissed))]);
}

#[test]
fn io_prompts_admitted_too_late_never_reach_the_shell() {
    for kind in PromptKind::ALL {
        let (engine, mut events) = FakeEngine::new();
        let closed = open_tab(&engine);
        let navigated = open_tab(&engine);
        let shut = open_tab(&engine);
        let after_close = engine.queue_io_prompt(sample(kind, closed).0);
        let after_commit = engine.queue_io_prompt(sample(kind, navigated).0);
        let after_shutdown = engine.queue_io_prompt(sample(kind, shut).0);
        engine.close_tab(closed, CloseMode::Force);
        engine.commit_document(navigated);
        assert_eq!(engine.admit(after_close), Err(Refused::BrowserGone), "{kind:?}");
        assert_eq!(engine.admit(after_commit), Err(Refused::NewDocument), "{kind:?}");
        engine.begin_shutdown();
        assert_eq!(engine.admit(after_shutdown), Err(Refused::ShuttingDown), "{kind:?}");
        assert!(!drain(&mut events).iter().any(|e| matches!(e, BrowserEvent::Prompt { .. })), "{kind:?}");
        let refused: Vec<Refused> = engine.refused_prompts().into_iter().map(|(_, why)| why).collect();
        assert_eq!(refused, [Refused::BrowserGone, Refused::NewDocument, Refused::ShuttingDown], "{kind:?}");
    }
}

#[test]
fn an_io_prompt_for_a_closing_tab_is_refused() {
    let (engine, _events) = FakeEngine::new();
    let tab = open_tab(&engine);
    engine.hold_close(tab);
    let queued = engine.queue_io_prompt(sample(PromptKind::Auth, tab).0);
    engine.close_tab(tab, CloseMode::Force);
    assert_eq!(engine.admit(queued), Err(Refused::BrowserClosing));
}

#[test]
fn an_io_prompt_in_the_same_document_is_admitted() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    let (request, answer, _) = sample(PromptKind::Auth, tab);
    let queued = engine.queue_io_prompt(request.clone());
    drain(&mut events);
    let id = engine.admit(queued).expect("same document");
    assert_eq!(drain(&mut events), [BrowserEvent::Prompt { id, request }]);
    assert_eq!(engine.answer_prompt(id, answer), Ok(()));
}

#[test]
fn an_auth_prompt_without_a_tab_is_admitted_until_shutdown() {
    let (engine, _events) = FakeEngine::new();
    let request = PromptRequest::Auth {
        tab: None,
        requester: None,
        challenger: AuthChallenger { host: "example.com".to_owned(), port: 443, is_proxy: false },
        realm: None,
        scheme: Some("basic".to_owned()),
    };
    let first = engine.queue_io_prompt(request.clone());
    let second = engine.queue_io_prompt(request);
    assert!(engine.admit(first).is_ok());
    engine.begin_shutdown();
    assert_eq!(engine.admit(second), Err(Refused::ShuttingDown));
}

#[test]
fn closing_navigating_and_shutting_down_end_open_prompts() {
    let (engine, mut events) = FakeEngine::new();
    let a = open_tab(&engine);
    let b = open_tab(&engine);
    let c = open_tab(&engine);
    let on_a = engine.push_prompt(sample(PromptKind::JsDialog, a).0).expect("admitted");
    let on_b = engine.push_prompt(sample(PromptKind::FileChooser, b).0).expect("admitted");
    let on_c = engine.push_prompt(sample(PromptKind::Permission, c).0).expect("admitted");
    drain(&mut events);
    engine.load(a, &url("https://example.com/next"));
    engine.close_tab(b, CloseMode::Force);
    engine.begin_shutdown();
    let ends: Vec<BrowserEvent> =
        drain(&mut events).into_iter().filter(|e| matches!(e, BrowserEvent::PromptEnded { .. })).collect();
    assert_eq!(
        ends,
        [
            BrowserEvent::PromptEnded { id: on_a, why: PromptEnd::Navigated },
            BrowserEvent::PromptEnded { id: on_b, why: PromptEnd::TabClosed },
            BrowserEvent::PromptEnded { id: on_c, why: PromptEnd::Shutdown },
        ]
    );
    assert!(engine.prompt_outcomes().iter().all(|(_, o)| matches!(o, PromptOutcome::Cancelled(_))));
}

#[test]
fn the_ledger_hands_each_callback_back_once() {
    let mut ledger: PromptLedger<u32, &str> = PromptLedger::new();
    let state = |_| Some(ScopeState { generation: 3, closing: false });
    let scope = Some(PromptScope { browser: 1, generation: 3 });
    let id = ledger.admit(PromptKind::BeforeUnload, scope, state, "cb").expect("admitted");
    assert_eq!(ledger.answer(id, &PromptAnswer::Files(None)).err(), Some(PromptError::WrongKind));
    assert_eq!(ledger.answer(id, &PromptAnswer::BeforeUnload { leave: true }), Ok("cb"));
    assert_eq!(ledger.end(id).err(), Some(PromptError::AlreadyEnded));
    let stale = Some(PromptScope { browser: 1, generation: 2 });
    assert_eq!(ledger.admit(PromptKind::Auth, stale, state, "late").err(), Some((Refused::NewDocument, "late")));
    assert_eq!(ledger.admit(PromptKind::Auth, scope, |_| None, "gone").err(), Some((Refused::BrowserGone, "gone")));
    let closing = |_| Some(ScopeState { generation: 3, closing: true });
    assert_eq!(ledger.admit(PromptKind::Auth, scope, closing, "closing").err(), Some((Refused::BrowserClosing, "closing")));
    let other = Some(PromptScope { browser: 2, generation: 3 });
    let on_one = ledger.admit(PromptKind::Auth, scope, state, "one").expect("admitted");
    let on_two = ledger.admit(PromptKind::Auth, other, state, "two").expect("admitted");
    assert_eq!(ledger.end_browser(1), [(on_one, "one")]);
    assert_eq!(ledger.open_ids(), [on_two]);
    assert_eq!(ledger.end(on_two), Ok("two"));
    let open = ledger.admit(PromptKind::Auth, scope, state, "open").expect("admitted");
    assert_eq!(ledger.shut(), [(open, "open")]);
    assert_eq!(ledger.admit(PromptKind::Auth, None, state, "after").err(), Some((Refused::ShuttingDown, "after")));
}

#[test]
fn shutdown_closes_user_browsers_then_internal_ones() {
    let (engine, mut events) = FakeEngine::new();
    engine.set_hidden_host(NativeHandle::from_raw(1));
    let a = open_tab(&engine);
    let b = open_tab(&engine);
    assert_eq!((engine.live_browsers(), engine.live_user_browsers()), (3, 2));
    drain(&mut events);
    engine.begin_shutdown();
    assert_eq!(engine.phase(), FakePhase::ClosingUser);
    assert_eq!(
        drain(&mut events),
        [
            BrowserEvent::Tab(a, TabEvent::Closed),
            BrowserEvent::Tab(b, TabEvent::Closed),
            BrowserEvent::UserBrowsersClosed,
        ]
    );
    assert_eq!(engine.live_browsers(), 1, "the maintenance browser serves the final jobs");
    assert_eq!(engine.create_tab(spec(TabId::next(), InitialPage::Blank)), Err(EngineError::ShuttingDown));
    engine.close_internal();
    assert_eq!(drain(&mut events), [BrowserEvent::AllBrowsersClosed]);
    assert_eq!((engine.phase(), engine.live_browsers()), (FakePhase::Drained, 0));
    engine.begin_shutdown();
    engine.close_internal();
    assert!(drain(&mut events).is_empty(), "idempotent");
}

#[test]
fn a_creation_finishing_after_begin_shutdown_is_closed_at_once_and_counted_until_closed() {
    let (engine, mut events) = FakeEngine::new();
    let late = TabId::next();
    engine.hold_creation(late);
    engine.hold_close(late);
    engine.create_tab(spec(late, InitialPage::Url(url("https://example.com/")))).expect("running");
    engine.begin_shutdown();
    engine.close_internal();
    assert_eq!(engine.live_browsers(), 1, "a requested creation is counted");
    assert!(drain(&mut events).is_empty(), "neither phase ends while it is counted");
    assert!(engine.complete_creation(late));
    assert_eq!(engine.live_browsers(), 1, "closed at once, counted until its close");
    assert!(drain(&mut events).is_empty(), "never shown: no Created");
    assert!(engine.complete_close(late));
    assert_eq!(
        drain(&mut events),
        [BrowserEvent::Tab(late, TabEvent::Closed), BrowserEvent::UserBrowsersClosed, BrowserEvent::AllBrowsersClosed]
    );
}

#[test]
fn a_creation_finishing_in_the_first_phase_ends_it_once_closed() {
    let (engine, mut events) = FakeEngine::new();
    engine.set_hidden_host(NativeHandle::from_raw(1));
    let late = TabId::next();
    engine.hold_creation(late);
    engine.create_tab(spec(late, InitialPage::Blank)).expect("running");
    engine.begin_shutdown();
    assert!(drain(&mut events).is_empty(), "the requested creation holds phase one open");
    assert!(engine.complete_creation(late));
    assert_eq!(drain(&mut events), [BrowserEvent::Tab(late, TabEvent::Closed), BrowserEvent::UserBrowsersClosed]);
    assert_eq!((engine.phase(), engine.live_browsers()), (FakePhase::ClosingUser, 1));
}

#[test]
fn repeating_a_shutdown_phase_while_it_runs_changes_nothing() {
    let (engine, mut events) = FakeEngine::new();
    engine.set_hidden_host(NativeHandle::from_raw(1));
    let tab = open_tab(&engine);
    engine.hold_close(tab);
    drain(&mut events);
    engine.begin_shutdown();
    engine.begin_shutdown();
    assert!(drain(&mut events).is_empty());
    engine.close_internal();
    engine.close_internal();
    assert_eq!((engine.phase(), engine.live_browsers()), (FakePhase::ClosingInternal, 1));
    assert!(drain(&mut events).is_empty(), "the held tab keeps both phases open");
    assert!(engine.complete_close(tab));
    assert_eq!(
        drain(&mut events),
        [BrowserEvent::Tab(tab, TabEvent::Closed), BrowserEvent::UserBrowsersClosed, BrowserEvent::AllBrowsersClosed]
    );
}

#[test]
fn a_held_creation_loads_its_first_page_when_it_finishes() {
    let (engine, mut events) = FakeEngine::new();
    let tab = TabId::next();
    let first = url("https://example.com/");
    engine.hold_creation(tab);
    engine.create_tab(spec(tab, InitialPage::Url(first.clone()))).expect("running");
    assert!(drain(&mut events).is_empty());
    assert!(engine.complete_creation(tab));
    let shown = drain(&mut events);
    assert!(matches!(shown[0], BrowserEvent::Tab(t, TabEvent::Created { .. }) if t == tab));
    assert!(shown.contains(&BrowserEvent::Tab(tab, TabEvent::AddressChanged(first.clone()))));
    assert_eq!(engine.tab(tab).and_then(|p| p.url), Some(first));
}

#[test]
fn shutdown_with_nothing_open_reports_both_phases() {
    let (engine, mut events) = FakeEngine::new();
    engine.close_internal();
    assert_eq!(drain(&mut events), [BrowserEvent::UserBrowsersClosed, BrowserEvent::AllBrowsersClosed]);
}

#[test]
fn popups_are_blocked_without_asking_after_shutdown_began() {
    struct Allow(RefCell<u32>);
    impl ShellHooks for Allow {
        fn navigation_policy(&self, _: &NavigationRequest) -> NavigationDecision {
            NavigationDecision::Allow
        }
        fn popup_policy(&self, _: &PopupRequest) -> PopupDecision {
            *self.0.borrow_mut() += 1;
            PopupDecision::AllowAsTab {
                tab: TabId::next(),
                parent: NativeHandle::from_raw(7),
                bounds: PhysRect::default(),
                scale_factor: 1.0,
            }
        }
        fn neutralize_foreign(&self, _: NativeHandle) -> Result<(), crate::NeutralizeFailed> {
            Ok(())
        }
    }
    let (engine, _events) = FakeEngine::new();
    let hooks = Rc::new(Allow(RefCell::new(0)));
    engine.set_hooks(hooks.clone());
    let opener = open_tab(&engine);
    let request = PopupRequest {
        opener,
        url: url("https://example.com/popup"),
        disposition: OpenDisposition::NewPopup,
        user_gesture: true,
    };
    engine.begin_shutdown();
    assert_eq!(engine.open_popup(request), PopupDecision::Block);
    assert_eq!(*hooks.0.borrow(), 0);
    assert_eq!(engine.live_browsers(), 0, "nothing counted");
}

#[test]
fn a_beforeunload_handler_can_keep_the_tab_open() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    engine.set_beforeunload(tab, true);
    drain(&mut events);
    let mut close_asks = || {
        engine.close_tab(tab, CloseMode::RunUnloadHandlers);
        let asked = drain(&mut events);
        let [BrowserEvent::Prompt { id, request }] = asked.as_slice() else { panic!("one prompt: {asked:?}") };
        assert_eq!(*request, PromptRequest::BeforeUnload { tab, is_reload: false });
        *id
    };
    let stay = close_asks();
    engine.answer_prompt(stay, PromptAnswer::BeforeUnload { leave: false }).expect("open");
    assert_eq!(engine.tabs(), [tab], "the page stays");
    let leave = close_asks();
    engine.answer_prompt(leave, PromptAnswer::BeforeUnload { leave: true }).expect("open");
    assert_eq!(drain(&mut events), [BrowserEvent::Tab(tab, TabEvent::Closed)]);
    let forced = open_tab(&engine);
    engine.set_beforeunload(forced, true);
    drain(&mut events);
    engine.close_tab(forced, CloseMode::Force);
    assert_eq!(drain(&mut events), [BrowserEvent::Tab(forced, TabEvent::Closed)], "Force asks nothing");
}

#[test]
fn a_same_document_commit_keeps_prompts_open() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    let id = engine.push_prompt(sample(PromptKind::JsDialog, tab).0).expect("admitted");
    drain(&mut events);
    let fragment = url("https://example.com/#part");
    engine.commit_same_document(tab, fragment.clone());
    assert_eq!(engine.open_prompts(), [id]);
    assert_eq!(
        drain(&mut events),
        [
            BrowserEvent::Tab(
                tab,
                TabEvent::Committed {
                    url: fragment.clone(),
                    kind: CommitKind::SameDocument,
                    transition: EngineTransition::Link,
                    mime: None
                }
            ),
            BrowserEvent::Tab(tab, TabEvent::AddressChanged(fragment)),
        ]
    );
}

#[test]
fn detach_parks_the_browser_until_its_downloads_end() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    let prompt = engine.push_prompt(sample(PromptKind::JsDialog, tab).0).expect("admitted");
    engine.set_downloading(tab, true);
    drain(&mut events);
    let parked = engine.detach_tab(tab).expect("live");
    assert_eq!(
        drain(&mut events),
        [BrowserEvent::PromptEnded { id: prompt, why: PromptEnd::TabClosed }, BrowserEvent::Tab(tab, TabEvent::Detached)]
    );
    assert_eq!(engine.tabs(), []);
    assert_eq!(engine.parked(), [parked]);
    assert_eq!(engine.live_user_browsers(), 1, "still counted");
    assert_eq!(engine.detach_tab(tab), Err(EngineError::UnknownTab(tab)));
    assert!(engine.end_parked_downloads(parked));
    assert_eq!(engine.live_user_browsers(), 0);
    assert!(drain(&mut events).is_empty(), "no Closed for a detached tab");
}

#[test]
fn detach_without_a_download_closes_the_browser_at_once() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    drain(&mut events);
    engine.detach_tab(tab).expect("live");
    assert_eq!(drain(&mut events), [BrowserEvent::Tab(tab, TabEvent::Detached)]);
    assert_eq!((engine.parked(), engine.live_browsers()), (vec![], 0));
}

#[test]
fn begin_shutdown_closes_parked_browsers() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    engine.set_downloading(tab, true);
    engine.detach_tab(tab).expect("live");
    drain(&mut events);
    engine.begin_shutdown();
    assert_eq!(drain(&mut events), [BrowserEvent::UserBrowsersClosed]);
}

#[test]
fn a_tab_loads_and_moves_through_its_history() {
    let (engine, mut events) = FakeEngine::new();
    let tab = TabId::next();
    let first = url("https://example.com/one");
    let second = url("https://example.com/two");
    engine.create_tab(spec(tab, InitialPage::Url(first.clone()))).expect("running");
    let created = drain(&mut events);
    assert!(matches!(created[0], BrowserEvent::Tab(t, TabEvent::Created { .. }) if t == tab));
    assert!(created.contains(&BrowserEvent::Tab(tab, TabEvent::AddressChanged(first.clone()))));
    engine.load(tab, &second);
    drain(&mut events);
    engine.go_back(tab);
    let page = engine.tab(tab).expect("open");
    assert_eq!(page.url, Some(first.clone()));
    assert_eq!((page.history.len(), page.history_index), (2, 0));
    let loading: Vec<BrowserEvent> = drain(&mut events)
        .into_iter()
        .filter(|e| matches!(e, BrowserEvent::Tab(_, TabEvent::LoadingStateChanged { .. })))
        .collect();
    assert_eq!(
        loading,
        [
            BrowserEvent::Tab(tab, TabEvent::LoadingStateChanged { loading: true, can_go_back: true, can_go_forward: false }),
            BrowserEvent::Tab(tab, TabEvent::LoadingStateChanged { loading: false, can_go_back: false, can_go_forward: true }),
        ],
        "before the move, then after it"
    );
    engine.go_forward(tab);
    assert_eq!(engine.tab(tab).and_then(|p| p.url), Some(second));
    engine.set_zoom(tab, Zoom(1.5));
    engine.set_audio_muted(tab, true);
    engine.show_devtools(tab, None);
    assert_eq!(engine.zoom(tab), Zoom(1.5));
    assert!(engine.has_devtools(tab) && engine.tab(tab).is_some_and(|p| p.muted));
}

#[test]
fn requests_get_scripted_results_or_wait() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    drain(&mut events);
    engine.script_result(RequestResult::Eval(Ok("2".to_owned())));
    let scripted = engine.eval(tab, "1 + 1");
    let pending = engine.capture_page(tab);
    assert_eq!(
        drain(&mut events),
        [BrowserEvent::RequestFinished { id: scripted, result: RequestResult::Eval(Ok("2".to_owned())) }]
    );
    assert_eq!(engine.pending_requests(), [pending]);
    assert!(engine.finish_request(pending, RequestResult::Captured(Ok(vec![0x89]))));
    assert!(!engine.finish_request(pending, RequestResult::Captured(Ok(vec![]))));
    assert_eq!(drain(&mut events).len(), 1);
    assert!(engine.commands().contains(&FakeCommand::Eval(tab, "1 + 1".to_owned(), scripted)));
}

#[test]
fn page_focus_is_kept_per_tab_and_focus_moves_between_pages() {
    let (engine, mut events) = FakeEngine::new();
    let a = TabId::next();
    engine.set_page_focus(a, PageFocus::NEW_BROWSER);
    engine.create_tab(spec(a, InitialPage::Blank)).expect("running");
    let b = open_tab(&engine);
    assert_eq!(engine.page_focus(a), Some(PageFocus::NEW_BROWSER));
    let selected = PageFocus { window_active: true, tab_selected: true, keyboard: KeyboardOwner::Page };
    engine.set_page_focus(b, selected);
    drain(&mut events);
    engine.focus_page(a);
    engine.focus_page(b);
    engine.blur_page(b);
    assert_eq!(
        drain(&mut events),
        [
            BrowserEvent::Tab(a, TabEvent::FocusChanged { focused: true }),
            BrowserEvent::Tab(a, TabEvent::FocusChanged { focused: false }),
            BrowserEvent::Tab(b, TabEvent::FocusChanged { focused: true }),
            BrowserEvent::Tab(b, TabEvent::FocusChanged { focused: false }),
        ]
    );
    assert_eq!(engine.focused(), None);
    assert_eq!(engine.page_focus(b), Some(selected));
}

#[test]
fn chromium_focus_requests_follow_the_published_policy() {
    let (engine, _events) = FakeEngine::new();
    let tab = open_tab(&engine);
    assert!(!engine.engine_focus_request(tab), "no policy published: refused like a new browser");
    engine.set_page_focus(tab, PageFocus { window_active: true, tab_selected: true, keyboard: KeyboardOwner::Chrome });
    assert!(!engine.engine_focus_request(tab), "the omnibox has the keyboard");
    engine.set_page_focus(tab, PageFocus { window_active: true, tab_selected: true, keyboard: KeyboardOwner::Page });
    assert!(engine.engine_focus_request(tab));
    assert_eq!(engine.focused(), Some(tab));
}

#[test]
fn hooks_decide_page_navigations_and_popups() {
    struct Hooks;
    impl ShellHooks for Hooks {
        fn navigation_policy(&self, request: &NavigationRequest) -> NavigationDecision {
            if request.url.scheme() == "http" {
                let mut https = request.url.clone();
                https.set_scheme("https").expect("http to https");
                NavigationDecision::Redirect(https)
            } else {
                NavigationDecision::Cancel
            }
        }
        fn popup_policy(&self, _: &PopupRequest) -> PopupDecision {
            PopupDecision::AllowAsTab {
                tab: TabId::next(),
                parent: NativeHandle::from_raw(9),
                bounds: PhysRect::default(),
                scale_factor: 2.0,
            }
        }
        fn neutralize_foreign(&self, _: NativeHandle) -> Result<(), crate::NeutralizeFailed> {
            Ok(())
        }
    }
    let (engine, mut events) = FakeEngine::new();
    engine.set_hooks(Rc::new(Hooks));
    let tab = open_tab(&engine);
    let navigation = |to: &str| NavigationRequest {
        tab,
        url: url(to),
        is_main_frame: true,
        is_redirect: false,
        user_gesture: true,
        browser_initiated: false,
        transition: EngineTransition::Link,
    };
    assert_eq!(engine.navigate_from_page(navigation("ftp://example.com/")), NavigationDecision::Cancel);
    assert_eq!(engine.tab(tab).and_then(|p| p.url), None);
    engine.navigate_from_page(navigation("http://example.com/"));
    assert_eq!(engine.tab(tab).and_then(|p| p.url), Some(url("https://example.com/")));
    drain(&mut events);
    let popup_url = url("https://example.com/popup");
    let PopupDecision::AllowAsTab { tab: popup, .. } = engine.open_popup(PopupRequest {
        opener: tab,
        url: popup_url.clone(),
        disposition: OpenDisposition::NewForegroundTab,
        user_gesture: true,
    }) else {
        panic!("allowed")
    };
    let after = drain(&mut events);
    assert!(after.contains(&BrowserEvent::PopupOpened {
        opener: tab,
        tab: popup,
        url: popup_url.clone(),
        disposition: OpenDisposition::NewForegroundTab,
    }));
    let page = engine.tab(popup).expect("a counted tab");
    assert_eq!((page.opener, page.url, page.scale_factor), (Some(tab), Some(popup_url), 2.0));
}

#[test]
fn a_page_closing_itself_reports_an_unrequested_close() {
    let (engine, mut events) = FakeEngine::new();
    let tab = open_tab(&engine);
    drain(&mut events);
    engine.close_from_page(tab);
    assert_eq!(drain(&mut events), [BrowserEvent::Tab(tab, TabEvent::Closed)]);
    assert!(!engine.commands().iter().any(|c| matches!(c, FakeCommand::CloseTab(..))));
}

#[test]
fn the_factory_hands_out_the_engine_once() {
    let factory = FakeEngineFactory::new();
    let config = EngineConfig {
        root_cache_path: PathBuf::from("profile/engine/cef"),
        log_file: None,
        verbose_log: false,
        smooth_scrolling: true,
        hardware_acceleration: true,
        user_agent_product: "Vsesvit/0.1".to_owned(),
        persist_session_cookies: false,
        background_color: 0xff20_2020,
        locale: "en-US".to_owned(),
        device_scale_factor: None,
        password_store: None,
        load_extensions: Vec::new(),
        extensions: Default::default(),
        scripted: true,
        extra_switches: Vec::new(),
    };
    assert_eq!(factory.chromium_version(), "154.0.8037.98");
    let mut parts = factory.initialize(config.clone()).expect("first call");
    assert_eq!(factory.config(), Some(config.clone()));
    assert!(matches!(factory.initialize(config), Err(EngineInitError::Failed(_))));
    let mut wakes = parts.driver.schedule();
    assert!(factory.engine().wake(Duration::from_millis(5)));
    assert_eq!(wakes.try_recv().ok(), Some(Duration::from_millis(5)));
    parts.driver.do_work();
    parts.engine.load(TabId::next(), &url("https://example.com/"));
    factory.engine().inject(BrowserEvent::EngineError("injected".to_owned()));
    assert_eq!(parts.events.try_recv().ok(), Some(BrowserEvent::EngineError("injected".to_owned())));
    parts.driver.shutdown();
    assert_eq!(factory.engine().pump_steps(), 1);
    assert!(factory.engine().driver_shut_down());
}

#[test]
fn events_can_cross_threads() {
    fn send<T: Send>() {}
    send::<BrowserEvent>();
}
