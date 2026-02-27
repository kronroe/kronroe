import Foundation

/// On-device conversation memory store for Kindly Roe.
///
/// Persists user-curated moments — highlights, pins, and annotations — using
/// Kronroe's bi-temporal graph engine. All data lives on-device with zero
/// network latency. Full temporal history is preserved automatically.
///
/// ## Usage
///
/// ```swift
/// let dbURL = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
///     .appendingPathComponent("kindlyroe-memory.kronroe")
/// let graph = try KronroeGraph.open(url: dbURL)
/// let store = KronroeMemoryStore(graph: graph)
///
/// // Record a highlight when the user highlights a message
/// try store.recordHighlight(messageId: message.id, category: "rights")
///
/// // Record a pin
/// try store.recordPin(messageId: message.id, label: "Equality Act adjustments")
///
/// // Record an annotation
/// try store.recordAnnotation(messageId: message.id, text: "Ask at next GP appointment")
///
/// // Restore on app launch
/// let json = try store.factsAbout(messageId: message.id)
/// ```
public final class KronroeMemoryStore {
    private let graph: KronroeGraph

    public init(graph: KronroeGraph) {
        self.graph = graph
    }

    // MARK: - Highlights

    /// Records that a message was highlighted with a given category.
    ///
    /// - Parameters:
    ///   - messageId: The UUID of the chat message.
    ///   - category: The highlight category string (e.g. `"important"`, `"work"`,
    ///     `"behavior"`, `"rights"`). Pass `HighlightCategory.rawValue` directly.
    public func recordHighlight(messageId: UUID, category: String) throws {
        try graph.assert(
            subject: "message:\(messageId.uuidString)",
            predicate: "highlight",
            object: category
        )
    }

    // MARK: - Pins

    /// Records that a message was pinned with a descriptive label.
    ///
    /// - Parameters:
    ///   - messageId: The UUID of the chat message.
    ///   - label: The pin label (auto-generated or user-written).
    public func recordPin(messageId: UUID, label: String) throws {
        try graph.assert(
            subject: "message:\(messageId.uuidString)",
            predicate: "pin",
            object: label
        )
    }

    // MARK: - Annotations

    /// Records a free-text annotation on a message.
    ///
    /// - Parameters:
    ///   - messageId: The UUID of the chat message.
    ///   - text: The annotation text.
    public func recordAnnotation(messageId: UUID, text: String) throws {
        try graph.assert(
            subject: "message:\(messageId.uuidString)",
            predicate: "annotation",
            object: text
        )
    }

    // MARK: - Recall

    /// Returns all stored facts about a message as a JSON string.
    ///
    /// The result is a JSON array of Kronroe `Fact` objects. Each fact has:
    /// - `subject` — `"message:<UUID>"`
    /// - `predicate` — `"highlight"`, `"pin"`, or `"annotation"`
    /// - `object` — `{ "type": "Text", "value": "<stored string>" }`
    /// - `recorded_at` — ISO-8601 timestamp of when it was stored
    /// - `valid_from` — same as `recorded_at` for user-asserted facts
    ///
    /// Returns an empty JSON array (`"[]"`) if no facts have been stored yet.
    public func factsAbout(messageId: UUID) throws -> String {
        return try graph.factsAboutJSON(entity: "message:\(messageId.uuidString)")
    }
}
