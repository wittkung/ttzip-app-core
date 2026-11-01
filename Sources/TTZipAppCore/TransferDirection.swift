// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// Operational direction of payload movement across local, device, and network boundaries.
public enum TransferDirection: String, Equatable, Hashable, CaseIterable, Identifiable, Sendable, Codable, CustomStringConvertible, CustomDebugStringConvertible {
    case localCopy = "LocalCopy"
    case upload = "Upload"
    case download = "Download"
    case deviceToDevice = "DeviceToDevice"
    case virtual = "Virtual"

    public var id: Self { self }
    public var description: String { rawValue }
    public var debugDescription: String { "TransferDirection.\(rawValue)" }

    /// Canonical SF Symbol depicting this directional flow.
    public var systemImageName: String {
        switch self {
        case .localCopy: return "doc.on.doc"
        case .upload: return "arrow.up.circle"
        case .download: return "arrow.down.circle"
        case .deviceToDevice: return "arrow.left.and.right.circle"
        case .virtual: return "cylinder.split.1x2"
        }
    }

    /// Resolves operational transfer direction by inspecting source and destination URI schemes with zero heap allocations.
    @inlinable
    public static func resolve(sourceUri: String, destinationUri: String) -> TransferDirection {
        let srcScheme = extractScheme(from: sourceUri)
        let dstScheme = extractScheme(from: destinationUri)

        let isSrcDevice = isDeviceOrRemoteScheme(srcScheme)
        let isDstDevice = isDeviceOrRemoteScheme(dstScheme)
        let isSrcVirtual = isVirtualScheme(srcScheme)
        let isDstVirtual = isVirtualScheme(dstScheme)

        if isSrcVirtual || isDstVirtual {
            return .virtual
        }
        if isSrcDevice && isDstDevice {
            return .deviceToDevice
        }
        if isSrcDevice && !isDstDevice {
            return .download
        }
        if !isSrcDevice && isDstDevice {
            return .upload
        }
        return .localCopy
    }

    @inlinable
    internal static func extractScheme(from uri: String) -> Substring {
        guard let colonIndex = uri.firstIndex(of: ":") else { return "" }
        let afterColon = uri.index(after: colonIndex)
        guard afterColon < uri.endIndex, uri[afterColon] == "/" else { return "" }
        let secondSlash = uri.index(after: afterColon)
        guard secondSlash < uri.endIndex, uri[secondSlash] == "/" else { return "" }
        return uri[..<colonIndex]
    }

    @inlinable
    internal static func isDeviceOrRemoteScheme(_ scheme: Substring) -> Bool {
        switch scheme {
        case "mtp", "adb", "smb", "ftp", "sftp", "http", "https": return true
        default: return false
        }
    }

    @inlinable
    internal static func isVirtualScheme(_ scheme: Substring) -> Bool {
        switch scheme {
        case "pipe", "archive", "memory": return true
        default: return false
        }
    }
}

// MARK: - TransferJobInfo Direction Extensions

extension TransferJobInfo {
    /// Operational direction of this transfer resolved from URI schemes.
    @inlinable
    public var direction: TransferDirection {
        TransferDirection.resolve(sourceUri: sourceUri, destinationUri: destinationUri)
    }

    /// Evaluates if the transfer writes to a remote or mobile device.
    @inlinable public var isUpload: Bool { direction == .upload }

    /// Evaluates if the transfer reads from a remote or mobile device.
    @inlinable public var isDownload: Bool { direction == .download }

    /// Evaluates if the transfer operates entirely on local disk files.
    @inlinable public var isLocalCopy: Bool { direction == .localCopy }

    /// Evaluates if either endpoint involves an attached peripheral device.
    @inlinable public var isDeviceTransfer: Bool { direction == .upload || direction == .download || direction == .deviceToDevice }

    /// Protocol capability: whether the underlying transport supports pausing and resumption.
    /// Virtual pipes and unseekable streams return false.
    @inlinable public var supportsResume: Bool { direction != .virtual }

    /// Protocol capability: whether the underlying transport supports pausing.
    @inlinable public var supportsPause: Bool { direction != .virtual }
}
