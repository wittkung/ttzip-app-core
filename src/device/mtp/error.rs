// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Strongly-typed errors, cause-chain iteration, and standard I/O conversions for MTP.

use crate::device::mtp::codes::resp;
use std::time::Duration;

/// Returns true if the provided standard I/O error represents an abrupt physical disconnection,
/// pipe severance, or unreachable endpoint.
#[inline]
#[must_use]
pub fn is_io_disconnected(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::NetworkUnreachable
            | std::io::ErrorKind::HostUnreachable
            | std::io::ErrorKind::NetworkDown
            | std::io::ErrorKind::WriteZero
            | std::io::ErrorKind::UnexpectedEof
    )
}

#[inline]
fn is_reason_disconnected(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    lower.contains("no device")
        || lower.contains("not found")
        || lower.contains("disconnected")
        || lower.contains("pipe")
}

/// Unified top-level error enum covering all failure scenarios across the MTP protocol subsystem.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum MtpError {
    /// Low-level USB bulk transfer or hardware enumeration failure.
    #[error("USB transport error: {0}")]
    Usb(String),

    /// USB bulk transfer or communication timed out.
    #[error("USB transfer timed out after {timeout:?}: {reason}")]
    Timeout { timeout: Duration, reason: String },

    /// Protocol packet framing, underflow, or length mismatch.
    #[error("MTP packet framing error: {0}")]
    Framing(String),

    /// Protocol transaction status code error returned by the MTP responder.
    #[error("MTP protocol error (code: 0x{code:04X}): {message}")]
    Protocol { code: u16, message: String },

    /// Invalid session lifecycle state transition.
    #[error("MTP invalid state: {0}")]
    InvalidState(String),

    /// Transaction identifier mismatch.
    #[error("MTP transaction mismatch: expected {expected}, received {received}")]
    TransactionMismatch { expected: u32, received: u32 },

    /// Hardware signal termination (e.g., SIGSEGV, SIGBUS) caused by hardware fault or unplug.
    #[error("Hardware signal terminated worker process (signal: {0})")]
    HardwareSignalTerminated(i32),

    /// Standard I/O socket or USB streaming error.
    #[error("MTP I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl MtpError {
    /// Borrows the underlying standard I/O error if this failure was caused by I/O.
    #[inline]
    #[must_use]
    pub fn as_io_error(&self) -> Option<&std::io::Error> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }

    /// Mutably borrows the underlying standard I/O error if this failure was caused by I/O.
    #[inline]
    #[must_use]
    pub fn as_io_error_mut(&mut self) -> Option<&mut std::io::Error> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }

    /// Extracts the underlying standard I/O error by value if this failure was caused by I/O.
    #[inline]
    #[must_use]
    pub fn into_io_error(self) -> Option<std::io::Error> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }

    /// Returns true if this error was caused by an underlying I/O failure.
    #[inline]
    #[must_use]
    pub fn is_io(&self) -> bool {
        self.as_io_error().is_some()
    }

    /// Returns true if this error represents a transfer or connection timeout.
    #[inline]
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        if matches!(self, Self::Timeout { .. }) {
            return true;
        }
        self.as_io_error()
            .is_some_and(|err| err.kind() == std::io::ErrorKind::TimedOut)
    }

    /// Returns true if the responder indicated that the requested operation or parameter is unsupported.
    #[inline]
    #[must_use]
    pub fn is_not_supported(&self) -> bool {
        matches!(
            self,
            Self::Protocol { code, .. }
                if *code == resp::OPERATION_NOT_SUPPORTED
                    || *code == resp::PARAMETER_NOT_SUPPORTED
                    || *code == resp::SPECIFICATION_BY_FORMAT_UNSUPPORTED
                    || *code == resp::DESTINATION_UNSUPPORTED
                    || *code == resp::DEVICE_PROP_NOT_SUPPORTED
        )
    }

    /// Returns true if access to the object or store was denied.
    #[inline]
    #[must_use]
    pub fn is_access_denied(&self) -> bool {
        matches!(
            self,
            Self::Protocol { code, .. }
                if *code == resp::ACCESS_DENIED
                    || *code == resp::STORE_READ_ONLY
                    || *code == resp::OBJECT_WRITE_PROTECTED
        )
    }

    /// Returns true if the storage medium has reached capacity.
    #[inline]
    #[must_use]
    pub fn is_store_full(&self) -> bool {
        matches!(self, Self::Protocol { code, .. } if *code == resp::STORE_FULL)
    }

    /// Returns true if the target device is currently busy.
    #[inline]
    #[must_use]
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Protocol { code, .. } if *code == resp::DEVICE_BUSY)
    }

    /// Returns true if this error represents a physical device disconnection or unplug event.
    #[inline]
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        match self {
            Self::Usb(reason) => is_reason_disconnected(reason),
            Self::Io(err) => is_io_disconnected(err),
            Self::HardwareSignalTerminated(_) => true,
            _ => false,
        }
    }

    /// Returns a zero-allocation iterator over the causal chain of errors.
    #[inline]
    #[must_use]
    pub fn chain(&self) -> ErrorChain<'_> {
        ErrorChain::new(self)
    }

    /// Downcasts by reference to a specific error type along the causal source chain.
    #[inline]
    #[must_use]
    pub fn downcast_ref<T: std::error::Error + 'static>(&self) -> Option<&T> {
        self.chain().find_map(|err| err.downcast_ref::<T>())
    }

    /// Returns true if this error or any error in its causal chain matches the target type `T`.
    #[inline]
    #[must_use]
    pub fn is<T: std::error::Error + 'static>(&self) -> bool {
        self.downcast_ref::<T>().is_some()
    }

    /// Navigates the causal chain to locate the bottom-most root cause.
    #[inline]
    #[must_use]
    pub fn root_cause(&self) -> &(dyn std::error::Error + 'static) {
        let mut current: &(dyn std::error::Error + 'static) = self;
        while let Some(cause) = current.source() {
            current = cause;
        }
        current
    }
}

