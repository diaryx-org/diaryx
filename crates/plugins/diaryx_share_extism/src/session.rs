//! Thread-local share session state management.
//!
//! Uses RefCell-based globals for the single-threaded WASM guest runtime.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use yrs::Doc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareRole {
    Host,
    Guest,
}

use serde::{Deserialize, Serialize};

#[allow(dead_code)]
pub struct ShareSession {
    pub role: ShareRole,
    pub namespace_id: String,
    pub join_code: String,
    pub manifest_doc: Doc,
    pub open_files: HashMap<String, Doc>,
    pub pending_requests: HashSet<String>,
    pub read_only: bool,
}

thread_local! {
    static SESSION: RefCell<Option<ShareSession>> = const { RefCell::new(None) };
}

pub fn init_session(role: ShareRole, namespace_id: String, join_code: String, read_only: bool) {
    SESSION.with(|s| {
        *s.borrow_mut() = Some(ShareSession {
            role,
            namespace_id,
            join_code,
            manifest_doc: Doc::new(),
            open_files: HashMap::new(),
            pending_requests: HashSet::new(),
            read_only,
        });
    });
}

pub fn destroy_session() {
    SESSION.with(|s| {
        *s.borrow_mut() = None;
    });
}

pub fn has_session() -> bool {
    SESSION.with(|s| s.borrow().is_some())
}

pub fn with_session<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&ShareSession) -> R,
{
    SESSION.with(|s| {
        let borrow = s.borrow();
        borrow.as_ref().map(f)
    })
}

pub fn with_session_mut<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut ShareSession) -> R,
{
    SESSION.with(|s| {
        let mut borrow = s.borrow_mut();
        borrow.as_mut().map(f)
    })
}
