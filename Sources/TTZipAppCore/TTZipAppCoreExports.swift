// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// MARK: - Core Domain Type Aliases
// Re-export core UniFFI symbols into canonical, idiomatic domain names.

/// Core business error generated across the FFI boundary.
public typealias AppCoreError = UniFfiAppCoreError

/// Metadata describing hardware and attached storage peripherals.
public typealias DeviceDescriptor = UniFfiDeviceDescriptor

/// Snapshot of archive and transfer operations.
public typealias TransferJobInfo = UniFfiTransferJobInfo

/// Lifecycle state of a background transfer job across the FFI boundary.
public typealias TransferState = UniFfiTransferState

/// State descriptor for an active workspace tab.
public typealias WorkspaceTab = UniFfiWorkspaceTab

/// Workspace session context managed across the engine lifecycle.
public typealias WorkspaceSession = UniFfiWorkspaceSession

/// User-pinned favorite item metadata.
public typealias FavoriteItem = UniFfiFavoriteItem

// MARK: - TransferState Domain Ergonomics

extension TransferState {
    /// Categorical lifecycle state representation decoupled from payload diagnostics.
    /// Serves as the canonical, payload-free identity for SwiftUI Pickers, filters, matrices, and state persistence.
    public enum Kind: String, Equatable, Hashable, CaseIterable, Identifiable, Sendable, Codable, Comparable, CustomStringConvertible, CustomDebugStringConvertible, LosslessStringConvertible {
        case pending
        case running
        case paused
        case completed
        case failed
        case cancelled

        public var id: Self { self }

        /// Creates a state category from a case-insensitive textual description.
        ///
        /// Employs a zero-allocation fast path for exact string literals, falling back to lowercasing.
        /// Fully tolerates both Commonwealth ('cancelled') and US ('canceled') orthographies.
        @inlinable
        public init?(_ description: String) {
            switch description {
            case "pending", "Pending": self = .pending
            case "running", "Running": self = .running
            case "paused", "Paused": self = .paused
            case "completed", "Completed": self = .completed
            case "failed", "Failed": self = .failed
            case "cancelled", "Cancelled", "canceled", "Canceled": self = .cancelled
            default:
                switch description.lowercased() {
                case "pending": self = .pending
                case "running": self = .running
                case "paused": self = .paused
                case "completed": self = .completed
                case "failed": self = .failed
                case "cancelled", "canceled": self = .cancelled
                default: return nil
                }
            }
        }

        /// Canonical human-readable title for UI selectors and labels.
        public var description: String {
            switch self {
            case .pending: return "Pending"
            case .running: return "Running"
            case .paused: return "Paused"
            case .completed: return "Completed"
            case .failed: return "Failed"
            case .cancelled: return "Cancelled"
            }
        }

        /// Canonical debugging identifier returning static string literals without dynamic heap allocation.
        public var debugDescription: String {
            switch self {
            case .pending: return "TransferState.Kind.pending"
            case .running: return "TransferState.Kind.running"
            case .paused: return "TransferState.Kind.paused"
            case .completed: return "TransferState.Kind.completed"
            case .failed: return "TransferState.Kind.failed"
            case .cancelled: return "TransferState.Kind.cancelled"
            }
        }

        /// Priority rank used for deterministic, human-centric sorting in UI tables and views.
        ///
        /// Running tasks rank highest (0) for prominent visibility, followed by queued pending jobs,
        /// paused jobs, and terminal outcomes.
        @inlinable
        public var sortPriority: Int {
            switch self {
            case .running: return 0
            case .pending: return 1
            case .paused: return 2
            case .completed: return 3
            case .failed: return 4
            case .cancelled: return 5
            }
        }

        @inlinable
        public static func < (lhs: Self, rhs: Self) -> Bool {
            lhs.sortPriority < rhs.sortPriority
        }

        /// Evaluates if the state category is queued and waiting.
        @inlinable public var isPending: Bool { self == .pending }

        /// Evaluates if the state category is actively executing data operations.
        @inlinable public var isRunning: Bool { self == .running }

        /// Evaluates if execution is temporarily suspended.
        @inlinable public var isPaused: Bool { self == .paused }

        /// Evaluates if the state category represents successful completion.
        @inlinable public var isCompleted: Bool { self == .completed }