pub use ttzip_types::ErrorChain;

impl<'a> IntoIterator for &'a MtpError {
    type Item = &'a (dyn std::error::Error + 'static);
    type IntoIter = ErrorChain<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.chain()
    }
}

impl From<MtpError> for std::io::Error {
    #[inline]
    fn from(err: MtpError) -> Self {
        match err {
            MtpError::Io(io_err) => io_err,
            err @ MtpError::Timeout { .. } => {
                std::io::Error::new(std::io::ErrorKind::TimedOut, err)
            }
            err @ MtpError::Usb(_) => {
                let kind = if err.is_disconnected() {
                    std::io::ErrorKind::NotConnected
                } else {
                    std::io::ErrorKind::Other
                };
                std::io::Error::new(kind, err)
            }
            err @ (MtpError::Framing(_) | MtpError::TransactionMismatch { .. }) => {
                std::io::Error::new(std::io::ErrorKind::InvalidData, err)
            }
            err @ MtpError::InvalidState(_) => {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, err)
            }
            MtpError::HardwareSignalTerminated(sig) => std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                format!("Hardware signal: {sig}"),
            ),
            err @ MtpError::Protocol { code, .. } => {
                let kind = match code {
                    resp::OPERATION_NOT_SUPPORTED
                    | resp::PARAMETER_NOT_SUPPORTED
                    | resp::SPECIFICATION_BY_FORMAT_UNSUPPORTED
                    | resp::DESTINATION_UNSUPPORTED
                    | resp::DEVICE_PROP_NOT_SUPPORTED => std::io::ErrorKind::Unsupported,
                    resp::ACCESS_DENIED | resp::STORE_READ_ONLY | resp::OBJECT_WRITE_PROTECTED => {
                        std::io::ErrorKind::PermissionDenied
                    }
                    resp::INVALID_OBJECT_HANDLE
                    | resp::INVALID_STORAGE_ID
                    | resp::INVALID_PARENT_OBJECT
                    | resp::NO_THUMBNAIL_PRESENT
                    | resp::NO_VALID_OBJECT_INFO => std::io::ErrorKind::NotFound,
                    resp::SESSION_NOT_OPEN | resp::STORE_NOT_AVAILABLE => {
                        std::io::ErrorKind::NotConnected
                    }
                    resp::STORE_FULL => std::io::ErrorKind::StorageFull,
                    resp::DEVICE_BUSY => std::io::ErrorKind::ResourceBusy,
                    resp::INCOMPLETE_TRANSFER => std::io::ErrorKind::UnexpectedEof,
                    resp::INVALID_TRANSACTION_ID
                    | resp::INVALID_OBJECT_FORMAT_CODE
                    | resp::INVALID_CODE_FORMAT
                    | resp::INVALID_DEVICE_PROP_FORMAT
                    | resp::INVALID_DEVICE_PROP_VALUE
                    | resp::INVALID_PARAMETER => std::io::ErrorKind::InvalidInput,
                    resp::TRANSACTION_CANCELLED => std::io::ErrorKind::Interrupted,
                    _ => std::io::ErrorKind::Other,
                };
                std::io::Error::new(kind, err)
            }
        }
    }
}

