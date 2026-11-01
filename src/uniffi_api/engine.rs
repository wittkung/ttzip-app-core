// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Centralized UniFFI application core facade engine.

use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use crate::device::adb::AdbClient;
use crate::device::entry::DeviceEntry;
use crate::transfer::coordinator::TransferCoordinator;
use crate::transfer::job::{TransferPriority, TransferProgress};
use crate::transfer::sliding_window::SlidingWindowThroughput;
use crate::uniffi_api::types::{
    UniFFIAppCoreError, UniFFIDeviceDescriptor, UniFFIFavoriteItem,
    UniFFITransferJobInfo, UniFFIWorkspaceSession, UniFFIWorkspaceTab,
};
use crate::workspace::favorites::FavoritesRegistry;
use crate::workspace::session::SessionManager;
use crate::workspace::types::{WorkspaceSession, WorkspaceTab};

/// The unified top-level application core instance exposed to foreign languages.
#[derive(uniffi::Object)]
pub struct AppCoreEngine {
    transfer_coord: Arc<TransferCoordinator>,
    session_mgr: Arc<SessionManager>,
    favorites_reg: Arc<FavoritesRegistry>,
}

#[uniffi::export]
impl AppCoreEngine {
    /// Creates a new AppCoreEngine instance anchored to the given configuration directory.
    #[uniffi::constructor]
    pub fn new(config_dir: String) -> Result<Arc<Self>, UniFFIAppCoreError> {
        let base_path = PathBuf::from(config_dir);
        let fav_path = base_path.join("favorites.json");

        let throughput = Arc::new(SlidingWindowThroughput::default());
        let transfer_coord = Arc::new(TransferCoordinator::new(throughput));

        let session_mgr = Arc::new(SessionManager::new());
        let favorites_reg = Arc::new(FavoritesRegistry::load_or_create(&fav_path)?);

        Ok(Arc::new(Self {
            transfer_coord,
            session_mgr,
            favorites_reg,
        }))
    }

    // ========================================================================
    // Device Subsystem
    // ========================================================================

    /// Scans the physical USB bus for connected MTP devices synchronously.
    pub fn scan_mtp_devices(&self) -> Result<Vec<UniFFIDeviceDescriptor>, UniFFIAppCoreError> {
        let devices = crate::device::mtp::scan_physical_mtp_devices()?;
        let descriptors = devices
            .into_iter()
            .map(|d| UniFFIDeviceDescriptor {
                id: format!("mtp://{:04X}:{:04X}", d.vendor_id, d.product_id),
                name: d.model,
                manufacturer: d.manufacturer,
                transport_type: "USB-MTP".to_string(),
                state: "Connected".to_string(),
            })
            .collect();
        Ok(descriptors)
    }

    /// Queries connected devices from the local ADB server bridge daemon synchronously.
    /// Connects via std::net::TcpStream with an explicit 4-second timeout and terminates socket immediately on return.
    pub fn list_adb_devices(&self) -> Result<Vec<UniFFIDeviceDescriptor>, UniFFIAppCoreError> {
        let devices = AdbClient::list_devices_via_daemon(None)?;
        let descriptors = devices
            .into_iter()
            .map(|d| UniFFIDeviceDescriptor {
                id: format!("adb://{}", d.serial),
                name: d.model.unwrap_or_else(|| d.serial.clone()),
                manufacturer: d.product.unwrap_or_else(|| "Android".to_string()),
                transport_type: format!("{:?}", d.transport_type),
                state: format!("{:?}", d.state),
            })
            .collect();
        Ok(descriptors)
    }

    /// Connects directly to a wireless ADB device at host:port synchronously.
    /// Performs authentic CNXN handshake with an explicit 4-second timeout and terminates socket immediately on return.
    pub fn connect_wireless_adb(
        &self,
        host: String,
        port: Option<u16>,
    ) -> Result<UniFFIDeviceDescriptor, UniFFIAppCoreError> {
        let dev = AdbClient::connect_wireless_direct(&host, port)?;
        Ok(UniFFIDeviceDescriptor {
            id: format!("adb://{}", dev.serial),
            name: dev.model.unwrap_or_else(|| dev.serial.clone()),
            manufacturer: dev.product.unwrap_or_else(|| "Android".to_string()),
            transport_type: "Tcp".to_string(),
            state: "Connected".to_string(),
        })
    }