        /// Evaluates if the state category represents fatal error termination.
        @inlinable public var isFailed: Bool { self == .failed }

        /// Evaluates if the state category represents cancellation.
        @inlinable public var isCancelled: Bool { self == .cancelled }

        /// Canonical SF Symbol name representing this lifecycle state per Apple Human Interface Guidelines.
        public var systemImageName: String {
            switch self {
            case .pending: return "clock"
            case .running: return "arrow.triangle.2.circlepath"
            case .paused: return "pause.circle"
            case .completed: return "checkmark.circle.fill"
            case .failed: return "exclamationmark.triangle.fill"
            case .cancelled: return "xmark.circle"
            }
        }

        /// Evaluates if execution in this state can be paused.
        @inlinable public var canPause: Bool { self == .running }

        /// Evaluates if execution in this state can be resumed.
        @inlinable public var canResume: Bool { self == .paused }

        /// Evaluates if a job in this state can be cancelled.
        @inlinable public var canCancel: Bool { isActive || isPaused }

        /// Semantic alias for `canPause` adhering to Swift API design guidelines.
        @inlinable public var isPausable: Bool { canPause }

        /// Semantic alias for `canResume` adhering to Swift API design guidelines.
        @inlinable public var isResumable: Bool { canResume }

        /// Semantic alias for `canCancel` adhering to Swift API design guidelines.
        @inlinable public var isCancellable: Bool { canCancel }

        /// Evaluates if execution in this state can be retried.
        @inlinable public var canRetry: Bool { self == .failed || self == .cancelled }

        /// Semantic alias for `canRetry` adhering to Swift API design guidelines.
        @inlinable public var isRetryable: Bool { canRetry }

        /// Evaluates if the state category represents any terminal outcome.
        @inlinable
        public var isTerminal: Bool {
            switch self {
            case .completed, .failed, .cancelled:
                return true
            case .pending, .running, .paused:
                return false
            }
        }

        /// Evaluates if the state category represents active engagement in the execution pipeline.
        @inlinable
        public var isActive: Bool {
            switch self {
            case .pending, .running:
                return true
            case .paused, .completed, .failed, .cancelled:
                return false
            }
        }

        /// All active lifecycle categories currently participating in the execution queue.
        public static let activeCases: [Kind] = [.pending, .running]

        /// All terminal lifecycle categories that have permanently ceased execution.
        public static let terminalCases: [Kind] = [.completed, .failed, .cancelled]

        /// All lifecycle categories that can be cancelled by the user.
        public static let cancellableCases: [Kind] = [.pending, .running, .paused]

        /// All lifecycle categories that can be retried by the user.
        public static let retryableCases: [Kind] = [.failed, .cancelled]
    }

    /// High-level lifecycle category for this state, ignoring specific failure diagnostics.
    @inlinable
    public var kind: Kind {
        switch self {
        case .pending: return .pending
        case .running: return .running
        case .paused: return .paused
        case .completed: return .completed
        case .failed(_): return .failed
        case .cancelled: return .cancelled
        }
    }

    /// Symmetrical constructor creating a canonical `TransferState` from a category and optional diagnostics.
    @inlinable
    public init(kind: Kind, failureReason: String = "") {
        switch kind {
        case .pending: self = .pending
        case .running: self = .running
        case .paused: self = .paused
        case .completed: self = .completed
        case .failed: self = .failed(reason: failureReason)
        case .cancelled: self = .cancelled
        }
    }
}

// MARK: - TransferState Predicates & Diagnostics

extension TransferState {
    /// Evaluates if the state is queued and waiting to begin execution.
    @inlinable public var isPending: Bool { kind.isPending }

    /// Evaluates if the job is actively executing byte transfers or disk operations.
    @inlinable public var isRunning: Bool { kind.isRunning }

    /// Evaluates if the state represents successful terminal completion.
    @inlinable public var isCompleted: Bool { kind.isCompleted }

    /// Evaluates if the state represents a fatal error termination.
    @inlinable public var isFailed: Bool { kind.isFailed }

    /// Evaluates if the state represents explicit user or system cancellation.
    @inlinable public var isCancelled: Bool { kind.isCancelled }

    /// Evaluates if execution is temporarily suspended and can be resumed.
    @inlinable public var isPaused: Bool { kind.isPaused }

