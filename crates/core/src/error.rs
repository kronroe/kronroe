//! Kronroe-native error system.
//!
//! Zero external dependencies. Provides:
//! - Stable numeric [`ErrorCode`]s for FFI consumers (iOS, Android, WASM, MCP)
//! - Built-in error context chaining (replaces `anyhow::Context`)
//! - Cold-path optimisation on all constructors
//! - Predicate methods for ergonomic matching without exposing internals
//! - Structured metadata accessors (contradictions, schema versions)

use std::fmt;

#[cfg(feature = "contradiction")]
use crate::contradiction::Contradiction;

// ---------------------------------------------------------------------------
// ErrorCode — stable numeric codes for FFI surfaces
// ---------------------------------------------------------------------------

/// Stable numeric error codes that remain constant across Kronroe releases.
///
/// These codes are designed for FFI consumers who cannot pattern-match on Rust
/// enums. The numbering scheme groups errors by category:
///
/// - `1xxx` — Storage and I/O
/// - `2xxx` — Validation (bad input)
/// - `3xxx` — Query and search
/// - `4xxx` — Temporal and contradiction
/// - `9xxx` — Internal / unexpected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
#[non_exhaustive]
pub enum ErrorCode {
    // 1xxx — Storage / IO
    /// File or storage backend error.
    Storage = 1001,
    /// JSON or data serialization failure.
    Serialization = 1002,
    /// On-disk schema version does not match this build.
    SchemaMismatch = 1003,

    // 2xxx — Validation
    /// Requested entity or fact was not found.
    NotFound = 2001,
    /// Fact ID string is malformed.
    InvalidFactId = 2002,
    /// Embedding vector is invalid (empty, wrong dimension, non-finite).
    InvalidEmbedding = 2003,

    // 3xxx — Query
    /// Search or query error.
    Search = 3001,

    // 4xxx — Temporal / contradiction
    /// Fact storage was rejected due to contradiction(s).
    #[cfg(feature = "contradiction")]
    ContradictionRejected = 4001,

    // 9xxx — Internal
    /// Unexpected internal error (lock poisoned, overflow, etc.).
    Internal = 9001,
}

impl ErrorCode {
    /// Returns the numeric code as a `u16`.
    #[inline]
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:04}", *self as u16)
    }
}

// ---------------------------------------------------------------------------
// KronroeError — the public error type
// ---------------------------------------------------------------------------

/// The error type for all Kronroe operations.
///
/// `KronroeError` is an opaque struct — not an enum. Inspect errors via:
/// - [`.code()`](Self::code) for a stable numeric code (safe across FFI)
/// - [`.message()`](Self::message) for the human-readable description
/// - [`.is_storage()`](Self::is_storage), [`.is_not_found()`](Self::is_not_found), etc. for matching
/// - [`.contradictions()`](Self::contradictions), [`.schema_versions()`](Self::schema_versions) for structured data
/// - [`.context()`](Self::context) for chaining additional context
pub struct KronroeError {
    /// Boxed inner — keeps `Result<T, KronroeError>` pointer-sized.
    inner: Box<ErrorInner>,
}

struct ErrorInner {
    kind: ErrorKind,
    /// Optional context chain (like anyhow, but zero-dep).
    cause: Option<KronroeError>,
}

enum ErrorKind {
    Storage(String),
    Serialization(serde_json::Error),
    NotFound(String),
    Search(String),
    InvalidFactId(String),
    InvalidEmbedding(String),
    Internal(String),
    #[cfg(feature = "contradiction")]
    ContradictionRejected(Vec<Contradiction>),
    SchemaMismatch {
        found: u64,
        expected: u64,
    },
}

// Compile-time assertions: KronroeError must be Send + Sync for use across
// threads (TemporalGraph uses Mutex, Python bindings use py.allow_threads()).
// If a future ErrorKind variant adds a !Send or !Sync type, this will fail.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<KronroeError>();
};

// ---------------------------------------------------------------------------
// Constructors — all #[cold] #[inline(never)] to optimise the happy path
// ---------------------------------------------------------------------------

