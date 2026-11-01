// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Multi-tab workspace session manager and navigation history stack.

use std::collections::HashMap;
use parking_lot::RwLock;
use uuid::Uuid;

use crate::error::AppCoreError;
use crate::workspace::types::{
    current_epoch_ms, normalize_uri, HistoryEntry, WorkspaceLayout, WorkspaceSession, WorkspaceTab,
};

/// Thread-safe manager handling multiple workspace windows and tabs.
pub struct SessionManager {
    sessions: RwLock<HashMap<Uuid, WorkspaceSession>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    /// Creates a new empty session manager.
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Creates a new workspace session with an initial default tab.
    pub fn create_session(&self, name: &str, initial_uri: &str) -> Uuid {
        let session_id = Uuid::new_v4();
        let initial_tab = WorkspaceTab::new("Home".to_string(), initial_uri.to_string());
        let active_tab_id = initial_tab.id;
        let now_ms = current_epoch_ms();

        let session = WorkspaceSession {
            id: session_id,
            name: name.to_string(),
            active_tab_id,
            tabs: vec![initial_tab],
            layout: WorkspaceLayout::Single,
            created_at_epoch_ms: now_ms,
            updated_at_epoch_ms: now_ms,
        };

        self.sessions.write().insert(session_id, session);
        session_id
    }

    /// Closes and cleans up an entire workspace session.
    pub fn close_session(&self, session_id: Uuid) -> bool {
        self.sessions.write().remove(&session_id).is_some()
    }

    /// Appends a new tab to the specified session and makes it active.
    pub fn add_tab(&self, session_id: Uuid, title: &str, uri: &str) -> Result<Uuid, AppCoreError> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(&session_id).ok_or_else(|| AppCoreError::Workspace {
            reason: format!("Session '{}' not found", session_id),
        })?;

        let tab = WorkspaceTab::new(title.to_string(), uri.to_string());
        let tab_id = tab.id;
        session.tabs.push(tab);
        session.active_tab_id = tab_id;
        session.updated_at_epoch_ms = current_epoch_ms();