    /// Evaluates if the state represents any terminal outcome (no further updates expected).
    @inlinable public var isTerminal: Bool { kind.isTerminal }

    /// Evaluates if the job is engaged in the engine queue (either queued or executing).
    @inlinable public var isActive: Bool { kind.isActive }

    /// Evaluates if execution in this state can be paused.
    @inlinable public var canPause: Bool { kind.canPause }

    /// Evaluates if execution in this state can be resumed.
    @inlinable public var canResume: Bool { kind.canResume }

    /// Evaluates if a job in this state can be cancelled.
    @inlinable public var canCancel: Bool { kind.canCancel }

    /// Semantic alias for `canPause` adhering to Swift API design guidelines.
    @inlinable public var isPausable: Bool { canPause }

    /// Semantic alias for `canResume` adhering to Swift API design guidelines.
    @inlinable public var isResumable: Bool { canResume }

    /// Semantic alias for `canCancel` adhering to Swift API design guidelines.
    @inlinable public var isCancellable: Bool { canCancel }

    /// Evaluates if execution in this state can be retried.
    @inlinable public var canRetry: Bool { kind.canRetry }

    /// Semantic alias for `canRetry` adhering to Swift API design guidelines.
    @inlinable public var isRetryable: Bool { canRetry }

    /// Semantic alias for `isTerminal` aligned with Apple `Foundation.Operation.isFinished`.
    @inlinable public var isFinished: Bool { isTerminal }

    /// Semantic alias for `isCompleted` explicitly denoting successful termination.
    @inlinable public var isSuccessful: Bool { isCompleted }

    /// Canonical SF Symbol name representing this lifecycle state per Apple Human Interface Guidelines.
    @inlinable public var systemImageName: String { kind.systemImageName }

    /// Underlying diagnostic reason if the state is `.failed` with a non-blank payload; otherwise nil.
    /// Strictly filters out empty or whitespace-only ghost diagnostics without Foundation dependencies.
    @inlinable
    public var failureReason: String? {
        guard case .failed(let reason) = self, !reason.allSatisfy(\.isWhitespace) else { return nil }
        return reason
    }
}

// MARK: - TransferState String Representations

extension TransferState: CustomStringConvertible, CustomDebugStringConvertible {
    public var description: String {
        switch self {
        case .failed(let reason) where !reason.allSatisfy(\.isWhitespace):
            return "Failed (\(reason))"
        default:
            return kind.description
        }
    }

    /// Canonical debugging representation returning static string literals without dynamic heap allocation for non-failed states.
    public var debugDescription: String {
        switch self {
        case .pending: return "TransferState.pending"
        case .running: return "TransferState.running"
        case .paused: return "TransferState.paused"
        case .completed: return "TransferState.completed"
        case .failed(let reason): return "TransferState.failed(reason: \(reason.debugDescription))"
        case .cancelled: return "TransferState.cancelled"
        }
    }
}

// MARK: - TransferJobInfo High-Performance Domain Inlines (Zero-Cost Abstractions)

extension TransferJobInfo {
    // MARK: Lifecycle State Forwarding

    /// The category of this job's current lifecycle state.
    @inlinable public var stateKind: TransferState.Kind { state.kind }

    /// Evaluates if the job is currently queued.
    @inlinable public var isPending: Bool { state.isPending }

    /// Evaluates if the job is actively executing data operations.
    @inlinable public var isRunning: Bool { state.isRunning }

    /// Evaluates if the job has entered the terminal successful completion state.
    @inlinable public var isCompleted: Bool { state.isCompleted }

    /// Evaluates if the job failed due to an unrecoverable error.
    @inlinable public var isFailed: Bool { state.isFailed }

    /// Evaluates if the job was explicitly cancelled.
    @inlinable public var isCancelled: Bool { state.isCancelled }

    /// Evaluates if the job is temporarily suspended.
    @inlinable public var isPaused: Bool { state.isPaused }

    /// Evaluates if the job is actively queued or running.
    @inlinable public var isActive: Bool { state.isActive }

    /// Evaluates if the job has permanently ceased execution.
    @inlinable public var isTerminal: Bool { state.isTerminal }

    /// Evaluates if the job can be paused from its current execution state.
    @inlinable public var canPause: Bool { state.canPause }

    /// Evaluates if the job can be resumed from its suspended state.
    @inlinable public var canResume: Bool { state.canResume }