    /// Lists directory contents on a connected ADB or MTP peripheral device synchronously.
    pub fn list_device_directory(
        &self,
        serial: String,
        path: String,
    ) -> Result<Vec<DeviceEntry>, UniFFIAppCoreError> {
        let vfs_path = if path.is_empty() { "/" } else { &path };
        if serial.starts_with("mtp://") || serial.starts_with("mtp:") {
            Self::list_mtp_directory(&serial, vfs_path)
        } else {
            Self::list_adb_directory(&serial, vfs_path)
        }
    }

    // ========================================================================
    // Transfer Subsystem
    // ========================================================================

    /// Queues a new file transfer job with priority and initial WAL state.
    pub fn submit_transfer(
        &self,
        priority_str: String,
        source_uri: String,
        destination_uri: String,
        total_bytes: u64,
    ) -> Result<String, UniFFIAppCoreError> {
        let priority = match priority_str.to_lowercase().as_str() {
            "critical" => TransferPriority::Critical,
            "high" => TransferPriority::High,
            "low" => TransferPriority::Low,
            _ => TransferPriority::Normal,
        };

        let job_id = self
            .transfer_coord
            .submit_job(priority, source_uri, destination_uri, total_bytes)?;
        Ok(job_id.to_string())
    }

    /// Records newly transferred physical chunk bytes.
    pub fn record_transfer_chunk(
        &self,
        job_id_str: String,
        chunk_bytes: u64,
    ) -> Result<UniFFITransferJobInfo, UniFFIAppCoreError> {
        let job_id = parse_uuid(&job_id_str)?;
        let progress = self
            .transfer_coord
            .record_chunk_transferred(job_id, chunk_bytes)?;
        Ok(map_transfer_progress(&progress))
    }

    /// Streams a physical chunk of data into the active transfer PartWriter.
    pub fn write_transfer_chunk(
        &self,
        job_id_str: String,
        chunk_data: Vec<u8>,
    ) -> Result<u64, UniFFIAppCoreError> {
        let job_id = parse_uuid(&job_id_str)?;
        let written = self
            .transfer_coord
            .write_chunk(job_id, &chunk_data)
            .map_err(|e| UniFFIAppCoreError::IoError {
                reason: format!("Failed to write chunk for job '{}': {}", job_id_str, e),
            })?;
        Ok(written)
    }

    /// Atomically finalizes an active transfer, flushing buffers and moving `.ttpart` to destination.
    pub fn finalize_transfer(&self, job_id_str: String) -> Result<String, UniFFIAppCoreError> {
        let job_id = parse_uuid(&job_id_str)?;
        let final_path = self
            .transfer_coord
            .finalize_job(job_id)
            .map_err(|e| UniFFIAppCoreError::IoError {
                reason: format!("Failed to finalize transfer job '{}': {}", job_id_str, e),
            })?;
        Ok(final_path.to_string_lossy().to_string())
    }

    /// Pauses an active transfer job.
    pub fn pause_transfer(&self, job_id_str: String) -> Result<(), UniFFIAppCoreError> {
        let job_id = parse_uuid(&job_id_str)?;
        self.transfer_coord.pause_job(job_id)?;
        Ok(())
    }

    /// Resumes a paused transfer job from its latest WAL checkpoint.
    pub fn resume_transfer(&self, job_id_str: String) -> Result<u64, UniFFIAppCoreError> {
        let job_id = parse_uuid(&job_id_str)?;
        let offset = self.transfer_coord.resume_job(job_id)?;
        Ok(offset)
    }

    /// Cancels a transfer job and purges its WAL checkpoint.
    pub fn cancel_transfer(&self, job_id_str: String) -> Result<(), UniFFIAppCoreError> {
        let job_id = parse_uuid(&job_id_str)?;
        self.transfer_coord.cancel_job(job_id)?;
        Ok(())
    }

    /// Queries real-time snapshot for a transfer job.
    pub fn get_transfer_progress(&self, job_id_str: String) -> Option<UniFFITransferJobInfo> {
        let job_id = Uuid::parse_str(&job_id_str).ok()?;
        self.transfer_coord
            .get_job_progress(job_id)
            .map(|p| map_transfer_progress(&p))
    }

