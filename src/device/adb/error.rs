// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Unified error models, causal chain iterator, standard I/O conversions, and error code extractors.

use super::{
    sync::AdbSyncError,
    transport::AdbTransportError,
};

#[cfg(test)]
use super::transport::DEFAULT_CONNECT_TIMEOUT;

/// Unified top-level error enum covering all failure scenarios across the ADB protocol subsystem.
#[derive(Debug, thiserror::Error)]
pub enum AdbError {
    /// Socket network errors, timeouts, or daemon protocol rejections.
    #[error("ADB transport error: {0}")]
    Transport(#[from] AdbTransportError),

    /// Remote filesystem synchronization protocol errors.
    #[error("ADB sync error: {0}")]
    Sync(#[from] AdbSyncError),

    /// Standard I/O socket errors.
    #[error("ADB I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub use ttzip_types::ErrorChain;

impl<'a> IntoIterator for &'a AdbError {
    type Item = &'a (dyn std::error::Error + 'static);
    type IntoIter = ErrorChain<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.chain()
    }
}

/// A specialized [`Result`] type for ADB protocol and client operations.
pub type AdbResult<T> = Result<T, AdbError>;

#[inline]
pub(crate) const fn transport_error_code(err: &AdbTransportError) -> &'static str {
    match err {
        AdbTransportError::Timeout(_) => "TIMED_OUT",
        AdbTransportError::ConnectionFailed(_) => "CONNECTION_FAILED",
        AdbTransportError::DaemonRejected(_) => "DAEMON_REJECTED",
        AdbTransportError::Unauthorized => "UNAUTHORIZED",
        AdbTransportError::DeviceOffline => "DEVICE_OFFLINE",
        AdbTransportError::Io(_) => "IO_ERROR",
        AdbTransportError::Protocol(_) => "PROTOCOL_ERROR",
    }
}

#[inline]
pub(crate) const fn sync_error_code(err: &AdbSyncError) -> &'static str {
    match err {
        AdbSyncError::BufferTooSmall { .. } => "BUFFER_TOO_SMALL",
        AdbSyncError::PacketUnderflow { .. } => "PACKET_UNDERFLOW",
        AdbSyncError::PayloadTooLarge { .. } => "PAYLOAD_TOO_LARGE",
        AdbSyncError::UnexpectedResponse { .. } => "SYNC_UNEXPECTED_RESPONSE",
        AdbSyncError::RemoteFailure(_) => "SYNC_REMOTE_FAILURE",
        AdbSyncError::Io(_) => "IO_ERROR",
    }
}

/// Inspects an I/O error to determine if it indicates physical disconnection or loss of connection.
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

impl AdbError {
    /// Borrows the underlying standard I/O error if this failure was caused by socket I/O.
    #[inline]
    #[must_use]
    pub fn as_io_error(&self) -> Option<&std::io::Error> {
        match self {
            Self::Io(err)
            | Self::Transport(AdbTransportError::Io(err))
            | Self::Sync(AdbSyncError::Io(err)) => Some(err),
            _ => None,
        }
    }

    /// Mutably borrows the underlying standard I/O error if this failure was caused by socket I/O.
    #[inline]
    #[must_use]
    pub fn as_io_error_mut(&mut self) -> Option<&mut std::io::Error> {
        match self {
            Self::Io(err)
            | Self::Transport(AdbTransportError::Io(err))
            | Self::Sync(AdbSyncError::Io(err)) => Some(err),
            _ => None,
        }
    }

    /// Extracts the underlying standard I/O error if this failure was caused by socket I/O.
    #[inline]
    #[must_use]
    pub fn into_io_error(self) -> Option<std::io::Error> {
        match self {
            Self::Io(err)
            | Self::Transport(AdbTransportError::Io(err))
            | Self::Sync(AdbSyncError::Io(err)) => Some(err),
            _ => None,
        }
    }

    /// Returns `true` if this error is directly or transitively caused by standard socket I/O.
    #[inline]
    #[must_use]
    pub fn is_io(&self) -> bool {
        self.as_io_error().is_some()
    }