    /// Evaluates if the job can be cancelled by the user.
    @inlinable public var canCancel: Bool { state.canCancel }

    /// Semantic alias for `canPause` adhering to Swift API design guidelines.
    @inlinable public var isPausable: Bool { canPause }

    /// Semantic alias for `canResume` adhering to Swift API design guidelines.
    @inlinable public var isResumable: Bool { canResume }

    /// Semantic alias for `canCancel` adhering to Swift API design guidelines.
    @inlinable public var isCancellable: Bool { canCancel }

    /// Evaluates if execution can be restarted after a terminal non-success outcome.
    @inlinable public var canRetry: Bool { state.canRetry }

    /// Semantic alias for `canRetry` adhering to Swift API design guidelines.
    @inlinable public var isRetryable: Bool { canRetry }

    /// Semantic alias for `isTerminal` aligned with Apple `Foundation.Operation.isFinished`.
    @inlinable public var isFinished: Bool { isTerminal }

    /// Semantic alias for `isCompleted` explicitly denoting successful termination.
    @inlinable public var isSuccessful: Bool { isCompleted }

    /// Evaluates if the job has transitioned out of the initial queued state.
    @inlinable public var hasStarted: Bool { !isPending }

    /// Semantic alias for `isPending` aligning with queue-centric architectures.
    @inlinable public var isQueued: Bool { isPending }

    /// Canonical SF Symbol name representing this job's current lifecycle state.
    @inlinable public var systemImageName: String { state.systemImageName }

    /// Standard status title for UI display, distinguishing finalizing/verifying from regular states.
    @inlinable
    public var localizedStatusTitle: String {
        if isFinalizing { return "Finalizing" }
        return stateKind.description
    }

    /// Indicates whether active data transfer has stalled (running with zero throughput before completion).
    /// Guarded against NaN anomalies by using sanitized byte throughput and requiring positive payload size.
    @inlinable
    public var isStalled: Bool {
        isRunning && currentSpeedBytesPerSecond == 0 && !hasTransferredAllBytes && totalBytes > 0
    }

    /// Extracted file or payload name from source or destination URI without Foundation dependency.
    ///
    /// Resolves against trailing path segments; trims query ('?'), fragment ('#'), and consecutive
    /// trailing slashes. Accurately isolates URI schemes (e.g. `file:///`, `pipe:///`) and network authorities,
    /// preserves Windows drive letters (`C:\`), performs zero-allocation Substring slicing, and decodes
    /// percent-encoded UTF-8 octets when present. Falls back cleanly to `id`.
    @inlinable
    public var fileName: String {
        let uri = sourceUri.isEmpty ? destinationUri : sourceUri
        guard !uri.isEmpty else { return id }

        var slice = uri[...]

        // 1. Strip query ('?') and fragment ('#') components per RFC 3986 §3.3
        if let delim = slice.firstIndex(where: { $0 == "?" || $0 == "#" }) {
            slice = slice[..<delim]
        }

        // 2. Isolate hierarchical path from scheme and authority (e.g., "file://", "pipe://")
        if let schemeSep = slice.firstRange(of: "://") {
            let afterScheme = slice[schemeSep.upperBound...]
            if let pathStart = afterScheme.firstIndex(of: "/") {
                slice = afterScheme[pathStart...]
            } else {
                slice = ""
            }
        } else if let colonIdx = slice.firstIndex(of: ":") {
            let prefix = slice[..<colonIdx]
            let isWindowsDrive = prefix.count == 1 && prefix.first!.isLetter
            if !isWindowsDrive {
                slice = slice[slice.index(after: colonIdx)...]
            }
        }

        // 3. Trim trailing path separators (POSIX '/' and Windows '\')
        while slice.last == "/" || slice.last == "\\" {
            slice = slice.dropLast()
        }

        guard !slice.isEmpty else { return id }

        // 4. Extract last path segment
        let candidate: Substring
        if let lastSep = slice.lastIndex(where: { $0 == "/" || $0 == "\\" }) {
            candidate = slice[slice.index(after: lastSep)...]
        } else {
            candidate = slice
        }

        guard !candidate.isEmpty else { return id }

        // 5. Percent-decode UTF-8 escape sequences if present
        if candidate.contains(where: { $0 == "%" }) {
            return Self.percentDecode(candidate)
        }

        return String(candidate)
    }

