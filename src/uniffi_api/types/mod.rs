// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Strongly typed records and error enumerations exported to multi-language bindings via UniFFI.

use thiserror::Error;

/// UniFFI-compatible top-level error enum.
#[derive(Debug, Clone, PartialEq, Eq, Error, uniffi::Error)]
pub enum UniFFIAppCoreError {
    #[error("USB transport error: {reason}")]
    UsbError { reason: String },

    #[error("MTP protocol error: code=0x{code:04X}, message={message}")]
    MtpError { code: u16, message: String },

    #[error("ADB error: {reason}")]
    AdbError { reason: String },

    #[error("Network error: {reason}")]
    NetworkError { reason: String },

    #[error("Transfer error: {reason}")]
    TransferError { reason: String },

    #[error("Workspace error: {reason}")]
    WorkspaceError { reason: String },

    #[error("Security violation: {reason}")]
    SecurityViolation { reason: String },

    #[error("Invalid state: {reason}")]
    InvalidState { reason: String },

    #[error("I/O error: {reason}")]
    IoError { reason: String },
}

impl From<crate::error::AppCoreError> for UniFFIAppCoreError {
    fn from(err: crate::error::AppCoreError) -> Self {
        match err {
            crate::error::AppCoreError::Usb { reason, .. } => Self::UsbError { reason },
            crate::error::AppCoreError::Mtp { code, message } => Self::MtpError { code, message },
            crate::error::AppCoreError::Adb { reason, .. } => Self::AdbError { reason },
            crate::error::AppCoreError::Network { reason } => Self::NetworkError { reason },
            crate::error::AppCoreError::Transfer { reason } => Self::TransferError { reason },
            crate::error::AppCoreError::Workspace { reason } => Self::WorkspaceError { reason },
            crate::error::AppCoreError::SecurityViolation { reason } => Self::SecurityViolation { reason },
            crate::error::AppCoreError::InvalidState { reason } => Self::InvalidState { reason },
            crate::error::AppCoreError::Io { reason } => Self::IoError { reason },
        }
    }
}

impl From<crate::device::adb::transport::AdbTransportError> for UniFFIAppCoreError {
    fn from(err: crate::device::adb::transport::AdbTransportError) -> Self {
        let core_err: crate::error::AppCoreError = err.into();
        core_err.into()
    }
}

/// Description of a connected physical (MTP) or wireless/daemon (ADB) device.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UniFFIDeviceDescriptor {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub transport_type: String,
    pub state: String,
}

/// Canonical unified directory entry description on a physical or logical device filesystem.
pub type UniFFIDeviceEntry = crate::device::entry::DeviceEntry;

use crate::transfer::job::TransferState;

/// Lifecycle state of an active file transfer operation.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum UniFFITransferState {
    Pending,
    Running,
    Paused,
    Completed,
    Failed { reason: String },
    Cancelled,
}

impl From<TransferState> for UniFFITransferState {
    #[inline]
    fn from(state: TransferState) -> Self {
        match state {
            TransferState::Pending => Self::Pending,
            TransferState::Running => Self::Running,
            TransferState::Paused => Self::Paused,
            TransferState::Completed => Self::Completed,
            TransferState::Failed { reason } => Self::Failed { reason },
            TransferState::Cancelled => Self::Cancelled,
        }
    }
}

impl From<&TransferState> for UniFFITransferState {
    #[inline]
    fn from(state: &TransferState) -> Self {
        match state {
            TransferState::Pending => Self::Pending,
            TransferState::Running => Self::Running,
            TransferState::Paused => Self::Paused,
            TransferState::Completed => Self::Completed,
            TransferState::Failed { reason } => Self::Failed {
                reason: reason.clone(),
            },
            TransferState::Cancelled => Self::Cancelled,
        }
    }
}

/// Real-time progress and status of a file transfer task.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct UniFFITransferJobInfo {
    pub id: String,
    pub source_uri: String,
    pub destination_uri: String,
    pub priority: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub state: UniFFITransferState,
    pub current_speed_mb_s: f64,
    pub progress_ratio: f64,
}

/// Description of an individual workspace tab.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UniFFIWorkspaceTab {
    pub id: String,
    pub title: String,
    pub current_uri: String,
    pub view_mode: String,
    pub sort_order: String,
    pub is_pinned: bool,
}

/// Description of a multi-tab workspace session.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UniFFIWorkspaceSession {
    pub id: String,
    pub name: String,
    pub active_tab_id: String,
    pub tabs: Vec<UniFFIWorkspaceTab>,
    pub layout: String,
}

/// Standardized bookmark/favorite entry.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UniFFIFavoriteItem {
    pub id: String,
    pub name: String,
    pub uri: String,
    pub icon_hint: String,
    pub is_pinned: bool,
    pub tags: Vec<String>,
    pub order_index: u32,
}
