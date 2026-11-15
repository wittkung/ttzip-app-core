// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

import Foundation
import TTZipAppCore
import TTZipReader
import XCTest

final class ReaderInteropTests: XCTestCase {
    func testReaderProductsShareTheExistingDiskFormat() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let identity = "reader-interoperability"
        let legacy = try TTZipAppCore.ReaderLibrary(directory: directory.path)
        try legacy.savePosition(
            identity: identity,
            position: .init(chapterIndex: 7, blockIndex: 0, characterOffset: 0, chapterRatio: 0)
        )

        let reader = try TTZipReader.ReaderLibrary(directory: directory.path)
        let position = try reader.loadDocument(identity: identity).position
        XCTAssertEqual(position.chapterIndex, 7)
        XCTAssertEqual(position.chapterRatio, 0)

        let annotation = try reader.upsertAnnotation(
            identity: identity,
            annotation: .init(id: "annotation", anchor: position, quote: "quote", note: "note", color: "yellow")
        )
        let reopened = try TTZipAppCore.ReaderLibrary(directory: directory.path)
        XCTAssertEqual(try reopened.loadDocument(identity: identity).annotations.first?.id, annotation.id)
    }

    func testLegacyAndReaderErrorsRetainTheirModuleIdentity() throws {
        XCTAssertThrowsError(try TTZipAppCore.ReaderLibrary(directory: "relative")) { error in
            XCTAssertTrue(error is TTZipAppCore.ReaderError)
        }
        XCTAssertThrowsError(try TTZipReader.ReaderLibrary(directory: "relative")) { error in
            XCTAssertTrue(error is TTZipReader.ReaderError)
        }
    }
}
