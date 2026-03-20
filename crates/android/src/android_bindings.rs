use chrono::Utc;
use kronroe::TemporalGraph;
use std::cell::RefCell;

// ---------------------------------------------------------------------------
// Layer 1 — Pure Rust handle (testable on host without JVM or NDK)
// ---------------------------------------------------------------------------

pub(crate) struct KronroeGraphHandle {
    graph: TemporalGraph,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn set_last_error(msg: String) {
    // Strip null bytes so JNI new_string never fails on embedded nulls.
    let sanitized = msg.replace('\0', "\\0");
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = Some(sanitized);
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

impl KronroeGraphHandle {
    fn open(path: &str) -> Result<Self, String> {
        TemporalGraph::open(path)
            .map(|graph| Self { graph })
            .map_err(|e| e.to_string())
    }

    fn open_in_memory() -> Result<Self, String> {
        TemporalGraph::open_in_memory()
            .map(|graph| Self { graph })
            .map_err(|e| e.to_string())
    }

    fn assert_text(&self, subject: &str, predicate: &str, object: &str) -> Result<bool, String> {
        self.graph
            .assert_fact(subject, predicate, object.to_string(), Utc::now())
            .map(|_| true)
            .map_err(|e| e.to_string())
    }

    fn facts_about_json(&self, entity: &str) -> Result<String, String> {
        let facts = self
            .graph
            .all_facts_about(entity)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&facts).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Layer 2 — JNI bridge (thin wrappers delegating to KronroeGraphHandle)
// ---------------------------------------------------------------------------

mod jni_bridge {
    use super::*;
    use jni::objects::{JClass, JString};
    use jni::sys::{jboolean, jlong, jstring, JNI_FALSE, JNI_TRUE};
    use jni::JNIEnv;

    /// Convert a JNI handle (jlong) back to a reference.
    ///
    /// # Safety
    /// Caller must guarantee `handle` was returned by `nativeOpen` or
    /// `nativeOpenInMemory` and has not been closed via `nativeClose`.
    ///
    /// The `'static` lifetime is a necessary fiction for JNI interop — the
    /// reference is only valid as long as the handle is open. The Kotlin
    /// `KronroeGraph` wrapper enforces this via a `@Volatile closed` flag
    /// and `@Synchronized close()`. Direct JNI callers bypassing the wrapper
    /// must uphold this invariant manually.
    unsafe fn handle_ref(handle: jlong) -> &'static KronroeGraphHandle {
        unsafe { &*(handle as *const KronroeGraphHandle) }
    }

    /// Extract a Rust `String` from a JNI `JString`.
    fn jstring_to_string(env: &mut JNIEnv, s: &JString) -> Result<String, String> {
        env.get_string(s)
            .map(|js| js.into())
            .map_err(|e| e.to_string())
    }

    #[no_mangle]
    pub extern "system" fn Java_com_kronroe_KronroeGraph_nativeOpenInMemory(
        _env: JNIEnv,
        _class: JClass,
    ) -> jlong {
        clear_last_error();
        match KronroeGraphHandle::open_in_memory() {
            Ok(handle) => Box::into_raw(Box::new(handle)) as jlong,
            Err(msg) => {
                set_last_error(msg);
                0
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_kronroe_KronroeGraph_nativeOpen(
        mut env: JNIEnv,
        _class: JClass,
        path: JString,
    ) -> jlong {
        clear_last_error();
        let path = match jstring_to_string(&mut env, &path) {
            Ok(v) => v,
            Err(msg) => {
                set_last_error(msg);
                return 0;
            }
        };
        match KronroeGraphHandle::open(&path) {
            Ok(handle) => Box::into_raw(Box::new(handle)) as jlong,
            Err(msg) => {
                set_last_error(msg);
                0
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_kronroe_KronroeGraph_nativeClose(
        _env: JNIEnv,
        _class: JClass,
        handle: jlong,
    ) {
        if handle == 0 {
            return;
        }
        unsafe {
            drop(Box::from_raw(handle as *mut KronroeGraphHandle));
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_kronroe_KronroeGraph_nativeAssertText(
        mut env: JNIEnv,
        _class: JClass,
        handle: jlong,
        subject: JString,
        predicate: JString,
        object: JString,
    ) -> jboolean {
        clear_last_error();
        if handle == 0 {
            set_last_error("graph handle is null".to_string());
            return JNI_FALSE;
        }

        let subject = match jstring_to_string(&mut env, &subject) {
            Ok(v) => v,
            Err(msg) => {
                set_last_error(msg);
                return JNI_FALSE;
            }
        };
        let predicate = match jstring_to_string(&mut env, &predicate) {
            Ok(v) => v,
            Err(msg) => {
                set_last_error(msg);
                return JNI_FALSE;
            }
        };
        let object = match jstring_to_string(&mut env, &object) {
            Ok(v) => v,
            Err(msg) => {
                set_last_error(msg);
                return JNI_FALSE;
            }
        };

        let graph = unsafe { handle_ref(handle) };
        match graph.assert_text(&subject, &predicate, &object) {
            Ok(_) => JNI_TRUE,
            Err(msg) => {
                set_last_error(msg);
                JNI_FALSE
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_kronroe_KronroeGraph_nativeFactsAboutJson(
        mut env: JNIEnv,
        _class: JClass,
        handle: jlong,
        entity: JString,
    ) -> jstring {
        clear_last_error();
        if handle == 0 {
            set_last_error("graph handle is null".to_string());
            return std::ptr::null_mut();
        }

        let entity = match jstring_to_string(&mut env, &entity) {
            Ok(v) => v,
            Err(msg) => {
                set_last_error(msg);
                return std::ptr::null_mut();
            }
        };

        let graph = unsafe { handle_ref(handle) };
        match graph.facts_about_json(&entity) {
            Ok(json) => match env.new_string(&json) {
                Ok(js) => js.into_raw(),
                Err(e) => {
                    set_last_error(e.to_string());
                    std::ptr::null_mut()
                }
            },
            Err(msg) => {
                set_last_error(msg);
                std::ptr::null_mut()
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_kronroe_KronroeGraph_nativeLastErrorMessage(
        env: JNIEnv,
        _class: JClass,
    ) -> jstring {
        let msg = LAST_ERROR.with(|cell| cell.borrow().clone());
        match msg {
            Some(s) => match env.new_string(&s) {
                Ok(js) => js.into_raw(),
                Err(_) => {
                    // Fallback: try a plain error message so the caller at least
                    // knows something went wrong, rather than returning null
                    // (which is indistinguishable from "no error").
                    env.new_string("kronroe: error message could not be encoded")
                        .map(|js| js.into_raw())
                        .unwrap_or(std::ptr::null_mut())
                }
            },
            None => std::ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — exercise Layer 1 directly (no JVM needed)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_assert_query_roundtrip() {
        let handle = KronroeGraphHandle::open_in_memory().expect("open_in_memory");
        handle
            .assert_text("Freya", "attends", "Sunrise Primary")
            .expect("assert");
        let json = handle.facts_about_json("Freya").expect("facts_about");
        let facts: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let arr = facts.as_array().expect("json array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["subject"], "Freya");
        assert_eq!(arr[0]["predicate"], "attends");
        assert_eq!(arr[0]["object"]["value"], "Sunrise Primary");
    }

    #[test]
    fn open_file_backed_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir
            .path()
            .join("test.kronroe")
            .to_string_lossy()
            .to_string();
        let handle = KronroeGraphHandle::open(&path).expect("open");
        handle
            .assert_text("alice", "works_at", "Acme")
            .expect("assert");
        let json = handle.facts_about_json("alice").expect("facts_about");
        let facts: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let arr = facts.as_array().expect("json array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["subject"], "alice");
    }

    #[test]
    fn error_propagation_empty_entity() {
        let handle = KronroeGraphHandle::open_in_memory().expect("open_in_memory");
        // Empty entity should return an empty array, not error
        let json = handle.facts_about_json("").expect("facts_about");
        let facts: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let arr = facts.as_array().expect("json array");
        assert!(arr.is_empty());
    }

    #[test]
    fn last_error_set_and_cleared() {
        // Verify the LAST_ERROR thread-local works the same as iOS
        clear_last_error();
        let msg = LAST_ERROR.with(|cell| cell.borrow().clone());
        assert!(msg.is_none(), "error should be cleared");

        set_last_error("graph handle is null".to_string());
        let msg = LAST_ERROR.with(|cell| cell.borrow().clone());
        assert_eq!(msg.as_deref(), Some("graph handle is null"));

        clear_last_error();
        let msg = LAST_ERROR.with(|cell| cell.borrow().clone());
        assert!(msg.is_none(), "error should be cleared again");
    }

    #[test]
    fn last_error_sanitizes_null_bytes() {
        clear_last_error();
        set_last_error("broken\0message".to_string());

        let msg = LAST_ERROR.with(|cell| cell.borrow().clone());
        assert_eq!(msg.as_deref(), Some("broken\\0message"));
    }
}