    /// Returns `true` if the device is unauthorized.
    #[inline]
    #[must_use]
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Transport(AdbTransportError::Unauthorized))
    }

    /// Returns `true` if the device is offline or unreachable.
    #[inline]
    #[must_use]
    pub fn is_offline(&self) -> bool {
        matches!(self, Self::Transport(AdbTransportError::DeviceOffline))
    }

    /// Returns `true` if this error indicates that a transport or socket connection timed out.
    #[inline]
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        if matches!(self, Self::Transport(AdbTransportError::Timeout(_))) {
            return true;
        }
        self.as_io_error().is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
    }

    /// Returns `true` if this error indicates that the device or socket disconnected.
    #[inline]
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.is_offline() || self.as_io_error().is_some_and(is_io_disconnected)
    }

    /// Returns an iterator over the causal source chain of this error, beginning with `self`.
    #[inline]
    #[must_use]
    pub fn chain(&self) -> ErrorChain<'_> {
        ErrorChain::new(self)
    }

    /// Downcasts this error or any underlying cause in its causal chain to concrete type `T`.
    #[inline]
    #[must_use]
    pub fn downcast_ref<T: std::error::Error + 'static>(&self) -> Option<&T> {
        self.chain().find_map(|err| err.downcast_ref::<T>())
    }

    /// Returns `true` if this error or any underlying cause in its causal chain matches type `T`.
    #[inline]
    #[must_use]
    pub fn is<T: std::error::Error + 'static>(&self) -> bool {
        self.downcast_ref::<T>().is_some()
    }

    /// Returns a reference to the deepest underlying root cause in the error chain.
    #[inline]
    #[must_use]
    pub fn root_cause(&self) -> &(dyn std::error::Error + 'static) {
        self.chain().last().unwrap_or(self)
    }

    /// Returns a stable machine-readable diagnostic error code for telemetry, IPC, and observability.
    #[inline]
    #[must_use]
    pub fn error_code(&self) -> &'static str {
        if self.is_unauthorized() {
            return "UNAUTHORIZED";
        }
        if self.is_timeout() {
            return "TIMED_OUT";
        }
        if self.is_offline() {
            return "DEVICE_OFFLINE";
        }
        if self.is_disconnected() {
            return "DISCONNECTED";
        }
        match self {
            Self::Transport(e) => transport_error_code(e),
            Self::Sync(e) => sync_error_code(e),
            Self::Io(_) => "IO_ERROR",
        }
    }
}

impl From<AdbTransportError> for std::io::Error {
    #[inline]
    fn from(err: AdbTransportError) -> Self {
        match err {
            AdbTransportError::Io(io_err) => io_err,
            err @ AdbTransportError::Timeout(_) => {
                std::io::Error::new(std::io::ErrorKind::TimedOut, err)
            }
            err @ AdbTransportError::DeviceOffline => {
                std::io::Error::new(std::io::ErrorKind::NotConnected, err)
            }
            err @ AdbTransportError::Unauthorized => {
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, err)
            }
            err @ (AdbTransportError::ConnectionFailed(_)
            | AdbTransportError::DaemonRejected(_)) => {
                std::io::Error::new(std::io::ErrorKind::ConnectionRefused, err)
            }
            err @ AdbTransportError::Protocol(_) => {
                std::io::Error::new(std::io::ErrorKind::InvalidData, err)
            }
        }
    }
}

impl From<AdbSyncError> for std::io::Error {
    #[inline]
    fn from(err: AdbSyncError) -> Self {
        match err {
            AdbSyncError::Io(io_err) => io_err,
            err @ (AdbSyncError::BufferTooSmall { .. }
            | AdbSyncError::PacketUnderflow { .. }) => {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, err)
            }
            err @ (AdbSyncError::PayloadTooLarge { .. }
            | AdbSyncError::UnexpectedResponse { .. }) => {
                std::io::Error::new(std::io::ErrorKind::InvalidData, err)
            }
            err @ AdbSyncError::RemoteFailure(_) => std::io::Error::other(err),
        }
    }
}

impl From<AdbError> for std::io::Error {
    #[inline]
    fn from(err: AdbError) -> Self {
        match err {
            AdbError::Io(io_err) => io_err,
            AdbError::Transport(e) => e.into(),
            AdbError::Sync(e) => e.into(),
        }
    }
}