    /// Internal zero-dependency UTF-8 percent-decoding conforming to RFC 3986 §2.1.
    @inlinable
    internal static func percentDecode(_ slice: Substring) -> String {
        @inline(__always)
        func hexValue(_ byte: UInt8) -> UInt8? {
            switch byte {
            case UInt8(ascii: "0")...UInt8(ascii: "9"): return byte - UInt8(ascii: "0")
            case UInt8(ascii: "a")...UInt8(ascii: "f"): return byte - UInt8(ascii: "a") + 10
            case UInt8(ascii: "A")...UInt8(ascii: "F"): return byte - UInt8(ascii: "A") + 10
            default: return nil
            }
        }

        let utf8 = slice.utf8
        var bytes = [UInt8]()
        bytes.reserveCapacity(utf8.count)

        var idx = utf8.startIndex
        while idx < utf8.endIndex {
            if utf8[idx] == UInt8(ascii: "%") {
                let next1 = utf8.index(after: idx)
                if next1 < utf8.endIndex {
                    let next2 = utf8.index(after: next1)
                    if next2 < utf8.endIndex,
                       let h1 = hexValue(utf8[next1]),
                       let h2 = hexValue(utf8[next2]) {
                        bytes.append((h1 << 4) | h2)
                        idx = utf8.index(after: next2)
                        continue
                    }
                }
            }
            bytes.append(utf8[idx])
            idx = utf8.index(after: idx)
        }
        return String(decoding: bytes, as: UTF8.self)
    }

    /// Underlying diagnostic reason if the job failed with a non-empty payload; otherwise nil.
    @inlinable public var failureReason: String? { state.failureReason }

    // MARK: Transfer Metrics & Progress Computations

    /// Indicates whether the task has an unknown payload size (e.g. streaming or pre-scan).
    /// Bound strictly to lifecycle termination: remains indeterminate across active and paused states.
    @inlinable
    public var isIndeterminate: Bool {
        !isTerminal && totalBytes == 0
    }

    /// Indicates whether all targeted bytes have been transferred across the physical data plane.
    ///
    /// - Note: Reflects physical byte throughput or terminal completion. Does not guarantee error-free closure
    ///   if a verification error occurred afterward. Check `isCompleted` for successful termination.
    @inlinable
    public var hasTransferredAllBytes: Bool {
        if isCompleted { return true }
        return totalBytes > 0 && transferredBytes >= totalBytes
    }

    /// Indicates whether byte transfer has completed and the engine is performing final verification or disk flush.
    @inlinable
    public var isFinalizing: Bool {
        isRunning && hasTransferredAllBytes
    }

    /// Indicates whether progress representation should animate indeterminately.
    ///
    /// Evaluates true during initial unsized pre-scans (`isIndeterminate`) and trailing disk flush / verification (`isFinalizing`).
    /// Directly drives SwiftUI `ProgressView()` style switching without custom view branching.
    @inlinable
    public var isIndeterminateProgress: Bool {
        isIndeterminate || isFinalizing
    }

    /// Remaining bytes to be processed across the transfer boundary.
    /// Evaluates deterministically to 0 upon completion; strictly protected against unsigned underflow.
    @inlinable
    public var remainingBytes: UInt64 {
        if isCompleted { return 0 }
        return totalBytes > transferredBytes ? totalBytes - transferredBytes : 0
    }

    /// Normalized, UI-safe progress ratio clamped strictly within [0.0, 1.0].
    ///
    /// - Evaluates to 1.0 upon `.completed` (handling zero-byte files/folders cleanly).
    /// - Clamps active non-completed ratio monotonically at 0.99. Guarantees non-decreasing progress,
    ///   prevents integer percentage formatters from prematurely rounding to 100%, and eliminates visual jitter.
    /// - Strictly sanitizes non-finite floating-point anomalies (NaN/Infinity) and negative zero (-0.0) to +0.0.
    @inlinable
    public var effectiveProgressRatio: Double {
        if isCompleted { return 1.0 }
        guard progressRatio.isFinite, progressRatio > 0.0 else { return 0.0 }
        return min(progressRatio, 0.99)
    }

