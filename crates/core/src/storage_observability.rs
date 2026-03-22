use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub(crate) enum StorageOperation {
    InitializeSchema,
    ScanFacts,
    WriteFact,
    ReplaceFactRow,
    GetIdempotency,
    WriteFactAndIdempotency,
    #[cfg(feature = "contradiction")]
    ContradictionCheckedWrite,
    #[cfg(feature = "vector")]
    WriteFactWithEmbedding,
    #[cfg(feature = "vector")]
    EmbeddingRows,
    #[cfg_attr(not(test), allow(dead_code))]
    Compact,
    #[cfg(feature = "contradiction")]
    LoadPredicateRegistryEntries,
    #[cfg(feature = "contradiction")]
    WritePredicateRegistryEntry,
    #[cfg(feature = "uncertainty")]
    LoadVolatilityRegistryEntries,
    #[cfg(feature = "uncertainty")]
    LoadSourceWeightRegistryEntries,
    #[cfg(feature = "uncertainty")]
    WriteVolatilityRegistryEntry,
    #[cfg(feature = "uncertainty")]
    WriteSourceWeightRegistryEntry,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct StorageEvent {
    pub(crate) operation: StorageOperation,
    pub(crate) duration: Duration,
    pub(crate) rows_scanned: usize,
    pub(crate) success: bool,
}

pub(crate) trait StorageObserver: Send + Sync {
    fn on_event(&self, event: StorageEvent);
}

#[derive(Default)]
pub(crate) struct NoopStorageObserver;

impl StorageObserver for NoopStorageObserver {
    fn on_event(&self, _event: StorageEvent) {}
}

pub(crate) fn noop_observer() -> Arc<dyn StorageObserver> {
    Arc::new(NoopStorageObserver)
}