impl From<AdbError> for crate::error::AppCoreError {
    fn from(err: AdbError) -> Self {
        let reason = match &err {
            AdbError::Transport(AdbTransportError::Unauthorized) => {
                "Unauthorized: ADB device rejected connection (check device for RSA auth prompt)".to_string()
            }
            _ if err.is_timeout() => match &err {
                AdbError::Transport(AdbTransportError::Timeout(d)) => {
                    format!("Timeout: ADB operation timed out after {d:?}")
                }
                AdbError::Transport(AdbTransportError::Io(io)) => {
                    format!("Timeout: ADB transport operation timed out ({io})")
                }
                AdbError::Sync(AdbSyncError::Io(io)) => {
                    format!("Timeout: ADB sync operation timed out ({io})")
                }
                AdbError::Io(io) => format!("Timeout: ADB operation timed out ({io})"),
                _ => "Timeout: ADB operation timed out".to_string(),
            },
            _ if err.is_disconnected() => match &err {
                AdbError::Transport(AdbTransportError::DeviceOffline) => {
                    "Offline: device is unreachable or disconnected".to_string()
                }
                AdbError::Transport(AdbTransportError::Io(io)) => {
                    format!("Offline: device transport disconnected ({io})")
                }
                AdbError::Sync(AdbSyncError::Io(io)) => {
                    format!("Offline: device sync stream disconnected ({io})")
                }
                AdbError::Io(io) => {
                    format!("Offline: device is unreachable or disconnected ({io})")
                }
                _ => "Offline: device is unreachable or disconnected".to_string(),
            },
            _ if err.is_io() => match &err {
                AdbError::Io(io) => format!("ADB socket I/O failure: {io}"),
                AdbError::Transport(AdbTransportError::Io(io)) => {
                    format!("ADB transport socket I/O failure: {io}")
                }
                AdbError::Sync(AdbSyncError::Io(io)) => {
                    format!("ADB sync socket/stream I/O failure: {io}")
                }
                _ => format!("ADB I/O failure: {err}"),
            },
            _ => err.to_string(),
        };

        let kind = if err.is_unauthorized() {
            crate::error::DeviceErrorKind::Unauthorized
        } else if err.is_disconnected() {
            crate::error::DeviceErrorKind::Offline
        } else if err.is_timeout() {
            crate::error::DeviceErrorKind::Timeout
        } else if err.is_io() {
            crate::error::DeviceErrorKind::Transport
        } else {
            crate::error::DeviceErrorKind::Other
        };

        crate::error::AppCoreError::Adb { kind, reason }
    }
}

impl From<AdbTransportError> for crate::error::AppCoreError {
    #[inline]
    fn from(err: AdbTransportError) -> Self {
        AdbError::Transport(err).into()
    }
}

impl From<AdbSyncError> for crate::error::AppCoreError {
    #[inline]
    fn from(err: AdbSyncError) -> Self {
        AdbError::Sync(err).into()
    }
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    const fn assert_clone<T: Clone>() {}
    const fn assert_copy<T: Copy>() {}
    const fn assert_unpin<T: Unpin>() {}
    const fn assert_default<T: Default>() {}
    const fn assert_std_error<T: std::error::Error + Send + Sync + 'static>() {}
    const fn assert_fused_iter<T: std::iter::FusedIterator>() {}
    const fn assert_into_iter<T: IntoIterator>() {}
    const fn assert_from<T, F>() where T: From<F> {}

    assert_send_sync::<AdbError>();
    assert_send_sync::<AdbTransportError>();
    assert_send_sync::<AdbSyncError>();
    // Note: ErrorChain is not Send because &dyn std::error::Error lacks a Sync bound.

    // Standard Error integration
    assert_std_error::<AdbError>();
    assert_std_error::<AdbTransportError>();
    assert_std_error::<AdbSyncError>();

    // Outbound error conversion to std::io::Error
    assert_from::<std::io::Error, AdbError>();
    assert_from::<std::io::Error, AdbTransportError>();
    assert_from::<std::io::Error, AdbSyncError>();

    // Outbound error conversion to AppCoreError
    assert_from::<crate::error::AppCoreError, AdbError>();
    assert_from::<crate::error::AppCoreError, AdbTransportError>();
    assert_from::<crate::error::AppCoreError, AdbSyncError>();

    // Inbound sub-error and std::io::Error promotion
    assert_from::<AdbError, AdbTransportError>();
    assert_from::<AdbError, AdbSyncError>();
    assert_from::<AdbError, std::io::Error>();

    assert_clone::<ErrorChain<'static>>();
    assert_copy::<ErrorChain<'static>>();
    assert_default::<ErrorChain<'static>>();
    assert_unpin::<ErrorChain<'static>>();

    assert_fused_iter::<ErrorChain<'static>>();
    assert_into_iter::<&'static AdbError>();

    // Memory layout assertions
    assert!(std::mem::size_of::<AdbError>() <= 64);
    assert!(std::mem::align_of::<AdbError>() <= 8);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adb_unified_error_propagation() {
        let direct_transport_err: crate::error::AppCoreError =
            AdbTransportError::DeviceOffline.into();
        assert!(matches!(direct_transport_err, crate::error::AppCoreError::Adb { .. }));

        let direct_sync_err: crate::error::AppCoreError =
            AdbSyncError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).into();
        assert!(matches!(direct_sync_err, crate::error::AppCoreError::Adb { .. }));

