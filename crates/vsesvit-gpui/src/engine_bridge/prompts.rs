//! Owned by W1.6.
//! The engine's questions on their way to the code that asks the user (design section 1.5, prompts).
//! Each `Prompt` goes to its owner: the context menu (W2.5), the page prompts (W2.6: JS dialogs,
//! beforeunload, the file chooser, HTTP auth) or the permission prompt (W3.1). The bridge remembers
//! each open prompt's owner, so `PromptEnded`, which names only the id, reaches the same owner.

use std::collections::HashMap;

use gpui::App;
use vsesvit_engine::{PromptAnswer, PromptEnd, PromptError, PromptId, PromptRequest};

/// The code that shows a prompt and answers it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    /// `context_menu`.
    ContextMenu,
    /// `tab::prompts`.
    PagePrompts,
    /// `privacy::permissions`.
    Permissions,
}

/// The open prompts and their owners.
#[derive(Default)]
pub(crate) struct Open(HashMap<PromptId, Owner>);

/// The prompts the engine asked and nobody answered or ended yet, oldest first.
pub fn open(cx: &App) -> Vec<(PromptId, Owner)> {
    let mut open: Vec<(PromptId, Owner)> = super::bridge(cx).prompts.0.iter().map(|(&id, &owner)| (id, owner)).collect();
    open.sort_by_key(|(id, _)| *id);
    open
}

/// An owner's answer. The prompt is closed unless the answer was of the wrong kind, which leaves it
/// open for the right one.
pub fn answer(cx: &mut App, id: PromptId, answer: PromptAnswer) -> Result<(), PromptError> {
    let result = super::engine(cx).answer_prompt(id, answer);
    if result != Err(PromptError::WrongKind) {
        super::bridge_mut(cx).prompts.0.remove(&id);
    }
    result
}

pub(crate) fn opened(id: PromptId, request: PromptRequest, cx: &mut App) {
    let owner = owner_of(&request);
    super::bridge_mut(cx).prompts.0.insert(id, owner);
    #[cfg(test)]
    tests::delivered(owner, id, None);
    match request {
        PromptRequest::ContextMenu { tab, at, params, model } => crate::context_menu::show(id, tab, at, params, model, cx),
        PromptRequest::Permission(request) => crate::privacy::permissions::request(id, request, cx),
        page => crate::tab::prompts::show(id, page, cx),
    }
}

pub(crate) fn ended(id: PromptId, why: PromptEnd, cx: &mut App) {
    let Some(owner) = super::bridge_mut(cx).prompts.0.remove(&id) else {
        log::debug!("prompts: {id:?} ended ({why:?}) after its answer");
        return;
    };
    #[cfg(test)]
    tests::delivered(owner, id, Some(why));
    match owner {
        Owner::ContextMenu => crate::context_menu::ended(id, why, cx),
        Owner::PagePrompts => crate::tab::prompts::ended(id, why, cx),
        Owner::Permissions => crate::privacy::permissions::ended(id, why, cx),
    }
}

fn owner_of(request: &PromptRequest) -> Owner {
    match request {
        PromptRequest::ContextMenu { .. } => Owner::ContextMenu,
        PromptRequest::JsDialog { .. } | PromptRequest::BeforeUnload { .. } | PromptRequest::FileChooser { .. } | PromptRequest::Auth { .. } => {
            Owner::PagePrompts
        }
        PromptRequest::Permission(_) => Owner::Permissions,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        static DELIVERED: RefCell<Vec<(Owner, PromptId, Option<PromptEnd>)>> = const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn delivered(owner: Owner, id: PromptId, end: Option<PromptEnd>) {
        DELIVERED.with(|d| d.borrow_mut().push((owner, id, end)));
    }

    /// What reached the owners' stubs on this thread: `None` for the prompt, `Some(why)` for its end.
    pub(crate) fn take_delivered() -> Vec<(Owner, PromptId, Option<PromptEnd>)> {
        DELIVERED.with(|d| std::mem::take(&mut *d.borrow_mut()))
    }
}
