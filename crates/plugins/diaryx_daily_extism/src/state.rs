//! Plugin thread-local state and workspace lifecycle.

use std::cell::RefCell;

use chrono::{DateTime, FixedOffset, NaiveDate};
use diaryx_core::link_parser::LinkFormat;
use diaryx_plugin_sdk::prelude::*;

use crate::daily_logic::DailyPluginConfig;
use crate::links::read_link_format;
use crate::migration::migrate_legacy_config;
use crate::storage::{load_workspace_config, save_workspace_config};

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct InitParams {
    #[serde(default)]
    pub workspace_root: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DailyState {
    pub workspace_root: Option<String>,
    pub config: DailyPluginConfig,
    pub link_format: LinkFormat,
}

// WASM is single-threaded; use RefCell instead of Mutex to avoid panics on
// re-entrant host function calls (host may dispatch events while a host_*
// call is in flight, leading to recursive lock attempts).
thread_local! {
    static STATE: RefCell<DailyState> = RefCell::new(DailyState::default());
}

pub fn current_state() -> Result<DailyState, String> {
    STATE.with(|cell| Ok(cell.borrow().clone()))
}

pub fn with_state_mut<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut DailyState) -> Result<R, String>,
{
    STATE.with(|cell| {
        let mut state = cell.borrow().clone();
        let result = f(&mut state)?;
        *cell.borrow_mut() = state;
        Ok(result)
    })
}

pub fn current_local_datetime() -> Result<DateTime<FixedOffset>, String> {
    let raw = host::time::now_rfc3339()?;
    DateTime::parse_from_rfc3339(raw.trim())
        .map_err(|e| format!("failed to parse host_get_now response: {e}"))
}

pub fn current_local_date() -> Result<NaiveDate, String> {
    Ok(current_local_datetime()?.date_naive())
}

pub fn update_workspace_root(workspace_root: Option<String>) -> Result<(), String> {
    // Step 1: Always persist the workspace root and loaded config,
    // even if migration later fails.
    with_state_mut(|state| {
        state.workspace_root = workspace_root;
        state.config = load_workspace_config(state.workspace_root.as_deref());
        Ok(())
    })?;

    // Step 2: Read link_format from workspace root index.
    let link_format = {
        let state = current_state()?;
        read_link_format(&state)
    };
    with_state_mut(|state| {
        state.link_format = link_format;
        Ok(())
    })?;

    // Step 3: Attempt migration (reads/writes files). Non-fatal —
    // if this fails, the workspace root is still set from step 1.
    let migration_result = with_state_mut(|state| {
        migrate_legacy_config(state)?;
        save_workspace_config(state)?;
        Ok(())
    });
    if let Err(e) = migration_result {
        host::log::log("warn", &format!("Legacy config migration failed: {e}"));
    }
    Ok(())
}