impl From<MtpError> for crate::error::AppCoreError {
    fn from(err: MtpError) -> Self {
        match err {
            MtpError::Timeout { timeout, reason } => crate::error::AppCoreError::usb(
                crate::error::DeviceErrorKind::Timeout,
                format!("Timeout: USB transfer timed out after {timeout:?}: {reason}"),
            ),
            MtpError::Usb(reason) if is_reason_disconnected(&reason) => {
                crate::error::AppCoreError::usb(
                    crate::error::DeviceErrorKind::Offline,
                    format!("Offline: device is unreachable or disconnected: {reason}"),
                )
            }
            MtpError::Usb(reason) => {
                crate::error::AppCoreError::usb(crate::error::DeviceErrorKind::Transport, reason)
            }
            MtpError::Io(err) if is_io_disconnected(&err) => crate::error::AppCoreError::usb(
                crate::error::DeviceErrorKind::Offline,
                format!("Offline: device is unreachable or disconnected ({err})"),
            ),
            MtpError::Io(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                crate::error::AppCoreError::usb(
                    crate::error::DeviceErrorKind::Timeout,
                    format!("Timeout: USB transfer timed out ({err})"),
                )
            }
            MtpError::Io(err) => crate::error::AppCoreError::usb(
                crate::error::DeviceErrorKind::Transport,
                format!("USB I/O failure: {err}"),
            ),
            MtpError::Protocol { code, message } => {
                crate::error::AppCoreError::Mtp { code, message }
            }
            MtpError::InvalidState(reason) => crate::error::AppCoreError::InvalidState { reason },
            MtpError::HardwareSignalTerminated(sig) => crate::error::AppCoreError::usb(
                crate::error::DeviceErrorKind::Offline,
                format!("Offline: hardware signal terminated worker process (signal: {sig})"),
            ),
            other => crate::error::AppCoreError::Mtp {
                code: 0,
                message: other.to_string(),
            },
        }
    }
}

/// Unified Result type for MTP subsystem operations.
pub type MtpResult<T> = std::result::Result<T, MtpError>;