impl KronroeError {
    /// Storage or I/O error.
    #[cold]
    #[inline(never)]
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::from_kind(ErrorKind::Storage(msg.into()))
    }

    /// JSON serialization or deserialization error.
    #[cold]
    #[inline(never)]
    pub fn serialization(err: serde_json::Error) -> Self {
        Self::from_kind(ErrorKind::Serialization(err))
    }

    /// Entity or fact not found.
    #[cold]
    #[inline(never)]
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::from_kind(ErrorKind::NotFound(msg.into()))
    }

    /// Search or query error.
    #[cold]
    #[inline(never)]
    pub fn search(msg: impl Into<String>) -> Self {
        Self::from_kind(ErrorKind::Search(msg.into()))
    }

    /// Malformed fact ID.
    #[cold]
    #[inline(never)]
    pub fn invalid_fact_id(msg: impl Into<String>) -> Self {
        Self::from_kind(ErrorKind::InvalidFactId(msg.into()))
    }

    /// Invalid embedding vector.
    #[cold]
    #[inline(never)]
    pub fn invalid_embedding(msg: impl Into<String>) -> Self {
        Self::from_kind(ErrorKind::InvalidEmbedding(msg.into()))
    }

    /// Internal error (lock poisoned, arithmetic overflow, etc.).
    #[cold]
    #[inline(never)]
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::from_kind(ErrorKind::Internal(msg.into()))
    }

    /// Fact rejected because contradiction(s) were detected.
    #[cfg(feature = "contradiction")]
    #[cold]
    #[inline(never)]
    pub fn contradiction_rejected(contradictions: Vec<Contradiction>) -> Self {
        Self::from_kind(ErrorKind::ContradictionRejected(contradictions))
    }

    /// Schema version mismatch between on-disk format and this build.
    #[cold]
    #[inline(never)]
    pub fn schema_mismatch(found: u64, expected: u64) -> Self {
        Self::from_kind(ErrorKind::SchemaMismatch { found, expected })
    }

    #[inline]
    fn from_kind(kind: ErrorKind) -> Self {
        Self {
            inner: Box::new(ErrorInner { kind, cause: None }),
        }
    }
}

// ---------------------------------------------------------------------------
// Context chaining — replaces anyhow::Context
// ---------------------------------------------------------------------------

impl KronroeError {
    /// Wrap this error with additional context, forming a chain.
    ///
    /// The resulting error preserves the **same error category** as the
    /// original, and its [`Display`] output walks the chain with `: `
    /// separators (matching the `anyhow` convention).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// db.open(path)
    ///     .map_err(|e| e.context("failed to open kronroe database"))
    /// ```
    #[cold]
    #[inline(never)]
    pub fn context(self, msg: impl Into<String>) -> Self {
        let kind = match self.code() {
            ErrorCode::Storage | ErrorCode::Serialization | ErrorCode::SchemaMismatch => {
                ErrorKind::Storage(msg.into())
            }
            ErrorCode::NotFound => ErrorKind::NotFound(msg.into()),
            ErrorCode::InvalidFactId => ErrorKind::InvalidFactId(msg.into()),
            ErrorCode::InvalidEmbedding => ErrorKind::InvalidEmbedding(msg.into()),
            ErrorCode::Search => ErrorKind::Search(msg.into()),
            ErrorCode::Internal => ErrorKind::Internal(msg.into()),
            // ContradictionRejected wraps as Internal (the Vec<Contradiction>
            // cannot be carried through a string context). The original
            // contradiction data remains accessible via .cause().contradictions().
            #[cfg(feature = "contradiction")]
            ErrorCode::ContradictionRejected => ErrorKind::Internal(msg.into()),
        };
        Self {
            inner: Box::new(ErrorInner {
                kind,
                cause: Some(self),
            }),
        }
    }
}

/// Extension trait to add `.context()` to `Result<T, KronroeError>`.
///
/// This mirrors the ergonomics of `anyhow::Context` without the dependency.
pub trait ErrorContext<T> {
    /// If the result is `Err`, wrap the error with additional context.
    fn context(self, msg: impl Into<String>) -> std::result::Result<T, KronroeError>;

    /// Lazy version — only evaluates the message closure on error.
    fn with_context(self, f: impl FnOnce() -> String) -> std::result::Result<T, KronroeError>;
}

