// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Centralized error definitions for the TTZip Application Core microkernel.

use thiserror::Error;

/// Granular classification of physical device, peripheral transport, and protocol failure kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum DeviceErrorKind {
    /// Device connection is offline, severed, or unreachable.
    Offline = 0,
    /// Device requires RSA authorization key confirmation from the user.
    Unauthorized = 1,
    /// Connection, packet handshake, or bulk transfer timed out.
    Timeout = 2,
    /// Wire protocol framing, magic corruption, or invalid packet status.
    Protocol = 3,
    /// Low-level USB enumeration, I/O, or pipe communication failure.
    Transport = 4,
    /// Other unclassified peripheral failure.
    Other = 5,
}

/// Top-level error enumeration across device I/O, transfers, workspace, and plugins.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AppCoreError {
    /// Low-level USB enumeration or communication failure.
    #[error("USB transport error: {reason}")]
    Usb {
        kind: DeviceErrorKind,
        reason: String,
    },

    /// MTP 1.1 protocol violation or device response error.
    #[error("MTP protocol error: code=0x{code:04X}, message={message}")]
    Mtp { code: u16, message: String },

    /// ADB transport or authorization handshake failure.
    #[error("ADB error: {reason}")]
    Adb {
        kind: DeviceErrorKind,
        reason: String,
    },

    /// Network socket connection or packet streaming error.
    #[error("Network I/O error: {reason}")]
    Network { reason: String },

    /// File transfer scheduling, verification, or WAL checkpoint failure.
    #[error("Transfer error: {reason}")]
    Transfer { reason: String },

    /// Workspace session, tab navigation, or favorites registry failure.
    #[error("Workspace error: {reason}")]
    Workspace { reason: String },

    /// Cryptographic validation or capability sandbox security violation.
    #[error("Security violation: {reason}")]
    SecurityViolation { reason: String },

    /// Invalid state machine transition or operation called out of order.
    #[error("Invalid state: {reason}")]
    InvalidState { reason: String },

    /// General filesystem or I/O failure.
    #[error("I/O error: {reason}")]
    Io { reason: String },
}

impl AppCoreError {
    /// Helper to construct a USB error with a specific classification kind.
    #[inline]
    #[must_use]
    pub fn usb(kind: DeviceErrorKind, reason: impl Into<String>) -> Self {
        Self::Usb {
            kind,
            reason: reason.into(),
        }
    }

    /// Helper to construct an ADB error with a specific classification kind.
    #[inline]
    #[must_use]
    pub fn adb(kind: DeviceErrorKind, reason: impl Into<String>) -> Self {
        Self::Adb {
            kind,
            reason: reason.into(),
        }
    }

    /// Returns true if the error represents an unauthorized ADB connection
    /// (e.g. user hasn't accepted USB debugging prompt on the Android device).
    #[inline]
    #[must_use]
    pub fn is_adb_unauthorized(&self) -> bool {
        matches!(self, Self::Adb { kind: DeviceErrorKind::Unauthorized, .. })
    }

    /// Returns true if the error represents an offline device.
    #[inline]
    #[must_use]
    pub fn is_adb_offline(&self) -> bool {
        matches!(self, Self::Adb { kind: DeviceErrorKind::Offline, .. })
    }

    /// Returns true if the error represents an offline device or severed connection.
    #[inline]
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        matches!(
            self,
            Self::Adb { kind: DeviceErrorKind::Offline, .. }
                | Self::Usb { kind: DeviceErrorKind::Offline, .. }
        )
    }

    /// Returns true if the error represents a connection, transfer, or device timeout.
    #[inline]
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        match self {
            Self::Adb { kind: DeviceErrorKind::Timeout, .. }
            | Self::Usb { kind: DeviceErrorKind::Timeout, .. } => true,
            Self::Network { reason } | Self::Transfer { reason } => {
                let lower = reason.to_ascii_lowercase();
                lower.contains("timeout") || lower.contains("timed out")
            }
            _ => false,
        }
    }
}