    /// Returns current physical 500ms sliding-window throughput in MB/s.
    pub fn get_active_throughput_mb_s(&self) -> f64 {
        self.transfer_coord.active_throughput_mb_s()
    }

    /// Lists all transfer jobs sorted by priority.
    pub fn list_transfers(&self) -> Vec<UniFFITransferJobInfo> {
        self.transfer_coord
            .list_jobs()
            .iter()
            .map(map_transfer_progress)
            .collect()
    }

    // ========================================================================
    // Workspace Subsystem
    // ========================================================================

    /// Creates a new workspace session and returns its UUID.
    pub fn create_workspace_session(&self, name: String, initial_uri: String) -> String {
        self.session_mgr
            .create_session(&name, &initial_uri)
            .to_string()
    }

    /// Closes an entire workspace session.
    pub fn close_workspace_session(&self, session_id_str: String) -> bool {
        if let Ok(id) = Uuid::parse_str(&session_id_str) {
            self.session_mgr.close_session(id)
        } else {
            false
        }
    }

    /// Appends a new tab to a session and makes it active.
    pub fn add_workspace_tab(
        &self,
        session_id_str: String,
        title: String,
        uri: String,
    ) -> Result<String, UniFFIAppCoreError> {
        let sid = parse_uuid(&session_id_str)?;
        let tid = self.session_mgr.add_tab(sid, &title, &uri)?;
        Ok(tid.to_string())
    }

    /// Closes a tab within a session.
    pub fn close_workspace_tab(
        &self,
        session_id_str: String,
        tab_id_str: String,
    ) -> Result<(), UniFFIAppCoreError> {
        let sid = parse_uuid(&session_id_str)?;
        let tid = parse_uuid(&tab_id_str)?;
        self.session_mgr.close_tab(sid, tid)?;
        Ok(())
    }

    /// Switches the active tab in a session.
    pub fn switch_workspace_tab(
        &self,
        session_id_str: String,
        tab_id_str: String,
    ) -> Result<(), UniFFIAppCoreError> {
        let sid = parse_uuid(&session_id_str)?;
        let tid = parse_uuid(&tab_id_str)?;
        self.session_mgr.switch_tab(sid, tid)?;
        Ok(())
    }

    /// Navigates a tab to a new URI.
    pub fn navigate_workspace_tab(
        &self,
        session_id_str: String,
        tab_id_str: String,
        uri: String,
    ) -> Result<(), UniFFIAppCoreError> {
        let sid = parse_uuid(&session_id_str)?;
        let tid = parse_uuid(&tab_id_str)?;
        self.session_mgr.navigate_tab(sid, tid, &uri)?;
        Ok(())
    }

    /// Navigates backward in tab history.
    pub fn navigate_tab_back(
        &self,
        session_id_str: String,
        tab_id_str: String,
    ) -> Result<String, UniFFIAppCoreError> {
        let sid = parse_uuid(&session_id_str)?;
        let tid = parse_uuid(&tab_id_str)?;
        let prev = self.session_mgr.navigate_back(sid, tid)?;
        Ok(prev)
    }

    /// Navigates forward in tab history.
    pub fn navigate_tab_forward(
        &self,
        session_id_str: String,
        tab_id_str: String,
    ) -> Result<String, UniFFIAppCoreError> {
        let sid = parse_uuid(&session_id_str)?;
        let tid = parse_uuid(&tab_id_str)?;
        let next = self.session_mgr.navigate_forward(sid, tid)?;
        Ok(next)
    }

    /// Gets details of a session.
    pub fn get_workspace_session(&self, session_id_str: String) -> Option<UniFFIWorkspaceSession> {
        let sid = Uuid::parse_str(&session_id_str).ok()?;
        self.session_mgr.get_session(sid).map(|s| map_workspace_session(&s))
    }

    /// Lists all active workspace sessions.
    pub fn list_workspace_sessions(&self) -> Vec<UniFFIWorkspaceSession> {
        self.session_mgr
            .list_sessions()
            .iter()
            .map(map_workspace_session)
            .collect()
    }

    // ========================================================================
    // Favorites Subsystem
    // ========================================================================