// Compile-time static assertions ensuring thread-safety, standard error traits, and conversions
const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    const fn assert_std_error<T: std::error::Error + Send + Sync + 'static>() {}
    const fn assert_from<T, U>()
    where
        U: From<T>,
    {
    }

    assert_send_sync::<MtpError>();
    assert_std_error::<MtpError>();

    assert_from::<std::io::Error, MtpError>();
    assert_from::<MtpError, std::io::Error>();
    assert_from::<MtpError, crate::error::AppCoreError>();

    assert!(std::mem::size_of::<MtpError>() <= 64);
    assert!(std::mem::align_of::<MtpError>() <= 8);
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::mtp::driver::DEFAULT_USB_TIMEOUT;
    use crate::error::AppCoreError;

    #[test]
    fn test_mtp_reference_triplet_and_downcast_preservation() {
        let raw_io = std::io::Error::from(std::io::ErrorKind::AddrInUse);
        let mut mtp_io: MtpError = raw_io.into();
        assert!(mtp_io.is_io());
        assert_eq!(
            mtp_io.as_io_error().map(|e| e.kind()),
            Some(std::io::ErrorKind::AddrInUse)
        );
        assert_eq!(
            mtp_io.as_io_error_mut().map(|e| e.kind()),
            Some(std::io::ErrorKind::AddrInUse)
        );
        let extracted_io = mtp_io.into_io_error().expect("Must extract raw io::Error");
        assert_eq!(extracted_io.kind(), std::io::ErrorKind::AddrInUse);

        let mut non_io = MtpError::InvalidState("Engine not initialized".to_string());
        assert!(!non_io.is_io());
        assert!(non_io.as_io_error().is_none());
        assert!(non_io.as_io_error_mut().is_none());
        assert!(non_io.into_io_error().is_none());

        let timeout_err = MtpError::Timeout {
            timeout: DEFAULT_USB_TIMEOUT,
            reason: "read timeout".to_string(),
        };
        let timeout_io: std::io::Error = timeout_err.into();
        assert_eq!(timeout_io.kind(), std::io::ErrorKind::TimedOut);
        assert!(timeout_io
            .get_ref()
            .and_then(|e| e.downcast_ref::<MtpError>())
            .is_some());

        let disconnected_err = MtpError::Usb("pipe broken: disconnected".to_string());
        let disconnected_io: std::io::Error = disconnected_err.into();
        assert_eq!(disconnected_io.kind(), std::io::ErrorKind::NotConnected);
        assert!(disconnected_io
            .get_ref()
            .and_then(|e| e.downcast_ref::<MtpError>())
            .is_some());

        let generic_usb_err = MtpError::Usb("bulk transfer stall".to_string());
        let generic_usb_io: std::io::Error = generic_usb_err.into();
        assert_eq!(generic_usb_io.kind(), std::io::ErrorKind::Other);
        assert!(generic_usb_io
            .get_ref()
            .and_then(|e| e.downcast_ref::<MtpError>())
            .is_some());

        let framing_err = MtpError::Framing("bad header size".to_string());
        let framing_io: std::io::Error = framing_err.into();
        assert_eq!(framing_io.kind(), std::io::ErrorKind::InvalidData);
        assert!(framing_io
            .get_ref()
            .and_then(|e| e.downcast_ref::<MtpError>())
            .is_some());

        let mismatch_err = MtpError::TransactionMismatch {
            expected: 42,
            received: 43,
        };
        let mismatch_io: std::io::Error = mismatch_err.into();
        assert_eq!(mismatch_io.kind(), std::io::ErrorKind::InvalidData);
        assert!(mismatch_io
            .get_ref()
            .and_then(|e| e.downcast_ref::<MtpError>())
            .is_some());

        let state_err = MtpError::InvalidState("session closed".to_string());
        let state_io: std::io::Error = state_err.into();
        assert_eq!(state_io.kind(), std::io::ErrorKind::InvalidInput);
        assert!(state_io
            .get_ref()
            .and_then(|e| e.downcast_ref::<MtpError>())
            .is_some());

        let test_cases = [
            (
                resp::OPERATION_NOT_SUPPORTED,
                std::io::ErrorKind::Unsupported,
            ),
            (resp::ACCESS_DENIED, std::io::ErrorKind::PermissionDenied),
            (resp::INVALID_OBJECT_HANDLE, std::io::ErrorKind::NotFound),
            (resp::SESSION_NOT_OPEN, std::io::ErrorKind::NotConnected),
            (resp::STORE_FULL, std::io::ErrorKind::StorageFull),
            (resp::DEVICE_BUSY, std::io::ErrorKind::ResourceBusy),
            (resp::INCOMPLETE_TRANSFER, std::io::ErrorKind::UnexpectedEof),
            (resp::INVALID_PARAMETER, std::io::ErrorKind::InvalidInput),
            (resp::TRANSACTION_CANCELLED, std::io::ErrorKind::Interrupted),
            (0xFFFF, std::io::ErrorKind::Other),
        ];

        for (code, expected_kind) in test_cases {
            let proto_err = MtpError::Protocol {
                code,
                message: "status code test".to_string(),
            };
            let io_err: std::io::Error = proto_err.into();
            assert_eq!(io_err.kind(), expected_kind);
            assert!(io_err
                .get_ref()
                .and_then(|e| e.downcast_ref::<MtpError>())
                .is_some());
        }

        let raw_pass_through: std::io::Error =
            MtpError::Io(std::io::Error::from(std::io::ErrorKind::ConnectionAborted)).into();
        assert_eq!(
            raw_pass_through.kind(),
            std::io::ErrorKind::ConnectionAborted
        );
    }

    #[test]
    fn test_mtp_unified_error_elevation() {
        let timeout_usb = MtpError::Timeout {
            timeout: Duration::from_millis(500),
            reason: "bulk read stalled".to_string(),
        };
        let app_timeout: AppCoreError = timeout_usb.into();
        match app_timeout {
            AppCoreError::Usb { kind, reason } => {
                assert_eq!(kind, crate::error::DeviceErrorKind::Timeout);
                assert!(reason.starts_with("Timeout: USB transfer timed out"));
            }
            _ => panic!("Expected AppCoreError::Usb, got {:?}", app_timeout),
        }

        let disconnected_usb = MtpError::Usb("Device not found on USB bus".to_string());
        let app_disconnected: AppCoreError = disconnected_usb.into();
        match app_disconnected {
            AppCoreError::Usb { kind, reason } => {
                assert_eq!(kind, crate::error::DeviceErrorKind::Offline);
                assert!(reason.starts_with("Offline: device is unreachable or disconnected"));
            }
            _ => panic!("Expected AppCoreError::Usb, got {:?}", app_disconnected),
        }

        let normal_usb = MtpError::Usb("Endpoint stall".to_string());
        let app_normal: AppCoreError = normal_usb.into();
        match app_normal {
            AppCoreError::Usb { kind, reason } => {
                assert_eq!(kind, crate::error::DeviceErrorKind::Transport);
                assert_eq!(reason, "Endpoint stall");
            }
            _ => panic!("Expected AppCoreError::Usb, got {:?}", app_normal),
        }

        let timeout_io: MtpError = std::io::Error::from(std::io::ErrorKind::TimedOut).into();
        let app_timeout_io: AppCoreError = timeout_io.into();
        match app_timeout_io {
            AppCoreError::Usb { kind, reason } => {
                assert_eq!(kind, crate::error::DeviceErrorKind::Timeout);
                assert!(reason.starts_with("Timeout: USB transfer timed out"));
            }
            _ => panic!("Expected AppCoreError::Usb, got {:?}", app_timeout_io),
        }

        let other_io: MtpError = std::io::Error::from(std::io::ErrorKind::PermissionDenied).into();
        let app_other_io: AppCoreError = other_io.into();
        match app_other_io {
            AppCoreError::Usb { kind, reason } => {
                assert_eq!(kind, crate::error::DeviceErrorKind::Transport);
                assert!(reason.starts_with("USB I/O failure"));
            }
            _ => panic!("Expected AppCoreError::Usb, got {:?}", app_other_io),
        }

        let sig_err = MtpError::HardwareSignalTerminated(11);
        assert!(sig_err.is_disconnected());
        let sig_io: std::io::Error = sig_err.into();
        assert_eq!(sig_io.kind(), std::io::ErrorKind::ConnectionReset);
        let app_sig: AppCoreError = MtpError::HardwareSignalTerminated(11).into();
        match app_sig {
            AppCoreError::Usb { kind, reason } => {
                assert_eq!(kind, crate::error::DeviceErrorKind::Offline);
                assert!(reason.contains("signal: 11"));
            }
            _ => panic!("Expected AppCoreError::Usb, got {:?}", app_sig),
        }
    }

    #[test]
    fn test_error_chain_iteration_and_fused() {
        let inner_io = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "connection reset");
        let mtp_err = MtpError::Io(inner_io);

        let mut chain = mtp_err.chain();
        let first = chain.next().expect("first item must exist");
        assert!(first.is::<MtpError>());
        let second = chain.next().expect("second item must exist");
        assert!(second.is::<std::io::Error>());
        assert!(chain.next().is_none());
        assert!(chain.next().is_none());

        assert!(mtp_err.is::<std::io::Error>());
        assert!(mtp_err.is::<MtpError>());
        assert!(!mtp_err.is::<std::fmt::Error>());

        let downcasted: &std::io::Error =
            mtp_err.downcast_ref().expect("must downcast to io::Error");
        assert_eq!(downcasted.kind(), std::io::ErrorKind::ConnectionReset);

        let root = mtp_err.root_cause();
        assert!(root.is::<std::io::Error>());

        let empty = ErrorChain::empty();
        assert_eq!(empty.into_iter().count(), 0);
    }
}