        Ok(tab_id)
    }

    /// Closes a tab within a session. If the closed tab was active, activates the adjacent tab.
    pub fn close_tab(&self, session_id: Uuid, tab_id: Uuid) -> Result<(), AppCoreError> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(&session_id).ok_or_else(|| AppCoreError::Workspace {
            reason: format!("Session '{}' not found", session_id),
        })?;

        if session.tabs.len() <= 1 {
            return Err(AppCoreError::Workspace {
                reason: "Cannot close the last remaining tab in a session".to_string(),
            });
        }

        let pos = session.tabs.iter().position(|t| t.id == tab_id).ok_or_else(|| {
            AppCoreError::Workspace {
                reason: format!("Tab '{}' not found in session", tab_id),
            }
        })?;

        session.tabs.remove(pos);

        if session.active_tab_id == tab_id {
            let next_idx = pos.min(session.tabs.len() - 1);
            session.active_tab_id = session.tabs[next_idx].id;
        }

        session.updated_at_epoch_ms = current_epoch_ms();
        Ok(())
    }

    /// Switches the active tab in a session.
    pub fn switch_tab(&self, session_id: Uuid, tab_id: Uuid) -> Result<(), AppCoreError> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(&session_id).ok_or_else(|| AppCoreError::Workspace {
            reason: format!("Session '{}' not found", session_id),
        })?;

        let exists = session.tabs.iter().any(|t| t.id == tab_id);
        if !exists {
            return Err(AppCoreError::Workspace {
                reason: format!("Tab '{}' does not exist in session", tab_id),
            });
        }

        session.active_tab_id = tab_id;
        session.updated_at_epoch_ms = current_epoch_ms();
        Ok(())
    }

    /// Navigates a tab to a new URI, recording history.
    pub fn navigate_tab(
        &self,
        session_id: Uuid,
        tab_id: Uuid,
        new_uri: &str,
    ) -> Result<(), AppCoreError> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(&session_id).ok_or_else(|| AppCoreError::Workspace {
            reason: format!("Session '{}' not found", session_id),
        })?;

        let tab = session.tabs.iter_mut().find(|t| t.id == tab_id).ok_or_else(|| {
            AppCoreError::Workspace {
                reason: format!("Tab '{}' not found", tab_id),
            }
        })?;

        let norm_uri = normalize_uri(new_uri);
        if tab.current_uri != norm_uri {
            let now_ms = current_epoch_ms();
            let old_uri = std::mem::replace(&mut tab.current_uri, norm_uri.clone());
            tab.history_back.push(old_uri);
            tab.history_forward.clear();
            tab.last_accessed_epoch_ms = now_ms;
            tab.history_entries.push(HistoryEntry::with_timestamp(
                norm_uri,
                Some(tab.title.clone()),
                now_ms,
            ));
            session.updated_at_epoch_ms = now_ms;
        }

        Ok(())
    }

    /// Navigates backward in tab history.
    pub fn navigate_back(&self, session_id: Uuid, tab_id: Uuid) -> Result<String, AppCoreError> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(&session_id).ok_or_else(|| AppCoreError::Workspace {
            reason: format!("Session '{}' not found", session_id),
        })?;

        let tab = session.tabs.iter_mut().find(|t| t.id == tab_id).ok_or_else(|| {
            AppCoreError::Workspace {
                reason: format!("Tab '{}' not found", tab_id),
            }
        })?;

        let prev = tab.history_back.pop().ok_or_else(|| AppCoreError::Workspace {
            reason: "History back stack is empty".to_string(),
        })?;

        let now_ms = current_epoch_ms();
        let old_uri = std::mem::replace(&mut tab.current_uri, prev.clone());
        tab.history_forward.push(old_uri);
        tab.last_accessed_epoch_ms = now_ms;
        session.updated_at_epoch_ms = now_ms;
        Ok(prev)
    }

    /// Navigates forward in tab history.
    pub fn navigate_forward(&self, session_id: Uuid, tab_id: Uuid) -> Result<String, AppCoreError> {
        let mut sessions = self.sessions.write();
        let session = sessions.get_mut(&session_id).ok_or_else(|| AppCoreError::Workspace {
            reason: format!("Session '{}' not found", session_id),
        })?;

        let tab = session.tabs.iter_mut().find(|t| t.id == tab_id).ok_or_else(|| {
            AppCoreError::Workspace {
                reason: format!("Tab '{}' not found", tab_id),
            }
        })?;

        let next = tab.history_forward.pop().ok_or_else(|| AppCoreError::Workspace {
            reason: "History forward stack is empty".to_string(),
        })?;

        let now_ms = current_epoch_ms();
        let old_uri = std::mem::replace(&mut tab.current_uri, next.clone());
        tab.history_back.push(old_uri);
        tab.last_accessed_epoch_ms = now_ms;
        session.updated_at_epoch_ms = now_ms;
        Ok(next)
    }

    /// Gets a snapshot copy of a session.
    pub fn get_session(&self, session_id: Uuid) -> Option<WorkspaceSession> {
        self.sessions.read().get(&session_id).cloned()
    }

    /// Lists all active sessions.
    pub fn list_sessions(&self) -> Vec<WorkspaceSession> {
        self.sessions.read().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_manager_tab_lifecycle_and_history() {
        let mgr = SessionManager::new();
        let session_id = mgr.create_session("Main Workspace", "file:///Users/home/../home");

        let s1 = mgr.get_session(session_id).expect("session not found");
        assert_eq!(s1.tabs.len(), 1);
        assert_eq!(s1.tabs[0].current_uri, "file:///Users/home");
        let tab1_id = s1.active_tab_id;

        // Add a second tab
        let tab2_id = mgr
            .add_tab(session_id, "Downloads", "file:///Users/home/Downloads")
            .expect("add tab failed");
        let s2 = mgr.get_session(session_id).unwrap();
        assert_eq!(s2.tabs.len(), 2);
        assert_eq!(s2.active_tab_id, tab2_id);

        // Navigate tab 2
        mgr.navigate_tab(session_id, tab2_id, "file:///Users/home/Downloads/sub")
            .expect("navigate failed");

        // Navigate back
        let back_uri = mgr.navigate_back(session_id, tab2_id).expect("back failed");
        assert_eq!(back_uri, "file:///Users/home/Downloads");

        // Navigate forward
        let fwd_uri = mgr.navigate_forward(session_id, tab2_id).expect("forward failed");
        assert_eq!(fwd_uri, "file:///Users/home/Downloads/sub");

        // Close tab 2, should switch active tab to tab 1
        mgr.close_tab(session_id, tab2_id).expect("close tab failed");
        let s3 = mgr.get_session(session_id).unwrap();
        assert_eq!(s3.tabs.len(), 1);
        assert_eq!(s3.active_tab_id, tab1_id);

        // Closing last tab must fail
        assert!(mgr.close_tab(session_id, tab1_id).is_err());

        // Close session
        assert!(mgr.close_session(session_id));
        assert!(mgr.get_session(session_id).is_none());
    }

    #[test]
    fn test_switch_tab_and_error_handling() {
        let mgr = SessionManager::default();
        let session_id = mgr.create_session("Multi", "file:///root");
        let tab2_id = mgr.add_tab(session_id, "Tab 2", "file:///tmp").unwrap();

        assert!(mgr.switch_tab(session_id, tab2_id).is_ok());
        let s = mgr.get_session(session_id).unwrap();
        assert_eq!(s.active_tab_id, tab2_id);

        let fake_uuid = Uuid::new_v4();
        assert!(mgr.switch_tab(session_id, fake_uuid).is_err());
        assert!(mgr.navigate_tab(fake_uuid, tab2_id, "file:///dest").is_err());
        assert!(mgr.navigate_back(session_id, fake_uuid).is_err());
        assert!(mgr.navigate_forward(session_id, fake_uuid).is_err());
    }
}