    /// Adds a bookmark favorite item.
    pub fn add_favorite(
        &self,
        name: String,
        uri: String,
        icon_hint: String,
        is_pinned: bool,
        tags: Vec<String>,
    ) -> Result<UniFFIFavoriteItem, UniFFIAppCoreError> {
        let item = self
            .favorites_reg
            .add_favorite(&name, &uri, &icon_hint, is_pinned, tags)?;
        Ok(UniFFIFavoriteItem {
            id: item.id.to_string(),
            name: item.name,
            uri: item.uri,
            icon_hint: item.icon_hint,
            is_pinned: item.is_pinned,
            tags: item.tags,
            order_index: item.order_index,
        })
    }

    /// Removes a bookmark favorite item by ID.
    pub fn remove_favorite(&self, id_str: String) -> Result<bool, UniFFIAppCoreError> {
        let id = parse_uuid(&id_str)?;
        let removed = self.favorites_reg.remove_favorite(id)?;
        Ok(removed)
    }

    /// Lists all bookmarks sorted by order index.
    pub fn list_favorites(&self) -> Vec<UniFFIFavoriteItem> {
        self.favorites_reg
            .list_favorites()
            .into_iter()
            .map(|item| UniFFIFavoriteItem {
                id: item.id.to_string(),
                name: item.name,
                uri: item.uri,
                icon_hint: item.icon_hint,
                is_pinned: item.is_pinned,
                tags: item.tags,
                order_index: item.order_index,
            })
            .collect()
    }
}

impl AppCoreEngine {
    fn list_adb_directory(
        serial: &str,
        vfs_path: &str,
    ) -> Result<Vec<DeviceEntry>, UniFFIAppCoreError> {
        let clean_serial = serial.strip_prefix("adb://").unwrap_or(serial);
        let mut sync_stream = AdbClient::open_sync_via_daemon(clean_serial, None)
            .map_err(|e| UniFFIAppCoreError::AdbError {
                reason: format!("Failed to open ADB sync stream for device '{}': {}", clean_serial, e),
            })?;

        let adb_entries = sync_stream.list(vfs_path).map_err(|e| UniFFIAppCoreError::AdbError {
            reason: format!("Failed to list directory '{}' on device '{}': {}", vfs_path, clean_serial, e),
        })?;
        let _ = sync_stream.quit();

        let clean_path = vfs_path.trim_end_matches('/');
        let mapped = adb_entries
            .into_iter()
            .map(|entry| {
                let is_dir = entry.is_directory();
                let child_path = if clean_path.is_empty() || clean_path == "/" {
                    format!("/{}", entry.name)
                } else {
                    format!("{}/{}", clean_path, entry.name)
                };
                DeviceEntry {
                    path: child_path,
                    name: entry.name,
                    size: entry.size,
                    is_dir,
                    mtime: entry.mtime as u64,
                    object_id: None,
                    posix_mode: Some(entry.mode),
                }
            })
            .collect();
        Ok(mapped)
    }

    fn list_mtp_directory(
        serial: &str,
        vfs_path: &str,
    ) -> Result<Vec<DeviceEntry>, UniFFIAppCoreError> {
        crate::device::mtp::directory::list_hardware_directory(serial, vfs_path)
            .map_err(Into::into)
    }

}

fn parse_uuid(s: &str) -> Result<Uuid, UniFFIAppCoreError> {
    Uuid::parse_str(s).map_err(|e| UniFFIAppCoreError::WorkspaceError {
        reason: format!("Invalid UUID string '{}': {}", s, e),
    })
}

fn map_transfer_progress(p: &TransferProgress) -> UniFFITransferJobInfo {
    UniFFITransferJobInfo {
        id: p.job_id.to_string(),
        source_uri: p.source_uri.clone(),
        destination_uri: p.destination_uri.clone(),
        priority: "Normal".to_string(),
        total_bytes: p.total_bytes,
        transferred_bytes: p.transferred_bytes,
        state: (&p.state).into(),
        current_speed_mb_s: p.current_speed_mb_s,
        progress_ratio: p.progress_ratio,
    }
}