impl<T> ErrorContext<T> for std::result::Result<T, KronroeError> {
    #[inline]
    fn context(self, msg: impl Into<String>) -> std::result::Result<T, KronroeError> {
        self.map_err(|e| e.context(msg))
    }

    #[inline]
    fn with_context(self, f: impl FnOnce() -> String) -> std::result::Result<T, KronroeError> {
        self.map_err(|e| e.context(f()))
    }
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

impl KronroeError {
    /// Returns the stable numeric error code.
    #[inline]
    pub fn code(&self) -> ErrorCode {
        match &self.inner.kind {
            ErrorKind::Storage(_) => ErrorCode::Storage,
            ErrorKind::Serialization(_) => ErrorCode::Serialization,
            ErrorKind::NotFound(_) => ErrorCode::NotFound,
            ErrorKind::Search(_) => ErrorCode::Search,
            ErrorKind::InvalidFactId(_) => ErrorCode::InvalidFactId,
            ErrorKind::InvalidEmbedding(_) => ErrorCode::InvalidEmbedding,
            ErrorKind::Internal(_) => ErrorCode::Internal,
            #[cfg(feature = "contradiction")]
            ErrorKind::ContradictionRejected(_) => ErrorCode::ContradictionRejected,
            ErrorKind::SchemaMismatch { .. } => ErrorCode::SchemaMismatch,
        }
    }

    /// Returns the human-readable message for this error (without the context chain).
    pub fn message(&self) -> String {
        self.kind_display()
    }

    /// Returns the contradictions that caused this error, if applicable.
    #[cfg(feature = "contradiction")]
    pub fn contradictions(&self) -> Option<&[Contradiction]> {
        match &self.inner.kind {
            ErrorKind::ContradictionRejected(cs) => Some(cs),
            _ => None,
        }
    }

    /// Returns the `(found, expected)` schema versions, if this is a schema mismatch error.
    pub fn schema_versions(&self) -> Option<(u64, u64)> {
        match &self.inner.kind {
            ErrorKind::SchemaMismatch { found, expected } => Some((*found, *expected)),
            _ => None,
        }
    }

    /// Returns the chained cause, if context was added.
    pub fn cause(&self) -> Option<&KronroeError> {
        self.inner.cause.as_ref()
    }

    /// Walks the full error chain (self → cause → cause's cause → ...).
    pub fn chain(&self) -> ErrorChain<'_> {
        ErrorChain {
            current: Some(self),
        }
    }
}

/// Iterator over the error context chain.
pub struct ErrorChain<'a> {
    current: Option<&'a KronroeError>,
}

impl<'a> Iterator for ErrorChain<'a> {
    type Item = &'a KronroeError;

    fn next(&mut self) -> Option<Self::Item> {
        let err = self.current?;
        self.current = err.inner.cause.as_ref();
        Some(err)
    }
}

// ---------------------------------------------------------------------------
// Predicates — for ergonomic matching in tests and application code
// ---------------------------------------------------------------------------

impl KronroeError {
    /// True if this is a storage/IO error.
    #[inline]
    pub fn is_storage(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::Storage(_))
    }

    /// True if this is a serialization error.
    #[inline]
    pub fn is_serialization(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::Serialization(_))
    }

    /// True if this is a not-found error.
    #[inline]
    pub fn is_not_found(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::NotFound(_))
    }

    /// True if this is a search/query error.
    #[inline]
    pub fn is_search(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::Search(_))
    }

    /// True if this is an invalid fact ID error.
    #[inline]
    pub fn is_invalid_fact_id(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::InvalidFactId(_))
    }

    /// True if this is an invalid embedding error.
    #[inline]
    pub fn is_invalid_embedding(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::InvalidEmbedding(_))
    }

    /// True if this is an internal error.
    #[inline]
    pub fn is_internal(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::Internal(_))
    }

    /// True if this is a contradiction rejection error.
    #[cfg(feature = "contradiction")]
    #[inline]
    pub fn is_contradiction_rejected(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::ContradictionRejected(_))
    }

    /// True if this is a schema mismatch error.
    #[inline]
    pub fn is_schema_mismatch(&self) -> bool {
        matches!(self.inner.kind, ErrorKind::SchemaMismatch { .. })
    }
}

