// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// Priority rank for queuing and scheduling background file transfers.
public enum TransferPriority: String, Equatable, Hashable, CaseIterable, Identifiable, Sendable, Codable, Comparable, CustomStringConvertible, CustomDebugStringConvertible, LosslessStringConvertible {
    case critical = "Critical"
    case high = "High"
    case normal = "Normal"
    case low = "Low"

    public var id: Self { self }

    /// Creates a priority level from a case-insensitive raw text string.
    @inlinable
    public init?(_ description: String) {
        switch description {
        case "critical", "Critical": self = .critical
        case "high", "High": self = .high
        case "normal", "Normal": self = .normal
        case "low", "Low": self = .low
        default:
            switch description.lowercased() {
            case "critical": self = .critical
            case "high": self = .high
            case "normal": self = .normal
            case "low": self = .low
            default: return nil
            }
        }
    }

    /// Sorting rank: Critical transfers process ahead of standard queues.
    @inlinable
    public var sortPriority: Int {
        switch self {
        case .critical: return 0
        case .high: return 1
        case .normal: return 2
        case .low: return 3
        }
    }

    @inlinable
    public static func < (lhs: Self, rhs: Self) -> Bool {
        lhs.sortPriority < rhs.sortPriority
    }

    public var description: String { rawValue }
    public var debugDescription: String { "TransferPriority.\(rawValue)" }

    /// Canonical SF Symbol icon associated with this priority tier.
    public var systemImageName: String {
        switch self {
        case .critical: return "exclamationmark.3"
        case .high: return "arrow.up.to.line"
        case .normal: return "equal"
        case .low: return "arrow.down.to.line"
        }
    }
}

// MARK: - TransferJobInfo Priority Extensions

extension TransferJobInfo {
    /// Strongly-typed priority level mapped directly from string metadata.
    @inlinable
    public var transferPriority: TransferPriority {
        TransferPriority(priority) ?? .normal
    }
}
