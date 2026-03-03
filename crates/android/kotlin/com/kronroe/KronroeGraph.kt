package com.kronroe

/**
 * Kotlin wrapper for the Kronroe temporal graph engine.
 *
 * Usage:
 *   val graph = KronroeGraph.openInMemory()
 *   graph.assertText("alice", "works_at", "Acme")
 *   val json = graph.factsAboutJson("alice")
 *   graph.close()
 */
class KronroeGraph private constructor(private var handle: Long) : AutoCloseable {

    @Volatile
    private var closed = false

    companion object {
        init {
            System.loadLibrary("kronroe_android")
        }

        fun open(path: String): KronroeGraph {
            val handle = nativeOpen(path)
            if (handle == 0L) {
                throw KronroeException(nativeLastErrorMessage() ?: "unknown error")
            }
            return KronroeGraph(handle)
        }

        fun openInMemory(): KronroeGraph {
            val handle = nativeOpenInMemory()
            if (handle == 0L) {
                throw KronroeException(nativeLastErrorMessage() ?: "unknown error")
            }
            return KronroeGraph(handle)
        }

        @JvmStatic
        private external fun nativeOpen(path: String): Long

        @JvmStatic
        private external fun nativeOpenInMemory(): Long

        @JvmStatic
        private external fun nativeClose(handle: Long)

        @JvmStatic
        private external fun nativeAssertText(
            handle: Long,
            subject: String,
            predicate: String,
            obj: String,
        ): Boolean

        @JvmStatic
        private external fun nativeFactsAboutJson(handle: Long, entity: String): String?

        @JvmStatic
        private external fun nativeLastErrorMessage(): String?
    }

    fun assertText(subject: String, predicate: String, obj: String) {
        check(!closed) { "KronroeGraph is closed" }
        val ok = nativeAssertText(handle, subject, predicate, obj)
        if (!ok) {
            throw KronroeException(nativeLastErrorMessage() ?: "assert failed")
        }
    }

    fun factsAboutJson(entity: String): String {
        check(!closed) { "KronroeGraph is closed" }
        return nativeFactsAboutJson(handle, entity)
            ?: throw KronroeException(nativeLastErrorMessage() ?: "query failed")
    }

    @Synchronized
    override fun close() {
        if (!closed) {
            closed = true
            nativeClose(handle)
            handle = 0
        }
    }

    protected fun finalize() {
        close()
    }
}

class KronroeException(message: String) : Exception(message)
