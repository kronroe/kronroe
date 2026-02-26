use chrono::Utc;
use kronroe::TemporalGraph;
use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

pub struct KronroeGraphHandle {
    graph: TemporalGraph,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn set_last_error(msg: String) {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = CString::new(msg).ok();
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

fn cstr_to_string(ptr: *const c_char, field: &str) -> Result<String, String> {
    if ptr.is_null() {
        return Err(format!("{field} is null"));
    }
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| format!("{field} is not valid UTF-8"))?;
    Ok(s.to_string())
}

#[no_mangle]
/// Create an in-memory Kronroe graph handle (no file I/O).
///
/// Ideal for simulator testing and ephemeral workloads.
/// Returns NULL on error (inspect `kronroe_last_error_message`).
pub extern "C" fn kronroe_graph_open_in_memory() -> *mut KronroeGraphHandle {
    clear_last_error();
    match TemporalGraph::open_in_memory() {
        Ok(graph) => Box::into_raw(Box::new(KronroeGraphHandle { graph })),
        Err(err) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
    }
}

#[no_mangle]
/// Open/create a Kronroe graph handle.
///
/// # Safety
/// `path` must be a valid, NUL-terminated UTF-8 C string pointer.
pub unsafe extern "C" fn kronroe_graph_open(path: *const c_char) -> *mut KronroeGraphHandle {
    clear_last_error();
    let path = match cstr_to_string(path, "path") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };

    match TemporalGraph::open(&path) {
        Ok(graph) => Box::into_raw(Box::new(KronroeGraphHandle { graph })),
        Err(err) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
    }
}

#[no_mangle]
/// Close and free a graph handle.
///
/// # Safety
/// `handle` must be either NULL or a pointer returned from `kronroe_graph_open`
/// that has not already been closed.
pub unsafe extern "C" fn kronroe_graph_close(handle: *mut KronroeGraphHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(handle));
    }
}

#[no_mangle]
/// Assert a text fact on the graph.
///
/// # Safety
/// `handle` must be a valid graph handle pointer.
/// `subject`, `predicate`, and `object` must be valid NUL-terminated UTF-8 C strings.
pub unsafe extern "C" fn kronroe_graph_assert_text(
    handle: *mut KronroeGraphHandle,
    subject: *const c_char,
    predicate: *const c_char,
    object: *const c_char,
) -> bool {
    clear_last_error();
    if handle.is_null() {
        set_last_error("graph handle is null".to_string());
        return false;
    }

    let subject = match cstr_to_string(subject, "subject") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return false;
        }
    };
    let predicate = match cstr_to_string(predicate, "predicate") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return false;
        }
    };
    let object = match cstr_to_string(object, "object") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return false;
        }
    };

    let graph = unsafe { &*handle };
    match graph
        .graph
        .assert_fact(&subject, &predicate, object, Utc::now())
    {
        Ok(_) => true,
        Err(err) => {
            set_last_error(err.to_string());
            false
        }
    }
}

#[no_mangle]
/// Return all facts about an entity as a newly allocated JSON C string.
///
/// # Safety
/// `handle` must be a valid graph handle pointer.
/// `entity` must be a valid NUL-terminated UTF-8 C string.
/// The returned pointer must be released with `kronroe_string_free`.
pub unsafe extern "C" fn kronroe_graph_facts_about_json(
    handle: *mut KronroeGraphHandle,
    entity: *const c_char,
) -> *mut c_char {
    clear_last_error();
    if handle.is_null() {
        set_last_error("graph handle is null".to_string());
        return ptr::null_mut();
    }
    let entity = match cstr_to_string(entity, "entity") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };
    let graph = unsafe { &*handle };

    match graph.graph.all_facts_about(&entity) {
        Ok(facts) => match serde_json::to_string(&facts) {
            Ok(s) => match CString::new(s) {
                Ok(cs) => cs.into_raw(),
                Err(_) => {
                    set_last_error("failed to encode facts JSON".to_string());
                    ptr::null_mut()
                }
            },
            Err(err) => {
                set_last_error(err.to_string());
                ptr::null_mut()
            }
        },
        Err(err) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
    }
}