// ---------------------------------------------------------------------------
// Display — walks the context chain with `: ` separators
// ---------------------------------------------------------------------------

impl KronroeError {
    /// Format just this error's kind to the given formatter (no chain).
    fn fmt_kind(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner.kind {
            ErrorKind::Storage(msg) => write!(f, "storage error: {msg}"),
            ErrorKind::Serialization(err) => write!(f, "serialization error: {err}"),
            ErrorKind::NotFound(msg) => write!(f, "not found: {msg}"),
            ErrorKind::Search(msg) => write!(f, "search error: {msg}"),
            ErrorKind::InvalidFactId(msg) => write!(f, "invalid fact id: {msg}"),
            ErrorKind::InvalidEmbedding(msg) => write!(f, "invalid embedding: {msg}"),
            ErrorKind::Internal(msg) => write!(f, "internal error: {msg}"),
            #[cfg(feature = "contradiction")]
            ErrorKind::ContradictionRejected(_) => {
                write!(f, "fact rejected: contradiction(s) detected")
            }
            ErrorKind::SchemaMismatch { found, expected } => {
                write!(
                    f,
                    "schema version mismatch: file has version {found}, \
                     this build expects version {expected}; \
                     see https://github.com/kronroe/kronroe for migration guidance"
                )
            }
        }
    }

    /// Returns the human-readable message for this node only (no chain), as a `String`.
    fn kind_display(&self) -> String {
        // Use fmt_kind via Display adapter to avoid duplicating the match.
        struct KindAdapter<'a>(&'a KronroeError);
        impl fmt::Display for KindAdapter<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt_kind(f)
            }
        }
        KindAdapter(self).to_string()
    }
}

impl fmt::Display for KronroeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Walk the chain: outermost context first, root cause last.
        let mut first = true;
        for err in self.chain() {
            if !first {
                f.write_str(": ")?;
            }
            first = false;
            err.fmt_kind(f)?;
        }
        Ok(())
    }
}

impl fmt::Debug for KronroeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Debug shows: KronroeError(E1001, "storage error: ...")
        write!(f, "KronroeError({}, {:?})", self.code(), self.to_string())
    }
}

// ---------------------------------------------------------------------------
// std::error::Error
// ---------------------------------------------------------------------------