    /// Progress expressed as an integer percentage ranging from 0 to 100.
    ///
    /// Evaluates strictly to 100 upon completion, and rounds values accurately up to 99 while active transfers finalize.
    /// Uses standard nearest rounding to guard against IEEE 754 precision dropoffs (e.g. 0.29 * 100 = 28.999...).
    @inlinable
    public var progressPercentage: Int {
        if isCompleted { return 100 }
        guard effectiveProgressRatio.isFinite, effectiveProgressRatio > 0.0 else { return 0 }
        let raw = Int((effectiveProgressRatio * 100.0).rounded())
        return min(max(raw, 0), 99)
    }

    /// Current transfer throughput expressed in raw bytes per second for system formatters.
    ///
    /// Evaluates strictly to 0 when non-running, or when measurements exceed physical limits (> 100 GiB/s).
    /// Safely guards against floating-point conversion overflow traps and IEEE 754 truncation error.
    /// Aligns with the Rust microkernel 500ms sliding-window MiB/s calculation (1 MiB = 1,048,576 bytes).
    @inlinable
    public var currentSpeedBytesPerSecond: UInt64 {
        guard isRunning, currentSpeedMbS.isFinite, currentSpeedMbS > 0 else { return 0 }
        // 100 GiB/s physical sanity ceiling (100,000.0 MiB/s); suppresses corrupt sensor readings to 0.
        guard currentSpeedMbS <= 100_000.0 else { return 0 }
        // Rust microkernel calculates throughput in MiB/s (1,048,576 bytes per unit).
        return UInt64((currentSpeedMbS * 1_048_576.0).rounded())
    }

    /// Estimated remaining duration in seconds based on current throughput and remaining payload.
    ///
    /// Evaluates to nil if the transfer is indeterminate, idle, non-running, finalizing, or speed is zero while payload remains.
    /// Suppresses deceptive 0-second countdowns during trailing flush/verification (`isFinalizing`) per Apple HIG standards.
    /// Enforces a 30-day sanity ceiling to prevent astronomical values during severe throttling from overflowing formatters.
    @inlinable
    public var estimatedTimeRemaining: Double? {
        guard isRunning, !isIndeterminate, !isFinalizing else { return nil }
        let remaining = remainingBytes
        guard remaining > 0 else { return nil }
        let speed = currentSpeedBytesPerSecond
        guard speed > 0 else { return nil }
        let eta = Double(remaining) / Double(speed)
        // 30 days sanity cap (2,592,000s); suppress stalled/infinite estimates to nil for UI clarity.
        guard eta.isFinite, eta <= 2_592_000.0 else { return nil }
        return eta
    }

    /// Estimated remaining duration represented as a native Swift Standard Library `Duration`.
    ///
    /// Directly binds with modern SwiftUI duration formatters without requiring Foundation imports.
    /// Evaluates to nil if indeterminate, non-running, or finalizing.
    /// Strictly verifies numerical finiteness and non-negativity to prevent hardware-level arithmetic conversion traps.
    @inlinable
    public var estimatedDurationRemaining: Duration? {
        guard let seconds = estimatedTimeRemaining, seconds.isFinite, seconds >= 0.0 else { return nil }
        return .seconds(seconds)
    }

    /// Evaluates domain-level visual equivalence for SwiftUI view deduplication and throttled diffing.
    /// Tolerates sub-pixel progress micro-jitter and sub-KiB/s throughput noise.
    @inlinable
    public func isSemanticallyEqual(
        to other: TransferJobInfo,
        progressEpsilon: Double = 0.005,
        speedEpsilonBytesPerSecond: UInt64 = 51_200
    ) -> Bool {
        guard id == other.id,
              state == other.state,
              totalBytes == other.totalBytes,
              transferredBytes == other.transferredBytes else {
            return false
        }
        let progressDiff = abs(effectiveProgressRatio - other.effectiveProgressRatio)
        if progressDiff > progressEpsilon { return false }
        let speedDiff = currentSpeedBytesPerSecond > other.currentSpeedBytesPerSecond ?
            currentSpeedBytesPerSecond - other.currentSpeedBytesPerSecond :
            other.currentSpeedBytesPerSecond - currentSpeedBytesPerSecond
        return speedDiff <= speedEpsilonBytesPerSecond
    }
}

// MARK: - TransferJobInfo Ergonomic Initializers & Preview Mocking