fn map_workspace_session(s: &WorkspaceSession) -> UniFFIWorkspaceSession {
    UniFFIWorkspaceSession {
        id: s.id.to_string(),
        name: s.name.clone(),
        active_tab_id: s.active_tab_id.to_string(),
        tabs: s.tabs.iter().map(map_workspace_tab).collect(),
        layout: format!("{:?}", s.layout),
    }
}

fn map_workspace_tab(t: &WorkspaceTab) -> UniFFIWorkspaceTab {
    UniFFIWorkspaceTab {
        id: t.id.to_string(),
        title: t.title.clone(),
        current_uri: t.current_uri.clone(),
        view_mode: format!("{:?}", t.view_mode),
        sort_order: format!("{:?}", t.sort_order),
        is_pinned: t.is_pinned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_engine_initialization_and_workspace_lifecycle() {
        let tmp = tempdir().expect("Must create tempdir");
        let engine = AppCoreEngine::new(tmp.path().to_string_lossy().to_string())
            .expect("Must initialize engine");

        let session_id = engine.create_workspace_session("Session 1".to_string(), "/home/user".to_string());
        assert!(!session_id.is_empty());

        let sessions = engine.list_workspace_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "Session 1");

        let tab_id = engine
            .add_workspace_tab(session_id.clone(), "Tab 2".to_string(), "/var/log".to_string())
            .expect("Must add tab");
        assert!(!tab_id.is_empty());

        let switched = engine.switch_workspace_tab(session_id.clone(), tab_id.clone());
        assert!(switched.is_ok());

        let navigated = engine.navigate_workspace_tab(session_id.clone(), tab_id.clone(), "/tmp".to_string());
        assert!(navigated.is_ok());

        let closed_tab = engine.close_workspace_tab(session_id.clone(), tab_id);
        assert!(closed_tab.is_ok());

        let closed = engine.close_workspace_session(session_id);
        assert!(closed);
        assert_eq!(engine.list_workspace_sessions().len(), 0);
    }

    #[test]
    fn test_engine_favorites_lifecycle() {
        let tmp = tempdir().expect("Must create tempdir");
        let engine = AppCoreEngine::new(tmp.path().to_string_lossy().to_string())
            .expect("Must initialize engine");

        let fav = engine
            .add_favorite(
                "Downloads".to_string(),
                "file:///Users/me/Downloads".to_string(),
                "folder.fill".to_string(),
                true,
                vec!["quick-access".to_string()],
            )
            .expect("Must add favorite");

        assert_eq!(fav.name, "Downloads");
        assert!(fav.is_pinned);

        let list = engine.list_favorites();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, fav.id);

        let removed = engine.remove_favorite(fav.id).expect("Must remove favorite");
        assert!(removed);
        assert_eq!(engine.list_favorites().len(), 0);
    }

    #[test]
    fn test_engine_transfer_lifecycle() {
        let tmp = tempdir().expect("Must create tempdir");
        let engine = AppCoreEngine::new(tmp.path().to_string_lossy().to_string())
            .expect("Must initialize engine");

        let job_id = engine
            .submit_transfer(
                "High".to_string(),
                "file:///src/file.bin".to_string(),
                "file:///dst/file.bin".to_string(),
                1024 * 1024,
            )
            .expect("Must submit transfer");

        let progress = engine.get_transfer_progress(job_id.clone()).expect("Must find job");
        assert_eq!(progress.total_bytes, 1024 * 1024);
        assert_eq!(progress.transferred_bytes, 0);

        let updated = engine.record_transfer_chunk(job_id.clone(), 512 * 1024).expect("Must record chunk");
        assert_eq!(updated.transferred_bytes, 512 * 1024);

        let pause_res = engine.pause_transfer(job_id.clone());
        assert!(pause_res.is_ok());

        let resume_offset = engine.resume_transfer(job_id.clone()).expect("Must resume");
        assert_eq!(resume_offset, 512 * 1024);

        let cancel_res = engine.cancel_transfer(job_id);
        assert!(cancel_res.is_ok());

        let throughput = engine.get_active_throughput_mb_s();
        assert!(throughput >= 0.0);
    }

    #[test]
    fn test_engine_streaming_transfer_and_finalize() {
        let tmp = tempdir().expect("Must create tempdir");
        let engine = AppCoreEngine::new(tmp.path().to_string_lossy().to_string())
            .expect("Must initialize engine");

        let dest_file = tmp.path().join("streamed_output.bin");
        let dest_uri = format!("file://{}", dest_file.to_str().unwrap());
        let total_bytes: u64 = 128 * 1024;

        let job_id = engine
            .submit_transfer(
                "Normal".to_string(),
                "adb://device/sdcard/data.bin".to_string(),
                dest_uri,
                total_bytes,
            )
            .expect("Must submit transfer");

        // Write chunk 1
        let chunk1 = vec![0x11; 64 * 1024];
        let written1 = engine.write_transfer_chunk(job_id.clone(), chunk1).expect("chunk 1");
        assert_eq!(written1, 64 * 1024);

        // Write chunk 2
        let chunk2 = vec![0x22; 64 * 1024];
        let written2 = engine.write_transfer_chunk(job_id.clone(), chunk2).expect("chunk 2");
        assert_eq!(written2, 128 * 1024);

        // Finalize
        let final_path_str = engine.finalize_transfer(job_id.clone()).expect("finalize");
        assert_eq!(final_path_str, dest_file.to_string_lossy().to_string());
        assert!(dest_file.exists());
        assert_eq!(std::fs::metadata(&dest_file).unwrap().len(), 128 * 1024);

        // Confirm progress reflects completion
        let progress = engine.get_transfer_progress(job_id).expect("must get progress");
        assert_eq!(progress.transferred_bytes, total_bytes);
    }

    #[test]
    fn test_engine_sync_mtp_scan() {
        let tmp = tempdir().expect("Must create tempdir");
        let engine = AppCoreEngine::new(tmp.path().to_string_lossy().to_string())
            .expect("Must initialize engine");

        let res = engine.scan_mtp_devices();
        assert!(res.is_ok());
    }

    #[test]
    fn test_engine_stateless_adb_error_isolation() {
        let tmp = tempdir().expect("Must create tempdir");
        let engine = AppCoreEngine::new(tmp.path().to_string_lossy().to_string())
            .expect("Must initialize engine");

        // Peripheral ADB failure must return strongly typed error without poisoning engine
        let res = engine.list_adb_devices();
        // Regardless of whether local ADB daemon is running or offline, engine must handle it safely
        assert!(res.is_ok() || matches!(res, Err(UniFFIAppCoreError::AdbError { .. })));

        // Verify engine workspace subsystem remains fully operational
        let session_id = engine.create_workspace_session("Resilience Session".to_string(), "/tmp".to_string());
        assert!(!session_id.is_empty());
        assert_eq!(engine.list_workspace_sessions().len(), 1);
    }

    #[test]
    fn test_engine_stateless_wireless_adb_error_isolation() {
        let tmp = tempdir().expect("Must create tempdir");
        let engine = AppCoreEngine::new(tmp.path().to_string_lossy().to_string())
            .expect("Must initialize engine");

        // Connecting to unreachable port must yield AdbError and never crash the process
        let res = engine.connect_wireless_adb("127.0.0.1".to_string(), Some(1));
        assert!(matches!(res, Err(UniFFIAppCoreError::AdbError { .. })));

        // Verify favorites subsystem remains unaffected
        let fav = engine.add_favorite(
            "Isolated".to_string(),
            "/test".to_string(),
            "folder".to_string(),
            false,
            vec![],
        );
        assert!(fav.is_ok());
        assert_eq!(engine.list_favorites().len(), 1);
    }

    #[test]
    fn test_engine_list_device_directory_offline_error() {
        let tmp = tempdir().expect("Must create tempdir");
        let engine = AppCoreEngine::new(tmp.path().to_string_lossy().to_string())
            .expect("Must initialize engine");

        // Verify offline ADB listing gracefully propagates AdbError without panicking
        let adb_res = engine.list_device_directory(
            "adb://offline-test-device".to_string(),
            "/sdcard".to_string(),
        );
        assert!(matches!(adb_res, Err(UniFFIAppCoreError::AdbError { .. })));

        // Verify nonexistent MTP device listing gracefully propagates UsbError without panicking
        let mtp_res = engine.list_device_directory(
            "mtp://FFFF:FFFF".to_string(),
            "/".to_string(),
        );
        assert!(matches!(mtp_res, Err(UniFFIAppCoreError::UsbError { .. })));
    }
}


