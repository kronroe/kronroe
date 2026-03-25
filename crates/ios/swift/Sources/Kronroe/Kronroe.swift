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

    /// Explicitly close the graph and release the underlying file lock.
    /// Safe to call multiple times. After calling, all other methods will throw.
    public func close() {
        if let handle {
            kronroe_graph_close(handle)
            self.handle = nil
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

    // MARK: - Extended API

    /// Assert a fact with full control over confidence, source, and valid time.
    /// - Returns: The new fact's `kf_...` ID.
    @discardableResult
    public func assertFact(
        subject: String,
        predicate: String,
        object: String,
        confidence: Float = -1.0,
        source: String? = nil,
        validFrom: String? = nil
    ) throws -> String {
        guard let handle else {
            throw KronroeError.assertFailed("graph handle is nil")
        }
        let result = subject.withCString { cSubject in
            predicate.withCString { cPredicate in
                object.withCString { cObject in
                    if let source {
                        if let validFrom {
                            return source.withCString { cSource in
                                validFrom.withCString { cValidFrom in
                                    kronroe_graph_assert_fact(handle, cSubject, cPredicate, cObject, confidence, cSource, cValidFrom)
                                }
                            }
                        } else {
                            return source.withCString { cSource in
                                kronroe_graph_assert_fact(handle, cSubject, cPredicate, cObject, confidence, cSource, nil)
                            }
                        }
                    } else if let validFrom {
                        return validFrom.withCString { cValidFrom in
                            kronroe_graph_assert_fact(handle, cSubject, cPredicate, cObject, confidence, nil, cValidFrom)
                        }
                    } else {
                        return kronroe_graph_assert_fact(handle, cSubject, cPredicate, cObject, confidence, nil, nil)
                    }
                }
            }
        }
        guard let raw = result else {
            throw KronroeError.assertFailed(Self.lastErrorMessage())
        }
        defer { kronroe_string_free(raw) }
        guard let s = String(validatingUTF8: raw) else {
            throw KronroeError.invalidUTF8
        }
        return s
    }

    /// Query currently valid facts for a specific entity and predicate.
    public func currentFactsJSON(entity: String, predicate: String) throws -> String {
        guard let handle else {
            throw KronroeError.queryFailed("graph handle is nil")
        }
        guard let raw = entity.withCString({ cEntity in
            predicate.withCString { cPredicate in
                kronroe_graph_current_facts_json(handle, cEntity, cPredicate)
            }
        }) else {
            throw KronroeError.queryFailed(Self.lastErrorMessage())
        }
        defer { kronroe_string_free(raw) }
        guard let s = String(validatingUTF8: raw) else {
            throw KronroeError.invalidUTF8
        }
        return s
    }

    /// Full-text search across all current facts.
    public func searchJSON(query: String, limit: UInt32 = 10) throws -> String {
        guard let handle else {
            throw KronroeError.queryFailed("graph handle is nil")
        }
        guard let raw = query.withCString({ cQuery in
            kronroe_graph_search_json(handle, cQuery, limit)
        }) else {
            throw KronroeError.queryFailed(Self.lastErrorMessage())
        }
        defer { kronroe_string_free(raw) }
        guard let s = String(validatingUTF8: raw) else {
            throw KronroeError.invalidUTF8
        }
        return s
    }

    /// Correct a fact: invalidate the old value, assert a new one.
    /// - Returns: The new fact's `kf_...` ID.
    @discardableResult
    public func correctFact(factId: String, newObject: String) throws -> String {
        guard let handle else {
            throw KronroeError.assertFailed("graph handle is nil")
        }
        guard let raw = factId.withCString({ cFactId in
            newObject.withCString { cNewObject in
                kronroe_graph_correct_fact(handle, cFactId, cNewObject)
            }
        }) else {
            throw KronroeError.assertFailed(Self.lastErrorMessage())
        }
        defer { kronroe_string_free(raw) }
        guard let s = String(validatingUTF8: raw) else {
            throw KronroeError.invalidUTF8
        }
        return s
    }

    /// Invalidate (retire) a fact by its ID. History is preserved.
    public func invalidateFact(factId: String) throws {
        guard let handle else {
            throw KronroeError.assertFailed("graph handle is nil")
        }
        let ok = factId.withCString { cFactId in
            kronroe_graph_invalidate_fact(handle, cFactId)
        }
        if !ok {
            throw KronroeError.assertFailed(Self.lastErrorMessage())
        }
    }

    /// Look up a single fact by its `kf_...` ID.
    public func factByIdJSON(factId: String) throws -> String {
        guard let handle else {
            throw KronroeError.queryFailed("graph handle is nil")
        }
        guard let raw = factId.withCString({ cFactId in
            kronroe_graph_fact_by_id_json(handle, cFactId)
        }) else {
            throw KronroeError.queryFailed(Self.lastErrorMessage())
        }
        defer { kronroe_string_free(raw) }
        guard let s = String(validatingUTF8: raw) else {
            throw KronroeError.invalidUTF8
        }
        return s
    }

    // MARK: - Private

    private static func lastErrorMessage() -> String {
        guard let ptr = kronroe_last_error_message() else {
            return "unknown error"
        }
        // Copy to String before freeing. The XCFramework header declares
        // `kronroe_last_error_message` as returning `const char *`, so Swift 6
        // infers `ptr` as UnsafePointer<CChar>. We own this allocation and must
        // free it via `kronroe_string_free(char *)` — UnsafeMutablePointer(mutating:)
        // is the correct Swift 6 pattern for recovering the mutable pointer.
        let message = String(cString: ptr)
        kronroe_string_free(UnsafeMutablePointer(mutating: ptr))
        return message
    }
}