extension TransferJobInfo {
    /// Ergonomic domain initializer providing sensible defaults for previews, mocking, and testing.
    @inlinable
    public init(
        id: String,
        state: TransferState = .pending,
        totalBytes: UInt64 = 0,
        transferredBytes: UInt64 = 0,
        currentSpeedMbS: Double = 0.0,
        progressRatio: Double = 0.0,
        sourceUri: String = "",
        destinationUri: String = "",
        priority: String = "Normal"
    ) {
        self.init(
            id: id,
            sourceUri: sourceUri,
            destinationUri: destinationUri,
            priority: priority,
            totalBytes: totalBytes,
            transferredBytes: transferredBytes,
            state: state,
            currentSpeedMbS: currentSpeedMbS,
            progressRatio: progressRatio
        )
    }

    /// Canonical preview mock factory for SwiftUI `#Preview` and integration test suites.
    @inlinable
    public static func previewMock(
        id: String = "preview-job",
        state: TransferState = .running,
        totalBytes: UInt64 = 1_000_000_000,
        progressRatio: Double = 0.45,
        currentSpeedMbS: Double = 32.5
    ) -> TransferJobInfo {
        let transferred = UInt64(Double(totalBytes) * progressRatio)
        return TransferJobInfo(
            id: id,
            sourceUri: "/Users/dev/Source.zip",
            destinationUri: "/Volumes/Device/Target.zip",
            priority: "Normal",
            totalBytes: totalBytes,
            transferredBytes: transferred,
            state: state,
            currentSpeedMbS: currentSpeedMbS,
            progressRatio: progressRatio
        )
    }

    /// Full-spectrum preview sample matrix covering all key lifecycle states for SwiftUI `#Preview` and tests.
    public static let previewSamples: [TransferJobInfo] = [
        TransferJobInfo(
            id: "job-queued",
            state: .pending,
            totalBytes: 500_000_000,
            transferredBytes: 0,
            currentSpeedMbS: 0.0,
            progressRatio: 0.0,
            sourceUri: "/Users/dev/Archive.zip",
            destinationUri: "/Volumes/USB/Archive.zip"
        ),
        TransferJobInfo(
            id: "job-active",
            state: .running,
            totalBytes: 1_000_000_000,
            transferredBytes: 450_000_000,
            currentSpeedMbS: 48.5,
            progressRatio: 0.45,
            sourceUri: "/Users/dev/Video.mp4",
            destinationUri: "/Volumes/External/Video.mp4"
        ),
        TransferJobInfo(
            id: "job-finalizing",
            state: .running,
            totalBytes: 2_000_000_000,
            transferredBytes: 2_000_000_000,
            currentSpeedMbS: 0.0,
            progressRatio: 1.0,
            sourceUri: "/Users/dev/DiskBackup.dmg",
            destinationUri: "/Volumes/Backup/DiskBackup.dmg"
        ),
        TransferJobInfo(
            id: "job-paused",
            state: .paused,
            totalBytes: 800_000_000,
            transferredBytes: 320_000_000,
            currentSpeedMbS: 0.0,
            progressRatio: 0.40,
            sourceUri: "/Users/dev/Photos.tar",
            destinationUri: "/Volumes/Cloud/Photos.tar"
        ),
        TransferJobInfo(
            id: "job-completed",
            state: .completed,
            totalBytes: 150_000_000,
            transferredBytes: 150_000_000,
            currentSpeedMbS: 0.0,
            progressRatio: 1.0,
            sourceUri: "/Users/dev/Document.pdf",
            destinationUri: "/Volumes/Device/Document.pdf"
        ),
        TransferJobInfo(
            id: "job-failed",
            state: .failed(reason: "I/O device communication timeout"),
            totalBytes: 600_000_000,
            transferredBytes: 120_000_000,
            currentSpeedMbS: 0.0,
            progressRatio: 0.20,
            sourceUri: "/Users/dev/Corrupt.iso",
            destinationUri: "/Volumes/USB/Corrupt.iso"
        ),
        TransferJobInfo(
            id: "job-cancelled",
            state: .cancelled,
            totalBytes: 300_000_000,
            transferredBytes: 90_000_000,
            currentSpeedMbS: 0.0,
            progressRatio: 0.30,
            sourceUri: "/Users/dev/Cancelled.bin",
            destinationUri: "/Volumes/Drive/Cancelled.bin"
        )
    ]
}