        // 1. Unauthorized branch
        let unauth_err = AdbError::Transport(AdbTransportError::Unauthorized);
        assert!(unauth_err.is_unauthorized());
        let app_unauth: crate::error::AppCoreError = unauth_err.into();
        assert!(app_unauth.is_adb_unauthorized());

        let check_reason = |err: AdbError, prefix: &str| {
            if let crate::error::AppCoreError::Adb { reason, .. } = err.into() {
                assert!(reason.starts_with(prefix), "Expected '{prefix}', got '{reason}'");
            } else {
                panic!("Expected AppCoreError::Adb variant");
            }
        };

        // Table-driven assertions for Timeout, Offline, and Disconnection reason mapping
        for (err, prefix) in [
            (AdbError::Transport(AdbTransportError::Timeout(DEFAULT_CONNECT_TIMEOUT)), "Timeout: ADB operation timed out"),
            (std::io::Error::from(std::io::ErrorKind::TimedOut).into(), "Timeout: ADB operation timed out"),
            (AdbTransportError::Io(std::io::Error::from(std::io::ErrorKind::TimedOut)).into(), "Timeout: ADB transport operation timed out"),
            (AdbSyncError::Io(std::io::Error::from(std::io::ErrorKind::TimedOut)).into(), "Timeout: ADB sync operation timed out"),
            (AdbError::Transport(AdbTransportError::DeviceOffline), "Offline: device is unreachable or disconnected"),
            (std::io::Error::from(std::io::ErrorKind::BrokenPipe).into(), "Offline: device is unreachable or disconnected"),
            (AdbTransportError::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused)).into(), "Offline: device transport disconnected"),
            (AdbSyncError::Io(std::io::Error::from(std::io::ErrorKind::ConnectionReset)).into(), "Offline: device sync stream disconnected"),
            (AdbSyncError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).into(), "ADB sync socket/stream I/O failure"),
            (AdbTransportError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).into(), "ADB transport socket I/O failure"),
        ] {
            check_reason(err, prefix);
        }

        // Predicate checks for timeout and io
        let mut io_timeout: AdbError = std::io::Error::from(std::io::ErrorKind::TimedOut).into();
        assert!(io_timeout.is_timeout() && io_timeout.is_io());
        assert_eq!(io_timeout.as_io_error().map(|e| e.kind()), Some(std::io::ErrorKind::TimedOut));
        assert!(io_timeout.as_io_error_mut().is_some());

        let offline_err = AdbError::Transport(AdbTransportError::DeviceOffline);
        assert!(offline_err.is_offline() && offline_err.is_disconnected() && !offline_err.is_io());

        // IO Disconnection Matrix
        for kind in [
            std::io::ErrorKind::BrokenPipe, std::io::ErrorKind::NotConnected,
            std::io::ErrorKind::ConnectionRefused, std::io::ErrorKind::NetworkUnreachable,
            std::io::ErrorKind::HostUnreachable, std::io::ErrorKind::NetworkDown,
            std::io::ErrorKind::WriteZero, std::io::ErrorKind::UnexpectedEof,
            std::io::ErrorKind::ConnectionAborted,
        ] {
            assert!(AdbError::from(std::io::Error::from(kind)).is_disconnected());
        }

        // Standard std::io::Error roundtrip and high-fidelity semantic mapping
        let raw_io = std::io::Error::from(std::io::ErrorKind::AddrInUse);
        let adb_io: AdbError = raw_io.into();
        assert!(adb_io.is_io());
        assert_eq!(adb_io.as_io_error().map(|e| e.kind()), Some(std::io::ErrorKind::AddrInUse));
        let extracted_io = adb_io.into_io_error().expect("Must extract raw io::Error");
        assert_eq!(extracted_io.kind(), std::io::ErrorKind::AddrInUse);

        // Negative assertion: non-I/O errors must return None/false from io extraction
        let mut non_io_err = AdbError::Transport(AdbTransportError::Unauthorized);
        assert!(!non_io_err.is_io());
        assert!(non_io_err.as_io_error().is_none());
        assert!(non_io_err.as_io_error_mut().is_none());
        assert!(non_io_err.into_io_error().is_none());

        // High-fidelity semantic conversions to std::io::Error & downcast retention
        for (err, expected_kind) in [
            (AdbError::from(AdbTransportError::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe))), std::io::ErrorKind::BrokenPipe),
            (AdbError::from(AdbTransportError::Timeout(DEFAULT_CONNECT_TIMEOUT)), std::io::ErrorKind::TimedOut),
            (AdbError::from(AdbTransportError::DeviceOffline), std::io::ErrorKind::NotConnected),
            (AdbError::from(AdbTransportError::Unauthorized), std::io::ErrorKind::PermissionDenied),
            (AdbTransportError::Protocol("desync".into()).into(), std::io::ErrorKind::InvalidData),
            (AdbTransportError::DeviceOffline.into(), std::io::ErrorKind::NotConnected),
            (AdbSyncError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).into(), std::io::ErrorKind::PermissionDenied),
            (AdbSyncError::BufferTooSmall { expected: 24, actual: 10 }.into(), std::io::ErrorKind::UnexpectedEof),
            (AdbSyncError::PayloadTooLarge { max: 65536, actual: 131072 }.into(), std::io::ErrorKind::InvalidData),
        ] {
            let io_err: std::io::Error = err.into();
            assert_eq!(io_err.kind(), expected_kind);
        }

        // Downcast retention checks
        let timeout_io: std::io::Error = AdbError::from(AdbTransportError::Timeout(DEFAULT_CONNECT_TIMEOUT)).into();
        assert!(timeout_io.get_ref().and_then(|e| e.downcast_ref::<AdbTransportError>()).is_some());

        let sync_underflow_io: std::io::Error = AdbSyncError::BufferTooSmall { expected: 24, actual: 10 }.into();
        assert!(sync_underflow_io.get_ref().and_then(|e| e.downcast_ref::<AdbSyncError>()).is_some());
    }

    #[test]
    fn test_adb_error_downcast_root_cause_and_codes() {
        // 1. Chained downcast through transport to standard IO error
        let raw_io = std::io::Error::from(std::io::ErrorKind::TimedOut);
        let transport_io = AdbError::Transport(AdbTransportError::Io(raw_io));
        assert_eq!(transport_io.error_code(), "TIMED_OUT");
        assert!(transport_io.downcast_ref::<AdbError>().is_some());
        assert!(transport_io.downcast_ref::<AdbTransportError>().is_some());
        assert!(transport_io.downcast_ref::<std::io::Error>().is_some());
        assert_eq!(
            transport_io.root_cause().downcast_ref::<std::io::Error>().map(|e| e.kind()),
            Some(std::io::ErrorKind::TimedOut)
        );

        // 2. Status predicates error code classification
        assert_eq!(AdbError::Transport(AdbTransportError::Unauthorized).error_code(), "UNAUTHORIZED");
        assert_eq!(AdbError::Transport(AdbTransportError::DeviceOffline).error_code(), "DEVICE_OFFLINE");
        assert_eq!(AdbError::from(std::io::Error::from(std::io::ErrorKind::BrokenPipe)).error_code(), "DISCONNECTED");
        assert_eq!(AdbError::Sync(AdbSyncError::RemoteFailure("read only".into())).error_code(), "SYNC_REMOTE_FAILURE");
    }

    #[test]
    fn test_error_chain_iteration_and_fused() {
        let nested_err = AdbError::Transport(AdbTransportError::Io(
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
        ));

        // Test IntoIterator for &'a AdbError
        let mut count = 0;
        for cause in &nested_err {
            assert!(!cause.to_string().is_empty());
            count += 1;
        }
        assert_eq!(count, 3); // AdbError -> AdbTransportError -> std::io::Error

        // Test FusedIterator invariant & size_hint
        let mut chain = nested_err.chain();
        assert_eq!(chain.size_hint(), (1, None));
        for _ in 0..3 {
            assert!(chain.next().is_some());
        }
        assert_eq!(chain.size_hint(), (0, Some(0)));
        assert!(chain.next().is_none() && chain.next().is_none()); // Fused

        // Test empty and default constructors for ErrorChain
        let mut empty_chain = ErrorChain::empty();
        assert_eq!(empty_chain.size_hint(), (0, Some(0)));
        assert!(empty_chain.next().is_none());
        assert!(empty_chain.last().is_none());

        let default_chain = ErrorChain::default();
        assert_eq!(default_chain.size_hint(), (0, Some(0)));

        // Test is::<T>() predicates
        assert!(nested_err.is::<AdbError>());
        assert!(nested_err.is::<AdbTransportError>());
        assert!(nested_err.is::<std::io::Error>());
        assert!(!nested_err.is::<AdbSyncError>());
    }
}