impl From<std::io::Error> for AppCoreError {
    #[inline]
    fn from(err: std::io::Error) -> Self {
        Self::Io {
            reason: err.to_string(),
        }
    }
}

impl From<AppCoreError> for std::io::Error {
    #[inline]
    fn from(err: AppCoreError) -> Self {
        let kind = match &err {
            AppCoreError::Adb { .. } if err.is_adb_offline() => std::io::ErrorKind::NotConnected,
            AppCoreError::Usb { .. } if err.is_disconnected() => std::io::ErrorKind::NotConnected,
            AppCoreError::Adb { .. } if err.is_adb_unauthorized() => {
                std::io::ErrorKind::PermissionDenied
            }
            AppCoreError::Network { .. }
            | AppCoreError::Adb { .. }
            | AppCoreError::Usb { .. }
            | AppCoreError::Transfer { .. }
                if err.is_timeout() =>
            {
                std::io::ErrorKind::TimedOut
            }
            AppCoreError::SecurityViolation { .. } => std::io::ErrorKind::PermissionDenied,
            AppCoreError::InvalidState { .. } => {
                std::io::ErrorKind::InvalidInput
            }
            _ => std::io::ErrorKind::Other,
        };
        std::io::Error::new(kind, err)
    }
}

// Static compile-time barrier enforcing Send + Sync, std::error::Error, From conversions, and L1 cache line limits.
const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    const fn assert_std_error<T: std::error::Error + Send + Sync + 'static>() {}
    const fn assert_from<T, U>()
    where
        U: From<T>,
    {
    }

    assert_send_sync::<AppCoreError>();
    assert_std_error::<AppCoreError>();
    assert_from::<std::io::Error, AppCoreError>();
    assert_from::<AppCoreError, std::io::Error>();

    // Memory layout guarantee: ensure error type fits within a single 64-byte CPU cache line
    assert!(std::mem::size_of::<AppCoreError>() <= 64);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_core_error_downcast_and_io_conversions() {
        let app_err = AppCoreError::SecurityViolation {
            reason: "Access denied to unauthorized resource".to_string(),
        };
        let io_err: std::io::Error = app_err.clone().into();
        assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);

        // Verify downcast preservation (zero type erasure)
        let downcasted = io_err
            .get_ref()
            .and_then(|e| e.downcast_ref::<AppCoreError>());
        assert_eq!(downcasted, Some(&app_err));

        // Timeout conversion
        let timeout_err = AppCoreError::Transfer {
            reason: "Transfer job timed out after 1500ms".to_string(),
        };
        assert!(timeout_err.is_timeout());
        let io_timeout: std::io::Error = timeout_err.clone().into();
        assert_eq!(io_timeout.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(
            io_timeout
                .get_ref()
                .and_then(|e| e.downcast_ref::<AppCoreError>()),
            Some(&timeout_err)
        );

        // Disconnected check
        let offline_err = AppCoreError::Usb {
            kind: DeviceErrorKind::Offline,
            reason: "Device disconnected".to_string(),
        };
        assert!(offline_err.is_disconnected());
        let io_offline: std::io::Error = offline_err.clone().into();
        assert_eq!(io_offline.kind(), std::io::ErrorKind::NotConnected);
        assert_eq!(
            io_offline
                .get_ref()
                .and_then(|e| e.downcast_ref::<AppCoreError>()),
            Some(&offline_err)
        );

        // Unauthorized check
        let unauth_err = AppCoreError::adb(
            DeviceErrorKind::Unauthorized,
            "RSA confirmation rejected",
        );
        assert!(unauth_err.is_adb_unauthorized());
        let io_unauth: std::io::Error = unauth_err.clone().into();
        assert_eq!(io_unauth.kind(), std::io::ErrorKind::PermissionDenied);

        // From std::io::Error
        let raw_io = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let elevated: AppCoreError = raw_io.into();
        assert!(matches!(elevated, AppCoreError::Io { .. }));
    }
}