impl std::error::Error for KronroeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Context chain takes priority: if this error wraps a cause,
        // expose it so standard error-chain walkers (Report, eyre, etc.) work.
        if let Some(cause) = &self.inner.cause {
            return Some(cause);
        }
        // For leaf nodes, expose the underlying serde_json::Error if present.
        match &self.inner.kind {
            ErrorKind::Serialization(err) => Some(err),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// From conversions
// ---------------------------------------------------------------------------

impl From<serde_json::Error> for KronroeError {
    #[cold]
    #[inline(never)]
    fn from(err: serde_json::Error) -> Self {
        Self::serialization(err)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_numbering_is_stable() {
        assert_eq!(ErrorCode::Storage.as_u16(), 1001);
        assert_eq!(ErrorCode::Serialization.as_u16(), 1002);
        assert_eq!(ErrorCode::SchemaMismatch.as_u16(), 1003);
        assert_eq!(ErrorCode::NotFound.as_u16(), 2001);
        assert_eq!(ErrorCode::InvalidFactId.as_u16(), 2002);
        assert_eq!(ErrorCode::InvalidEmbedding.as_u16(), 2003);
        assert_eq!(ErrorCode::Search.as_u16(), 3001);
        assert_eq!(ErrorCode::Internal.as_u16(), 9001);
    }

    #[test]
    fn error_code_display_format() {
        assert_eq!(ErrorCode::Storage.to_string(), "E1001");
        assert_eq!(ErrorCode::Internal.to_string(), "E9001");
    }

    #[test]
    fn storage_error_roundtrip() {
        let err = KronroeError::storage("disk full");
        assert!(err.is_storage());
        assert_eq!(err.code(), ErrorCode::Storage);
        assert!(err.message().contains("disk full"));
        assert!(err.to_string().contains("disk full"));
    }

    #[test]
    fn not_found_error() {
        let err = KronroeError::not_found("fact id kf_abc");
        assert!(err.is_not_found());
        assert_eq!(err.code(), ErrorCode::NotFound);
        assert!(err.message().contains("kf_abc"));
    }

    #[test]
    fn schema_mismatch_structured_access() {
        let err = KronroeError::schema_mismatch(999, 2);
        assert!(err.is_schema_mismatch());
        assert_eq!(err.code(), ErrorCode::SchemaMismatch);
        assert_eq!(err.schema_versions(), Some((999, 2)));
        assert!(err.to_string().contains("999"));
        assert!(err.to_string().contains("migration guidance"));
    }

    #[test]
    fn context_chaining() {
        let root = KronroeError::storage("permission denied");
        let wrapped = root.context("failed to open database");

        // Outermost context is visible in display
        let display = wrapped.to_string();
        assert!(display.contains("failed to open database"));
        assert!(display.contains("permission denied"));
        assert!(display.contains(": "));

        // Code matches the outermost context's category
        assert!(wrapped.is_storage());

        // Chain traversal
        let chain: Vec<_> = wrapped.chain().collect();
        assert_eq!(chain.len(), 2);
        assert!(chain[0].message().contains("failed to open database"));
        assert!(chain[1].message().contains("permission denied"));
    }

    #[test]
    fn context_on_result() {
        let result: std::result::Result<(), KronroeError> = Err(KronroeError::storage("io error"));
        let wrapped = result.context("opening database");

        let err = wrapped.unwrap_err();
        assert!(err.to_string().contains("opening database"));
        assert!(err.to_string().contains("io error"));
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<String>("not-json").unwrap_err();
        let err = KronroeError::from(json_err);
        assert!(err.is_serialization());
        assert_eq!(err.code(), ErrorCode::Serialization);
    }

    #[test]
    fn debug_format_includes_code() {
        let err = KronroeError::internal("lock poisoned");
        let debug = format!("{err:?}");
        assert!(debug.contains("E9001"));
        assert!(debug.contains("lock poisoned"));
    }

    #[test]
    fn result_uses_niche_optimisation() {
        // KronroeError is Box<ErrorInner> (non-null pointer). Rust uses the null
        // niche so Result<(), KronroeError> is a single pointer — 8 bytes on 64-bit.
        // This is *better* than Result<(), Box<dyn Error>> (fat pointer = 16 bytes).
        assert_eq!(
            std::mem::size_of::<crate::Result<()>>(),
            std::mem::size_of::<*const ()>()
        );
    }

    #[cfg(feature = "contradiction")]
    #[test]
    fn contradiction_rejected_access() {
        // ContradictionRejected with empty vec (just testing the accessor path)
        let err = KronroeError::contradiction_rejected(vec![]);
        assert!(err.is_contradiction_rejected());
        assert_eq!(err.code(), ErrorCode::ContradictionRejected);
        assert_eq!(err.contradictions().unwrap().len(), 0);
    }

    #[test]
    fn source_walks_context_chain() {
        let root = KronroeError::storage("disk full");
        let wrapped = root.context("failed to flush");

        // std::error::Error::source() should return the cause
        let source = std::error::Error::source(&wrapped).unwrap();
        let downcasted = source.downcast_ref::<KronroeError>().unwrap();
        assert!(downcasted.is_storage());
        assert!(downcasted.message().contains("disk full"));
    }

    #[test]
    fn source_returns_serde_error_for_leaf() {
        let json_err = serde_json::from_str::<String>("not-json").unwrap_err();
        let err = KronroeError::from(json_err);

        // Leaf serialization node should expose the serde_json::Error
        let source = std::error::Error::source(&err).unwrap();
        assert!(source.downcast_ref::<serde_json::Error>().is_some());
    }

    #[test]
    fn predicates_are_exclusive() {
        let err = KronroeError::search("bad query");
        assert!(err.is_search());
        assert!(!err.is_storage());
        assert!(!err.is_not_found());
        assert!(!err.is_internal());
        assert!(!err.is_invalid_fact_id());
        assert!(!err.is_invalid_embedding());
        assert!(!err.is_serialization());
        assert!(!err.is_schema_mismatch());
    }
}
