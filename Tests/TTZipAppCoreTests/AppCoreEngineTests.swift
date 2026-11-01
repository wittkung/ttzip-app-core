// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import Foundation
import TTZipAppCore
import XCTest

final class AppCoreEngineTests: XCTestCase {
    private var tempDirectory: URL!
    private var engine: AppCoreEngine!

    override func setUpWithError() throws {
        try super.setUpWithError()
        tempDirectory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "ttzip_app_core_test_\(UUID().uuidString)"
        )
        try FileManager.default.createDirectory(at: tempDirectory, withIntermediateDirectories: true)
        engine = try AppCoreEngine(configDir: tempDirectory.path)
    }

    override func tearDownWithError() throws {
        engine = nil
        if let tempDirectory = tempDirectory {
            try? FileManager.default.removeItem(at: tempDirectory)
        }
        try super.tearDownWithError()
    }

    func testEngineInitializationAndFavorites() throws {
        XCTAssertNotNil(engine)

        // Initial favorites should be empty
        let initialFavorites = engine.listFavorites()
        XCTAssertEqual(initialFavorites.count, 0)

        // Add a bookmark favorite item
        let item = try engine.addFavorite(
            name: "Downloads",
            uri: "file:///Users/Shared/Downloads",
            iconHint: "folder",
            isPinned: true,
            tags: ["system", "fast"]
        )
        XCTAssertEqual(item.name, "Downloads")
        XCTAssertEqual(item.uri, "file:///Users/Shared/Downloads")
        XCTAssertTrue(item.isPinned)
        XCTAssertEqual(item.tags, ["system", "fast"])

        // Query favorites
        let updatedFavorites = engine.listFavorites()
        XCTAssertEqual(updatedFavorites.count, 1)
        XCTAssertEqual(updatedFavorites[0].id, item.id)

        // Remove favorite
        let removed = try engine.removeFavorite(idStr: item.id)
        XCTAssertTrue(removed)
        XCTAssertEqual(engine.listFavorites().count, 0)
    }

    func testWorkspaceSessionLifecycle() throws {
        // Create session
        let sessionId = engine.createWorkspaceSession(
            name: "Default Workspace",
            initialUri: "file:///Volumes/Data"
        )
        XCTAssertFalse(sessionId.isEmpty)

        // Retrieve session
        guard let session = engine.getWorkspaceSession(sessionIdStr: sessionId) else {
            XCTFail("Failed to retrieve workspace session")
            return
        }
        XCTAssertEqual(session.name, "Default Workspace")
        XCTAssertEqual(session.tabs.count, 1)

        // Add a tab
        let tabId = try engine.addWorkspaceTab(
            sessionIdStr: sessionId,
            title: "Archive View",
            uri: "archive:///tmp/test.zip"
        )
        XCTAssertFalse(tabId.isEmpty)

        let sessionWithTwoTabs = engine.getWorkspaceSession(sessionIdStr: sessionId)
        XCTAssertEqual(sessionWithTwoTabs?.tabs.count, 2)
        XCTAssertEqual(sessionWithTwoTabs?.activeTabId, tabId)

        // Close tab
        try engine.closeWorkspaceTab(sessionIdStr: sessionId, tabIdStr: tabId)
        let sessionAfterClose = engine.getWorkspaceSession(sessionIdStr: sessionId)
        XCTAssertEqual(sessionAfterClose?.tabs.count, 1)

        // Close session
        let closed = engine.closeWorkspaceSession(sessionIdStr: sessionId)
        XCTAssertTrue(closed)
        XCTAssertNil(engine.getWorkspaceSession(sessionIdStr: sessionId))
    }

    func testTransferCoordinatorLifecycle() throws {
        let totalBytes: UInt64 = 10 * 1024 * 1024
        let jobId = try engine.submitTransfer(
            priorityStr: "High",
            sourceUri: "file:///tmp/source.dat",
            destinationUri: "mtp://device/storage/target.dat",
            totalBytes: totalBytes
        )
        XCTAssertFalse(jobId.isEmpty)

        // Record chunk progress
        let chunkBytes: UInt64 = 2 * 1024 * 1024
        let progress = try engine.recordTransferChunk(jobIdStr: jobId, chunkBytes: chunkBytes)
        XCTAssertEqual(progress.transferredBytes, chunkBytes)
        XCTAssertEqual(progress.totalBytes, totalBytes)
        XCTAssertEqual(progress.progressRatio, 0.2, accuracy: 0.001)

        // Pause and Resume
        try engine.pauseTransfer(jobIdStr: jobId)
        let resumedOffset = try engine.resumeTransfer(jobIdStr: jobId)
        XCTAssertEqual(resumedOffset, chunkBytes)

        // Cancel
        try engine.cancelTransfer(jobIdStr: jobId)
        let finalSnapshot = engine.getTransferProgress(jobIdStr: jobId)
        XCTAssertEqual(finalSnapshot?.state, .cancelled)
    }

    func testDeviceSubsystem() throws {
        // Physical MTP scanning over USB bus
        let mtpDevices = try engine.scanMtpDevices()
        XCTAssertGreaterThanOrEqual(mtpDevices.count, 0)

        // ADB device listing via daemon bridge (handles environment where adb daemon is offline)
        do {
            let adbDevices = try engine.listAdbDevices()
            XCTAssertGreaterThanOrEqual(adbDevices.count, 0)
        } catch let AppCoreError.AdbError(reason) {
            // Expected in isolated environments without local adb server running
            XCTAssertTrue(reason.contains("Failed to connect") || reason.contains("Connection refused"))
        }
    }

    func testTransferDomainErgonomics() throws {
        // 1. TransferState predicate verification (exhaustive)
        let pendingState: TransferState = .pending
        XCTAssertTrue(pendingState.isPending)
        XCTAssertFalse(pendingState.isRunning)
        XCTAssertTrue(pendingState.isActive)
        XCTAssertFalse(pendingState.isTerminal)
        XCTAssertFalse(pendingState.isCompleted)
        XCTAssertFalse(pendingState.isFailed)
        XCTAssertFalse(pendingState.isCancelled)
        XCTAssertFalse(pendingState.isPaused)

        let runningState: TransferState = .running
        XCTAssertFalse(runningState.isPending)
        XCTAssertTrue(runningState.isRunning)
        XCTAssertTrue(runningState.isActive)
        XCTAssertFalse(runningState.isTerminal)
        XCTAssertFalse(runningState.isCompleted)
        XCTAssertFalse(runningState.isFailed)
        XCTAssertFalse(runningState.isCancelled)
        XCTAssertFalse(runningState.isPaused)

        let pausedState: TransferState = .paused
        XCTAssertFalse(pausedState.isPending)
        XCTAssertFalse(pausedState.isRunning)
        XCTAssertFalse(pausedState.isActive)
        XCTAssertFalse(pausedState.isTerminal)
        XCTAssertFalse(pausedState.isCompleted)
        XCTAssertFalse(pausedState.isFailed)
        XCTAssertFalse(pausedState.isCancelled)
        XCTAssertTrue(pausedState.isPaused)

        let completedState: TransferState = .completed
        XCTAssertFalse(completedState.isPending)
        XCTAssertFalse(completedState.isRunning)
        XCTAssertFalse(completedState.isActive)
        XCTAssertTrue(completedState.isTerminal)
        XCTAssertTrue(completedState.isCompleted)
        XCTAssertFalse(completedState.isFailed)
        XCTAssertFalse(completedState.isCancelled)
        XCTAssertFalse(completedState.isPaused)

        let failedState: TransferState = .failed(reason: "CRC32 mismatch")
        XCTAssertFalse(failedState.isPending)
        XCTAssertFalse(failedState.isRunning)
        XCTAssertFalse(failedState.isActive)
        XCTAssertTrue(failedState.isTerminal)
        XCTAssertFalse(failedState.isCompleted)
        XCTAssertTrue(failedState.isFailed)
        XCTAssertFalse(failedState.isCancelled)
        XCTAssertFalse(failedState.isPaused)

        let cancelledState: TransferState = .cancelled
        XCTAssertFalse(cancelledState.isPending)
        XCTAssertFalse(cancelledState.isRunning)
        XCTAssertFalse(cancelledState.isActive)
        XCTAssertTrue(cancelledState.isTerminal)
        XCTAssertFalse(cancelledState.isCompleted)
        XCTAssertFalse(cancelledState.isFailed)
        XCTAssertTrue(cancelledState.isCancelled)
        XCTAssertFalse(cancelledState.isPaused)

        // 2. Zero-byte completed job UI rendering and progress correction
        let zeroByteCompletedJob = TransferJobInfo(
            id: "zero-byte-job",
            sourceUri: "file:///empty.txt",
            destinationUri: "mtp://device/empty.txt",
            priority: "Normal",
            totalBytes: 0,
            transferredBytes: 0,
            state: .completed,
            currentSpeedMbS: 0.0,
            progressRatio: 0.0
        )
        XCTAssertTrue(zeroByteCompletedJob.isCompleted)
        XCTAssertTrue(zeroByteCompletedJob.isTerminal)
        XCTAssertFalse(zeroByteCompletedJob.isActive)
        XCTAssertFalse(zeroByteCompletedJob.isPaused)
        XCTAssertFalse(zeroByteCompletedJob.isFailed)
        XCTAssertFalse(zeroByteCompletedJob.isCancelled)
        XCTAssertFalse(zeroByteCompletedJob.isIndeterminate)
        XCTAssertTrue(zeroByteCompletedJob.hasTransferredAllBytes)
        XCTAssertFalse(zeroByteCompletedJob.isFinalizing)
        XCTAssertEqual(zeroByteCompletedJob.remainingBytes, 0)
        XCTAssertEqual(zeroByteCompletedJob.effectiveProgressRatio, 1.0)
        XCTAssertEqual(zeroByteCompletedJob.progressPercentage, 100)
        XCTAssertEqual(zeroByteCompletedJob.currentSpeedBytesPerSecond, 0)

        // 3. Active processing job domain calculations and speed conversion
        let activeJob = TransferJobInfo(
            id: "active-job",
            sourceUri: "file:///payload.bin",
            destinationUri: "mtp://device/payload.bin",
            priority: "High",
            totalBytes: 1000,
            transferredBytes: 400,
            state: .running,
            currentSpeedMbS: 25.5,
            progressRatio: 0.4
        )
        XCTAssertFalse(activeJob.isCompleted)
        XCTAssertFalse(activeJob.isTerminal)
        XCTAssertTrue(activeJob.isActive)
        XCTAssertTrue(activeJob.isRunning)
        XCTAssertFalse(activeJob.isPaused)
        XCTAssertFalse(activeJob.isIndeterminate)
        XCTAssertFalse(activeJob.hasTransferredAllBytes)
        XCTAssertFalse(activeJob.isFinalizing)
        XCTAssertFalse(activeJob.isIndeterminateProgress)
        XCTAssertEqual(activeJob.remainingBytes, 600)
        XCTAssertEqual(activeJob.effectiveProgressRatio, 0.4, accuracy: 0.001)
        XCTAssertEqual(activeJob.progressPercentage, 40)
        XCTAssertEqual(activeJob.currentSpeedBytesPerSecond, 26_738_688)

        // 4. Indeterminate tasks (e.g. dynamic streams, scanning without known size)
        let indeterminateJob = TransferJobInfo(
            id: "stream-job",
            sourceUri: "pipe:///stream",
            destinationUri: "file:///out.zip",
            priority: "Normal",
            totalBytes: 0,
            transferredBytes: 1024,
            state: .running,
            currentSpeedMbS: 5.0,
            progressRatio: 0.0
        )
        XCTAssertTrue(indeterminateJob.isIndeterminate)
        XCTAssertTrue(indeterminateJob.isIndeterminateProgress)
        XCTAssertTrue(indeterminateJob.isActive)
        XCTAssertFalse(indeterminateJob.isFinalizing)

        // 5. UX 0.99 cap: active transfer reaching 100% before engine flushes
        let flushingJob = TransferJobInfo(
            id: "flushing-job",
            sourceUri: "file:///large.zip",
            destinationUri: "mtp://device/large.zip",
            priority: "Normal",
            totalBytes: 1000,
            transferredBytes: 1000,
            state: .running,
            currentSpeedMbS: 12.0,
            progressRatio: 1.0
        )
        XCTAssertFalse(flushingJob.isCompleted)
        XCTAssertFalse(flushingJob.isTerminal)
        XCTAssertTrue(flushingJob.hasTransferredAllBytes)
        XCTAssertTrue(flushingJob.isFinalizing)
        XCTAssertTrue(flushingJob.isIndeterminateProgress)
        XCTAssertEqual(flushingJob.effectiveProgressRatio, 0.99, accuracy: 0.0001)
        XCTAssertEqual(flushingJob.progressPercentage, 99)
        // Suppress deceptive 0s countdown during finalizing phase per Apple HIG
        XCTAssertNil(flushingJob.estimatedTimeRemaining)

        // 5c. Finalizing job with 0 speed (disk sync/CRC phase) suppresses ETA to nil
        let zeroSpeedFinalizingJob = TransferJobInfo(
            id: "zero-speed-finalizing",
            sourceUri: "file:///large.zip",
            destinationUri: "mtp://device/large.zip",
            priority: "Normal",
            totalBytes: 1000,
            transferredBytes: 1000,
            state: .running,
            currentSpeedMbS: 0.0,
            progressRatio: 1.0
        )
        XCTAssertTrue(zeroSpeedFinalizingJob.isFinalizing)
        XCTAssertEqual(zeroSpeedFinalizingJob.remainingBytes, 0)
        XCTAssertEqual(zeroSpeedFinalizingJob.currentSpeedBytesPerSecond, 0)
        XCTAssertNil(zeroSpeedFinalizingJob.estimatedTimeRemaining)

        // 5b. Monotonic non-decreasing progress guarantee (no premature 100% rounding or rollback)
        let largeFileJob = TransferJobInfo(
            id: "large-file-job",
            sourceUri: "file:///archive.iso",
            destinationUri: "mtp://device/archive.iso",
            priority: "Normal",
            totalBytes: 100_000,
            transferredBytes: 99_500,
            state: .running,
            currentSpeedMbS: 50.0,
            progressRatio: 0.995
        )
        XCTAssertFalse(largeFileJob.hasTransferredAllBytes)
        // 0.995 is clamped monotonically to 0.99 so UI percentage stays at 99% until .completed
        XCTAssertEqual(largeFileJob.effectiveProgressRatio, 0.99, accuracy: 0.0001)

        // Monotonic sequence verification: 0.95 -> 0.99 -> 0.995 -> 1.0(running) -> 1.0(completed)
        let monotonicRatios = [0.95, 0.99, 0.995, 1.0]
        var previousEffective = 0.0
        for r in monotonicRatios {
            let job = TransferJobInfo(
                id: "mono-test",
                sourceUri: "",
                destinationUri: "",
                priority: "Normal",
                totalBytes: 1000,
                transferredBytes: UInt64(r * 1000.0),
                state: .running,
                currentSpeedMbS: 10.0,
                progressRatio: r
            )
            let effective = job.effectiveProgressRatio
            XCTAssertGreaterThanOrEqual(effective, previousEffective)
            XCTAssertLessThanOrEqual(effective, 0.99)
            previousEffective = effective
        }

        // 6. Stale speed suppression when paused or failed
        let pausedJobWithStaleSpeed = TransferJobInfo(
            id: "paused-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 1000,
            transferredBytes: 500,
            state: .paused,
            currentSpeedMbS: 48.5,
            progressRatio: 0.5
        )
        XCTAssertTrue(pausedJobWithStaleSpeed.isPaused)
        XCTAssertFalse(pausedJobWithStaleSpeed.isRunning)
        XCTAssertEqual(pausedJobWithStaleSpeed.currentSpeedBytesPerSecond, 0)

        // 7. Floating-point anomaly shielding (NaN, Infinity, and Out-of-Bounds clamping)
        let nanProgressJob = TransferJobInfo(
            id: "nan-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 10,
            state: .running,
            currentSpeedMbS: Double.nan,
            progressRatio: Double.nan
        )
        XCTAssertEqual(nanProgressJob.effectiveProgressRatio, 0.0)
        XCTAssertEqual(nanProgressJob.currentSpeedBytesPerSecond, 0)

        let infProgressJob = TransferJobInfo(
            id: "inf-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 10,
            state: .running,
            currentSpeedMbS: Double.infinity,
            progressRatio: Double.infinity
        )
        XCTAssertEqual(infProgressJob.effectiveProgressRatio, 0.0)
        XCTAssertEqual(infProgressJob.currentSpeedBytesPerSecond, 0)

        let clampedHighJob = TransferJobInfo(
            id: "high-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 100,
            state: .running,
            currentSpeedMbS: 0.0,
            progressRatio: 1.05
        )
        XCTAssertEqual(clampedHighJob.effectiveProgressRatio, 0.99, accuracy: 0.0001)

        let clampedLowJob = TransferJobInfo(
            id: "low-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 0,
            state: .running,
            currentSpeedMbS: -5.0,
            progressRatio: -0.2
        )
        XCTAssertEqual(clampedLowJob.effectiveProgressRatio, 0.0)
        XCTAssertEqual(clampedLowJob.currentSpeedBytesPerSecond, 0)

        // 8. Physical sanity ceiling and overflow protection (100 GB/s ceiling, exceeds return 0)
        let overflowSpeedJob = TransferJobInfo(
            id: "overflow-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 10,
            state: .running,
            currentSpeedMbS: Double.greatestFiniteMagnitude,
            progressRatio: 0.1
        )
        XCTAssertEqual(overflowSpeedJob.currentSpeedBytesPerSecond, 0)

        let excessiveSpeedJob = TransferJobInfo(
            id: "excessive-speed-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 10,
            state: .running,
            currentSpeedMbS: 150_000.0,
            progressRatio: 0.1
        )
        XCTAssertEqual(excessiveSpeedJob.currentSpeedBytesPerSecond, 0)

        let maxPermittedSpeedJob = TransferJobInfo(
            id: "max-permitted-speed-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 10,
            state: .running,
            currentSpeedMbS: 100_000.0,
            progressRatio: 0.1
        )
        XCTAssertEqual(maxPermittedSpeedJob.currentSpeedBytesPerSecond, 104_857_600_000)

        // 9. IEEE 754 truncation prevention via .rounded()
        // 0.9999999999999998 * 1_048_576 would truncate to 1_048_575 without rounding
        let roundingJob = TransferJobInfo(
            id: "rounding-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 10,
            state: .running,
            currentSpeedMbS: 0.9999999999999998,
            progressRatio: 0.1
        )
        XCTAssertEqual(roundingJob.currentSpeedBytesPerSecond, 1_048_576)

        // 10. Kind CaseIterable, Identifiable, CustomStringConvertible, Equatable, Hashable, Codable, and symmetric predicates validation
        XCTAssertEqual(TransferState.Kind.allCases.count, 6)
        let kindSet = Set(TransferState.Kind.allCases)
        XCTAssertEqual(kindSet.count, 6)
        XCTAssertTrue(kindSet.contains(.running))

        // Codable roundtrip verification for TransferState.Kind
        let encoder = JSONEncoder()
        let decoder = JSONDecoder()
        for kind in TransferState.Kind.allCases {
            let data = try encoder.encode(kind)
            let decoded = try decoder.decode(TransferState.Kind.self, from: data)
            XCTAssertEqual(decoded, kind)
        }

        // activeCases & terminalCases categorical sets
        XCTAssertEqual(TransferState.Kind.activeCases, [.pending, .running])
        XCTAssertEqual(TransferState.Kind.terminalCases, [.completed, .failed, .cancelled])

        // Symmetrical TransferState(kind:failureReason:) constructor
        XCTAssertEqual(TransferState(kind: .pending), .pending)
        XCTAssertEqual(TransferState(kind: .running), .running)
        XCTAssertEqual(TransferState(kind: .paused), .paused)
        XCTAssertEqual(TransferState(kind: .completed), .completed)
        XCTAssertEqual(TransferState(kind: .failed, failureReason: "Network IO"), .failed(reason: "Network IO"))
        XCTAssertEqual(TransferState(kind: .failed), .failed(reason: ""))
        XCTAssertEqual(TransferState(kind: .cancelled), .cancelled)

        for kind in TransferState.Kind.allCases {
            XCTAssertEqual(kind.id, kind)
            XCTAssertEqual(kind.description, kind.rawValue.capitalized)
            XCTAssertEqual(kind.debugDescription, "TransferState.Kind.\(kind.rawValue)")
        }

        // Kind symmetric state predicates
        XCTAssertTrue(TransferState.Kind.pending.isPending)
        XCTAssertFalse(TransferState.Kind.pending.isRunning)
        XCTAssertTrue(TransferState.Kind.running.isRunning)
        XCTAssertFalse(TransferState.Kind.running.isPending)
        XCTAssertTrue(TransferState.Kind.paused.isPaused)
        XCTAssertTrue(TransferState.Kind.completed.isCompleted)
        XCTAssertTrue(TransferState.Kind.failed.isFailed)
        XCTAssertTrue(TransferState.Kind.cancelled.isCancelled)

        // Kind Comparable sorting priority order
        XCTAssertTrue(TransferState.Kind.running < TransferState.Kind.pending)
        XCTAssertTrue(TransferState.Kind.pending < TransferState.Kind.paused)
        XCTAssertTrue(TransferState.Kind.paused < TransferState.Kind.completed)
        XCTAssertTrue(TransferState.Kind.completed < TransferState.Kind.failed)
        XCTAssertTrue(TransferState.Kind.failed < TransferState.Kind.cancelled)
        let unsortedKinds: [TransferState.Kind] = [.cancelled, .completed, .running, .paused, .pending, .failed]
        XCTAssertEqual(unsortedKinds.sorted(), [.running, .pending, .paused, .completed, .failed, .cancelled])

        // Kind LosslessStringConvertible validation
        XCTAssertEqual(TransferState.Kind("running"), .running)
        XCTAssertEqual(TransferState.Kind("PENDING"), .pending)
        XCTAssertEqual(TransferState.Kind("PaUsEd"), .paused)
        XCTAssertEqual(TransferState.Kind("Completed"), .completed)
        XCTAssertEqual(TransferState.Kind("failed"), .failed)
        XCTAssertEqual(TransferState.Kind("Cancelled"), .cancelled)
        XCTAssertNil(TransferState.Kind("unknown_state"))

        // Action and control predicates
        XCTAssertTrue(runningState.kind.canPause)
        XCTAssertFalse(pendingState.kind.canPause)
        XCTAssertTrue(pausedState.kind.canResume)
        XCTAssertFalse(runningState.kind.canResume)
        XCTAssertTrue(runningState.kind.canCancel)
        XCTAssertTrue(pendingState.kind.canCancel)
        XCTAssertTrue(pausedState.kind.canCancel)
        XCTAssertFalse(completedState.kind.canCancel)
        XCTAssertFalse(failedState.kind.canCancel)
        XCTAssertFalse(cancelledState.kind.canCancel)

        XCTAssertTrue(runningState.canPause)
        XCTAssertTrue(pausedState.canResume)
        XCTAssertTrue(pendingState.canCancel)
        XCTAssertTrue(activeJob.canPause)
        XCTAssertFalse(activeJob.canResume)
        XCTAssertTrue(activeJob.canCancel)

        // Action semantic aliases and SF Symbol representations
        XCTAssertEqual(runningState.isPausable, runningState.canPause)
        XCTAssertEqual(pausedState.isResumable, pausedState.canResume)
        XCTAssertEqual(pendingState.isCancellable, pendingState.canCancel)
        XCTAssertEqual(activeJob.isPausable, activeJob.canPause)
        XCTAssertEqual(activeJob.isResumable, activeJob.canResume)
        XCTAssertEqual(activeJob.isCancellable, activeJob.canCancel)
        XCTAssertEqual(activeJob.isFinished, activeJob.isTerminal)
        XCTAssertEqual(activeJob.isSuccessful, activeJob.isCompleted)

        XCTAssertEqual(TransferState.Kind.pending.systemImageName, "clock")
        XCTAssertEqual(TransferState.Kind.running.systemImageName, "arrow.triangle.2.circlepath")
        XCTAssertEqual(TransferState.Kind.paused.systemImageName, "pause.circle")
        XCTAssertEqual(TransferState.Kind.completed.systemImageName, "checkmark.circle.fill")
        XCTAssertEqual(TransferState.Kind.failed.systemImageName, "exclamationmark.triangle.fill")
        XCTAssertEqual(TransferState.Kind.cancelled.systemImageName, "xmark.circle")
        XCTAssertEqual(activeJob.systemImageName, "arrow.triangle.2.circlepath")

        XCTAssertEqual(TransferState.Kind.cancellableCases, [.pending, .running, .paused])

        // Indeterminate stream retains indeterminate across paused state until terminal
        let pausedIndeterminateJob = TransferJobInfo(
            id: "paused-stream",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 0,
            transferredBytes: 1024,
            state: .paused,
            currentSpeedMbS: 0.0,
            progressRatio: 0.0
        )
        XCTAssertTrue(pausedIndeterminateJob.isIndeterminate)
        XCTAssertTrue(pausedIndeterminateJob.isResumable)
        XCTAssertFalse(pausedIndeterminateJob.isPausable)

        // TransferState CustomStringConvertible and CustomDebugStringConvertible validation
        XCTAssertEqual(pendingState.description, "Pending")
        XCTAssertEqual(runningState.description, "Running")
        XCTAssertEqual(pausedState.description, "Paused")
        XCTAssertEqual(completedState.description, "Completed")
        XCTAssertEqual(failedState.description, "Failed (CRC32 mismatch)")
        XCTAssertEqual(cancelledState.description, "Cancelled")

        XCTAssertEqual(pendingState.debugDescription, "TransferState.pending")
        XCTAssertEqual(runningState.debugDescription, "TransferState.running")
        XCTAssertEqual(pausedState.debugDescription, "TransferState.paused")
        XCTAssertEqual(completedState.debugDescription, "TransferState.completed")
        XCTAssertEqual(failedState.debugDescription, "TransferState.failed(reason: \"CRC32 mismatch\")")
        XCTAssertEqual(cancelledState.debugDescription, "TransferState.cancelled")

        // 11. Kind categorical decoupling for safe UI filtering
        XCTAssertEqual(pendingState.kind, .pending)
        XCTAssertEqual(runningState.kind, .running)
        XCTAssertEqual(pausedState.kind, .paused)
        XCTAssertEqual(completedState.kind, .completed)
        XCTAssertEqual(failedState.kind, .failed)
        XCTAssertEqual(cancelledState.kind, .cancelled)
        XCTAssertEqual(activeJob.stateKind, .running)
        XCTAssertTrue(completedState.kind.isTerminal)
        XCTAssertTrue(failedState.kind.isTerminal)
        XCTAssertTrue(cancelledState.kind.isTerminal)
        XCTAssertFalse(runningState.kind.isTerminal)
        XCTAssertTrue(runningState.kind.isActive)
        XCTAssertTrue(pendingState.kind.isActive)
        XCTAssertFalse(pausedState.kind.isActive)
        XCTAssertFalse(completedState.kind.isActive)

        // 12. Failure reason extraction and ghost diagnostic sanitization
        XCTAssertEqual(failedState.failureReason, "CRC32 mismatch")
        XCTAssertNil(completedState.failureReason)
        let failedJob = TransferJobInfo(
            id: "failed-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 50,
            state: .failed(reason: "Permission Denied"),
            currentSpeedMbS: 0.0,
            progressRatio: 0.5
        )
        XCTAssertEqual(failedJob.failureReason, "Permission Denied")

        // Empty reason should safely evaluate to nil to prevent ghost bubbles
        let emptyReasonState: TransferState = .failed(reason: "")
        XCTAssertNil(emptyReasonState.failureReason)
        XCTAssertEqual(emptyReasonState.description, "Failed")
        XCTAssertEqual(emptyReasonState.debugDescription, "TransferState.failed(reason: \"\")")
        let emptyReasonJob = TransferJobInfo(
            id: "empty-reason-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 100,
            transferredBytes: 50,
            state: .failed(reason: ""),
            currentSpeedMbS: 0.0,
            progressRatio: 0.5
        )
        XCTAssertNil(emptyReasonJob.failureReason)

        // Whitespace-only reason should also evaluate to nil to prevent ghost bubbles
        let whitespaceReasonState: TransferState = .failed(reason: "   \t\n")
        XCTAssertNil(whitespaceReasonState.failureReason)
        XCTAssertEqual(whitespaceReasonState.description, "Failed")
        let whitespaceReasonJob = TransferJobInfo(id: "ws-job", state: .failed(reason: "  \t "))
        XCTAssertNil(whitespaceReasonJob.failureReason)

        // 13. Estimated time remaining (ETA)
        // 600 bytes remaining / 26_738_688 bytes/s (1 MiB = 1,048,576 B)
        let expectedEta = 600.0 / 26_738_688.0
        XCTAssertNotNil(activeJob.estimatedTimeRemaining)
        XCTAssertEqual(activeJob.estimatedTimeRemaining!, expectedEta, accuracy: 0.000001)

        // Throttled astronomical ETA: 50 GB remaining at 1 B/s = 50 billion seconds (> 30 days) -> nil
        let throttledJob = TransferJobInfo(
            id: "throttled-job",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 50_000_000_000,
            transferredBytes: 0,
            state: .running,
            currentSpeedMbS: 0.000001,
            progressRatio: 0.0
        )
        XCTAssertNil(throttledJob.estimatedTimeRemaining)

        // ETA guards: indeterminate, paused, zero-speed
        XCTAssertNil(indeterminateJob.estimatedTimeRemaining)
        XCTAssertNil(pausedJobWithStaleSpeed.estimatedTimeRemaining)
        let zeroSpeedJob = TransferJobInfo(
            id: "zero-speed",
            sourceUri: "",
            destinationUri: "",
            priority: "Normal",
            totalBytes: 1000,
            transferredBytes: 500,
            state: .running,
            currentSpeedMbS: 0.0,
            progressRatio: 0.5
        )
        XCTAssertNil(zeroSpeedJob.estimatedTimeRemaining)

        // 13b. Native Swift.Duration alignment
        XCTAssertNotNil(activeJob.estimatedDurationRemaining)
        XCTAssertEqual(activeJob.estimatedDurationRemaining, .seconds(expectedEta))
        for nilJob in [flushingJob, zeroSpeedFinalizingJob, throttledJob, indeterminateJob, pausedJobWithStaleSpeed, zeroSpeedJob] {
            XCTAssertNil(nilJob.estimatedDurationRemaining)
        }

        // 14. IEEE 754 progressPercentage rounding precision & active 99% cap
        let precisionTestCases: [(ratio: Double, expectedPercent: Int)] = [
            (0.29, 29), (0.57, 57), (0.58, 58), (0.991, 99), (0.999, 99), (1.0, 99)
        ]
        for testCase in precisionTestCases {
            let pJob = TransferJobInfo(
                id: "precision-\(testCase.expectedPercent)", state: .running,
                totalBytes: 1000, transferredBytes: UInt64(testCase.ratio * 1000.0),
                currentSpeedMbS: 1.0, progressRatio: testCase.ratio
            )
            XCTAssertEqual(pJob.progressPercentage, testCase.expectedPercent)
        }

        // 15. Initializers, preview mocks, samples, and path parsing
        let defaultJob = TransferJobInfo(id: "default-job")
        XCTAssertEqual(defaultJob.state, .pending)
        XCTAssertTrue(defaultJob.isQueued && !defaultJob.hasStarted)
        XCTAssertEqual(defaultJob.localizedStatusTitle, "Pending")

        let mock = TransferJobInfo.previewMock(id: "mock-1")
        XCTAssertEqual(mock.localizedStatusTitle, "Running")
        XCTAssertEqual(flushingJob.localizedStatusTitle, "Finalizing")
        XCTAssertEqual(TransferJobInfo.previewSamples.count, 7)

        // Path parsing, RFC 3986 query/fragment stripping, and UTF-8 percent decoding
        let posixJob = TransferJobInfo(id: "j1", sourceUri: "/Users/dev/Archive.tar.gz")
        let winJob = TransferJobInfo(id: "j2", sourceUri: "C:\\Users\\dev\\Archive.zip")
        let slashJob = TransferJobInfo(id: "j3", sourceUri: "/Users/dev/Folder///")
        let emptyUriJob = TransferJobInfo(id: "fallback-id", sourceUri: "///")
        let rfcJob = TransferJobInfo(id: "r1", sourceUri: "file:///Volumes/USB/Archive.zip?token=xyz#part1")
        let encodedJob = TransferJobInfo(id: "r2", sourceUri: "file:///Volumes/USB/My%20Archive%202026.zip")
        let rootSchemeJob = TransferJobInfo(id: "pipe-id", sourceUri: "pipe:///")
        XCTAssertEqual(posixJob.fileName, "Archive.tar.gz")
        XCTAssertEqual(winJob.fileName, "Archive.zip")
        XCTAssertEqual(slashJob.fileName, "Folder")
        XCTAssertEqual(emptyUriJob.fileName, "fallback-id")
        XCTAssertEqual(rfcJob.fileName, "Archive.zip")
        XCTAssertEqual(encodedJob.fileName, "My Archive 2026.zip")
        XCTAssertEqual(rootSchemeJob.fileName, "pipe-id")

        // US & Commonwealth orthography and fast-path matching
        XCTAssertEqual(TransferState.Kind("canceled"), .cancelled)
        XCTAssertEqual(TransferState.Kind("Canceled"), .cancelled)
        XCTAssertEqual(TransferState.Kind("Cancelled"), .cancelled)
        XCTAssertEqual(TransferState.Kind("Running"), .running)
        XCTAssertTrue(whitespaceReasonState.canRetry && whitespaceReasonState.isRetryable)
        XCTAssertFalse(activeJob.isStalled)

        // TransferPriority & TransferDirection domain ergonomics
        let uploadJob = TransferJobInfo(id: "up", sourceUri: "file:///a.zip", destinationUri: "mtp://dev/a.zip", priority: "High")
        let pipeJob = TransferJobInfo(id: "pipe", sourceUri: "pipe:///stream", destinationUri: "file:///out.zip")
        XCTAssertEqual(uploadJob.transferPriority, TransferPriority.high)
        XCTAssertTrue(TransferPriority.critical < TransferPriority.high)
        XCTAssertEqual(uploadJob.direction, TransferDirection.upload)
        XCTAssertTrue(uploadJob.isUpload && uploadJob.isDeviceTransfer && uploadJob.supportsResume)
        XCTAssertEqual(pipeJob.direction, TransferDirection.virtual)
        XCTAssertFalse(pipeJob.supportsResume)

        // isSemanticallyEqual throttled deduplication
        let activeClone = TransferJobInfo(
            id: activeJob.id, state: activeJob.state, totalBytes: activeJob.totalBytes,
            transferredBytes: activeJob.transferredBytes, currentSpeedMbS: activeJob.currentSpeedMbS + 0.0001,
            progressRatio: activeJob.progressRatio + 0.001
        )
        XCTAssertTrue(activeJob.isSemanticallyEqual(to: activeClone))
    }
}