#[no_mangle]
/// Return the last error message as a newly allocated C string.
///
/// Returns NULL if no error is set.
///
/// # Safety
/// The returned pointer must be freed with `kronroe_string_free` when no
/// longer needed. Unlike the previous implementation, this returns an
/// independent allocation — the pointer remains valid even after subsequent
/// Kronroe calls that clear or overwrite the internal error state.
pub extern "C" fn kronroe_last_error_message() -> *mut c_char {
    LAST_ERROR.with(|cell| match cell.borrow().as_ref() {
        Some(msg) => {
            // Clone so the caller owns the allocation independently
            // of the thread-local lifetime.
            msg.clone().into_raw()
        }
        None => ptr::null_mut(),
    })
}

#[no_mangle]
/// Free a C string returned by Kronroe FFI.
///
/// # Safety
/// `ptr` must be either NULL or a pointer previously returned by
/// `kronroe_graph_facts_about_json` that has not yet been freed.
pub unsafe extern "C" fn kronroe_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(ptr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn c(s: &str) -> CString {
        CString::new(s).expect("test CString")
    }

    fn unique_db_path() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let mut p = std::env::temp_dir();
        p.push(format!("kronroe-ios-ffi-{nanos}.kronroe"));
        p.to_string_lossy().to_string()
    }

    #[test]
    fn ffi_open_assert_query_roundtrip_file_backed() {
        let path = c(&unique_db_path());
        let subject = c("Freya");
        let predicate = c("attends");
        let object = c("Sunrise Primary");
        let entity = c("Freya");

        let handle = unsafe { kronroe_graph_open(path.as_ptr()) };
        assert!(!handle.is_null(), "open should return a valid handle");

        let ok = unsafe {
            kronroe_graph_assert_text(
                handle,
                subject.as_ptr(),
                predicate.as_ptr(),
                object.as_ptr(),
            )
        };
        assert!(ok, "assert should succeed");

        let json_ptr = unsafe { kronroe_graph_facts_about_json(handle, entity.as_ptr()) };
        assert!(!json_ptr.is_null(), "facts query should return JSON");

        let json = unsafe { CStr::from_ptr(json_ptr) }
            .to_str()
            .expect("valid utf8");
        let facts: serde_json::Value = serde_json::from_str(json).expect("valid json");
        let arr = facts.as_array().expect("json array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["subject"], "Freya");
        assert_eq!(arr[0]["predicate"], "attends");
        assert_eq!(arr[0]["object"]["value"], "Sunrise Primary");

        unsafe {
            kronroe_string_free(json_ptr);
            kronroe_graph_close(handle);
        }
    }

    #[test]
    fn ffi_open_in_memory_assert_query_roundtrip() {
        let subject = c("alice");
        let predicate = c("works_at");
        let object = c("Acme");
        let entity = c("alice");

        let handle = kronroe_graph_open_in_memory();
        assert!(
            !handle.is_null(),
            "open_in_memory should return a valid handle"
        );

        let ok = unsafe {
            kronroe_graph_assert_text(
                handle,
                subject.as_ptr(),
                predicate.as_ptr(),
                object.as_ptr(),
            )
        };
        assert!(ok, "assert should succeed");

        let json_ptr = unsafe { kronroe_graph_facts_about_json(handle, entity.as_ptr()) };
        assert!(!json_ptr.is_null(), "facts query should return JSON");

        let json = unsafe { CStr::from_ptr(json_ptr) }
            .to_str()
            .expect("valid utf8");
        let facts: serde_json::Value = serde_json::from_str(json).expect("valid json");
        let arr = facts.as_array().expect("json array");
        assert_eq!(arr.len(), 1);

        unsafe {
            kronroe_string_free(json_ptr);
            kronroe_graph_close(handle);
        }
    }

    #[test]
    fn ffi_failure_path_null_handle_assert_sets_error() {
        let subject = c("alice");
        let predicate = c("works_at");
        let object = c("Acme");

        let ok = unsafe {
            kronroe_graph_assert_text(
                std::ptr::null_mut(),
                subject.as_ptr(),
                predicate.as_ptr(),
                object.as_ptr(),
            )
        };
        assert!(!ok, "assert should fail with null handle");

        let msg_ptr = kronroe_last_error_message();
        assert!(!msg_ptr.is_null(), "error message should be set");
        let msg = unsafe { CStr::from_ptr(msg_ptr) }
            .to_str()
            .expect("valid utf8");
        assert!(
            msg.contains("graph handle is null"),
            "expected null-handle error, got: {msg}"
        );
        unsafe { kronroe_string_free(msg_ptr) };
    }
}
