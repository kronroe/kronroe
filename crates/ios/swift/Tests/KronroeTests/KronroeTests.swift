import Foundation
import XCTest
@testable import Kronroe

final class KronroeTests: XCTestCase {
    func testOpenAssertQueryRoundTrip() throws {
        let dbURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("kronroe-ios-\(UUID().uuidString).kronroe")
        let graph = try KronroeGraph.open(url: dbURL)
        try graph.assert(subject: "Freya", predicate: "attends", object: "Sunrise Primary")

        let json = try graph.factsAboutJSON(entity: "Freya")
        print("PROOF_QUERY_RESULT_JSON=\(json)")
        let data = try XCTUnwrap(json.data(using: .utf8))
        let decoded = try JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        let first = try XCTUnwrap(decoded?.first)
        XCTAssertEqual(first["subject"] as? String, "Freya")
        XCTAssertEqual(first["predicate"] as? String, "attends")
    }

    func testOpenInMemoryRoundTrip() throws {
        let graph = try KronroeGraph.openInMemory()
        try graph.assert(subject: "alice", predicate: "works_at", object: "Acme")
        let json = try graph.factsAboutJSON(entity: "alice")
        XCTAssertTrue(json.contains("\"subject\":\"alice\""))
    }

    func testFailurePathOpenInvalidPath() {
        let invalidURL = URL(fileURLWithPath: "/")
        XCTAssertThrowsError(try KronroeGraph.open(url: invalidURL))
    }

    // MARK: - KronroeMemoryStore (Kindly Roe integration proof)

    func testMemoryStoreHighlightPinAnnotationRoundTrip() throws {
        let graph = try KronroeGraph.openInMemory()
        let store = KronroeMemoryStore(graph: graph)
        let msgId = UUID()

        try store.recordHighlight(messageId: msgId, category: "rights")
        try store.recordPin(messageId: msgId, label: "Equality Act adjustments")
        try store.recordAnnotation(messageId: msgId, text: "Ask about this at next GP appointment")

        let json = try store.factsAbout(messageId: msgId)
        print("PROOF_MEMORY_STORE_JSON=\(json)")

        XCTAssertTrue(json.contains("rights"), "highlight category should be stored")
        XCTAssertTrue(json.contains("Equality Act adjustments"), "pin label should be stored")
        XCTAssertTrue(json.contains("next GP appointment"), "annotation should be stored")

        // Three facts stored for one message
        let data = try XCTUnwrap(json.data(using: .utf8))
        let decoded = try JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        XCTAssertEqual(decoded?.count, 3, "expected exactly 3 facts (highlight + pin + annotation)")
    }

    func testMemoryStoreEmptyRecallReturnsEmptyArray() throws {
        let graph = try KronroeGraph.openInMemory()
        let store = KronroeMemoryStore(graph: graph)
        let unknownId = UUID()

        let json = try store.factsAbout(messageId: unknownId)
        XCTAssertTrue(json.contains("[]") || json == "[]", "no facts stored should return empty array")
    }
}
