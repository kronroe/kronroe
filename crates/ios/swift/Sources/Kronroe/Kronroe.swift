import Foundation
import KronroeFFI

public enum KronroeError: Error, LocalizedError {
    case openFailed(String)
    case assertFailed(String)
    case queryFailed(String)
    case invalidUTF8

    public var errorDescription: String? {
        switch self {
        case .openFailed(let msg): return "Open failed: \(msg)"
        case .assertFailed(let msg): return "Assert failed: \(msg)"
        case .queryFailed(let msg): return "Query failed: \(msg)"
        case .invalidUTF8: return "Received invalid UTF-8 from Kronroe"
        }
    }
}

public final class KronroeGraph {
    private var handle: OpaquePointer?

    private init(handle: OpaquePointer) {
        self.handle = handle
    }

    deinit {
        if let handle {
            kronroe_graph_close(handle)
        }
    }

    public static func open(url: URL) throws -> KronroeGraph {
        let path = url.path
        guard let handle = path.withCString({ cPath in
            kronroe_graph_open(cPath)
        }) else {
            throw KronroeError.openFailed(Self.lastErrorMessage())
        }
        return KronroeGraph(handle: handle)
    }

    public static func openInMemory() throws -> KronroeGraph {
        guard let handle = kronroe_graph_open_in_memory() else {
            throw KronroeError.openFailed(Self.lastErrorMessage())
        }
        return KronroeGraph(handle: handle)
    }

    public func assert(subject: String, predicate: String, object: String) throws {
        guard let handle else {
            throw KronroeError.assertFailed("graph handle is nil")
        }
        let ok = subject.withCString { cSubject in
            predicate.withCString { cPredicate in
                object.withCString { cObject in
                    kronroe_graph_assert_text(handle, cSubject, cPredicate, cObject)
                }
            }
        }
        if !ok {
            throw KronroeError.assertFailed(Self.lastErrorMessage())
        }
    }

    public func factsAboutJSON(entity: String) throws -> String {
        guard let handle else {
            throw KronroeError.queryFailed("graph handle is nil")
        }
        guard let raw = entity.withCString({ cEntity in
            kronroe_graph_facts_about_json(handle, cEntity)
        }) else {
            throw KronroeError.queryFailed(Self.lastErrorMessage())
        }
        defer { kronroe_string_free(raw) }
        guard let s = String(validatingUTF8: raw) else {
            throw KronroeError.invalidUTF8
        }
        return s
    }

    private static func lastErrorMessage() -> String {
        guard let ptr = kronroe_last_error_message() else {
            return "unknown error"
        }
        return String(cString: ptr)
    }
}
