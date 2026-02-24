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
}
