use kronroe::KronroeTimestamp;
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
    // Strip null bytes so CString::new never fails — a null byte in an error
    // message would otherwise silently drop the entire error (CString::new
    // returns Err, .ok() yields None, and the caller sees "no error").
    let sanitized = msg.replace('\0', "\\0");
    LAST_ERROR.with(|cell| {
        *cell.borrow_mut() = CString::new(sanitized).ok();
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
        .assert_fact(&subject, &predicate, object, KronroeTimestamp::now_utc())
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
        Ok(facts) => {
            let json_parts: Vec<String> = facts.iter().map(|f| f.to_json_string()).collect();
            let json_str = format!("[{}]", json_parts.join(","));
            match CString::new(json_str) {
                Ok(cs) => cs.into_raw(),
                Err(_) => {
                    set_last_error("failed to encode facts JSON".to_string());
                    ptr::null_mut()
                }
            }
        }
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

// ---------------------------------------------------------------------------
// Extended FFI surface — unlocks Kindly Roe features
// ---------------------------------------------------------------------------

#[no_mangle]
/// Assert a fact with full control: confidence, source, and valid_from.
///
/// - `confidence`: 0.0–1.0 (pass negative to use default 1.0)
/// - `source`: provenance marker, or NULL for none
/// - `valid_from_iso`: RFC 3339 timestamp, or NULL for current time
///
/// Returns the fact ID as a newly allocated C string, or NULL on error.
///
/// # Safety
/// All string pointers must be valid NUL-terminated UTF-8 or NULL where noted.
/// The returned pointer must be freed with `kronroe_string_free`.
pub unsafe extern "C" fn kronroe_graph_assert_fact(
    handle: *mut KronroeGraphHandle,
    subject: *const c_char,
    predicate: *const c_char,
    object: *const c_char,
    confidence: f32,
    source: *const c_char,
    valid_from_iso: *const c_char,
) -> *mut c_char {
    clear_last_error();
    if handle.is_null() {
        set_last_error("graph handle is null".to_string());
        return ptr::null_mut();
    }
    let subject = match cstr_to_string(subject, "subject") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };
    let predicate = match cstr_to_string(predicate, "predicate") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };
    let object = match cstr_to_string(object, "object") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };

    let valid_from = if valid_from_iso.is_null() {
        KronroeTimestamp::now_utc()
    } else {
        match cstr_to_string(valid_from_iso, "valid_from_iso") {
            Ok(iso) => match KronroeTimestamp::parse_rfc3339(&iso) {
                Ok(ts) => ts,
                Err(e) => {
                    set_last_error(format!("invalid valid_from: {e}"));
                    return ptr::null_mut();
                }
            },
            Err(e) => {
                set_last_error(e);
                return ptr::null_mut();
            }
        }
    };

    let source_str = if source.is_null() {
        None
    } else {
        match cstr_to_string(source, "source") {
            Ok(v) => Some(v),
            Err(e) => {
                set_last_error(e);
                return ptr::null_mut();
            }
        }
    };

    let conf = if confidence < 0.0 { 1.0 } else { confidence };

    let graph = unsafe { &*handle };
    let result = if let Some(src) = source_str {
        graph
            .graph
            .assert_fact_with_source(&subject, &predicate, object, valid_from, conf, &src)
    } else {
        graph
            .graph
            .assert_fact_with_confidence(&subject, &predicate, object, valid_from, conf)
    };

    match result {
        Ok(fact_id) => match CString::new(fact_id.as_str()) {
            Ok(cs) => cs.into_raw(),
            Err(_) => {
                set_last_error("fact ID encoding failed".to_string());
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
/// Return currently valid facts for (entity, predicate) as JSON.
///
/// # Safety
/// All string pointers must be valid NUL-terminated UTF-8.
/// The returned pointer must be freed with `kronroe_string_free`.
pub unsafe extern "C" fn kronroe_graph_current_facts_json(
    handle: *mut KronroeGraphHandle,
    entity: *const c_char,
    predicate: *const c_char,
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
    let predicate = match cstr_to_string(predicate, "predicate") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };
    let graph = unsafe { &*handle };

    match graph.graph.current_facts(&entity, &predicate) {
        Ok(facts) => {
            let json_parts: Vec<String> = facts.iter().map(|f| f.to_json_string()).collect();
            let json_str = format!("[{}]", json_parts.join(","));
            match CString::new(json_str) {
                Ok(cs) => cs.into_raw(),
                Err(_) => {
                    set_last_error("JSON encoding failed".to_string());
                    ptr::null_mut()
                }
            }
        }
        Err(err) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
    }
}

#[no_mangle]
/// Full-text search across all current facts. Returns JSON array.
///
/// # Safety
/// `query` must be a valid NUL-terminated UTF-8 string.
/// The returned pointer must be freed with `kronroe_string_free`.
pub unsafe extern "C" fn kronroe_graph_search_json(
    handle: *mut KronroeGraphHandle,
    query: *const c_char,
    limit: u32,
) -> *mut c_char {
    clear_last_error();
    if handle.is_null() {
        set_last_error("graph handle is null".to_string());
        return ptr::null_mut();
    }
    let query = match cstr_to_string(query, "query") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };
    let graph = unsafe { &*handle };

    match graph.graph.search(&query, limit as usize) {
        Ok(facts) => {
            let json_parts: Vec<String> = facts.iter().map(|f| f.to_json_string()).collect();
            let json_str = format!("[{}]", json_parts.join(","));
            match CString::new(json_str) {
                Ok(cs) => cs.into_raw(),
                Err(_) => {
                    set_last_error("JSON encoding failed".to_string());
                    ptr::null_mut()
                }
            }
        }
        Err(err) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
    }
}

#[no_mangle]
/// Correct a fact: invalidate the old value and assert a new one.
///
/// Returns the new fact ID as a C string, or NULL on error.
///
/// # Safety
/// All string pointers must be valid NUL-terminated UTF-8.
/// The returned pointer must be freed with `kronroe_string_free`.
pub unsafe extern "C" fn kronroe_graph_correct_fact(
    handle: *mut KronroeGraphHandle,
    fact_id: *const c_char,
    new_object: *const c_char,
) -> *mut c_char {
    clear_last_error();
    if handle.is_null() {
        set_last_error("graph handle is null".to_string());
        return ptr::null_mut();
    }
    let fact_id = match cstr_to_string(fact_id, "fact_id") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };
    let new_object = match cstr_to_string(new_object, "new_object") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };
    let graph = unsafe { &*handle };

    match graph
        .graph
        .correct_fact(&fact_id, new_object, KronroeTimestamp::now_utc())
    {
        Ok(new_id) => match CString::new(new_id.as_str()) {
            Ok(cs) => cs.into_raw(),
            Err(_) => {
                set_last_error("fact ID encoding failed".to_string());
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
/// Invalidate (retire) a fact by its ID.
///
/// The fact's `valid_to` is set to now — history is preserved.
///
/// # Safety
/// All string pointers must be valid NUL-terminated UTF-8.
pub unsafe extern "C" fn kronroe_graph_invalidate_fact(
    handle: *mut KronroeGraphHandle,
    fact_id: *const c_char,
) -> bool {
    clear_last_error();
    if handle.is_null() {
        set_last_error("graph handle is null".to_string());
        return false;
    }
    let fact_id = match cstr_to_string(fact_id, "fact_id") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return false;
        }
    };
    let graph = unsafe { &*handle };

    match graph
        .graph
        .invalidate_fact(&fact_id, KronroeTimestamp::now_utc())
    {
        Ok(()) => true,
        Err(err) => {
            set_last_error(err.to_string());
            false
        }
    }
}

#[no_mangle]
/// Look up a single fact by its `kf_...` ID. Returns JSON or NULL.
///
/// # Safety
/// All string pointers must be valid NUL-terminated UTF-8.
/// The returned pointer must be freed with `kronroe_string_free`.
pub unsafe extern "C" fn kronroe_graph_fact_by_id_json(
    handle: *mut KronroeGraphHandle,
    fact_id: *const c_char,
) -> *mut c_char {
    clear_last_error();
    if handle.is_null() {
        set_last_error("graph handle is null".to_string());
        return ptr::null_mut();
    }
    let fact_id = match cstr_to_string(fact_id, "fact_id") {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return ptr::null_mut();
        }
    };
    let graph = unsafe { &*handle };

    match graph.graph.fact_by_id(&fact_id) {
        Ok(fact) => {
            let json_str = fact.to_json_string();
            match CString::new(json_str) {
                Ok(cs) => cs.into_raw(),
                Err(_) => {
                    set_last_error("JSON encoding failed".to_string());
                    ptr::null_mut()
                }
            }
        }
        Err(err) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
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

    #[test]
    fn ffi_last_error_sanitizes_null_bytes() {
        clear_last_error();
        set_last_error("broken\0message".to_string());

        let msg_ptr = kronroe_last_error_message();
        assert!(!msg_ptr.is_null(), "error message should be present");
        let msg = unsafe { CStr::from_ptr(msg_ptr) }
            .to_str()
            .expect("valid utf8");
        assert_eq!(msg, "broken\\0message");

        unsafe { kronroe_string_free(msg_ptr) };
    }

    #[test]
    fn ffi_assert_fact_with_confidence_and_source() {
        let handle = kronroe_graph_open_in_memory();
        assert!(!handle.is_null());

        let subject = c("alice");
        let predicate = c("works_at");
        let object = c("Acme");
        let source = c("linkedin");

        let id_ptr = unsafe {
            kronroe_graph_assert_fact(
                handle,
                subject.as_ptr(),
                predicate.as_ptr(),
                object.as_ptr(),
                0.85,
                source.as_ptr(),
                ptr::null(), // current time
            )
        };
        assert!(!id_ptr.is_null(), "assert_fact should return a fact ID");
        let id = unsafe { CStr::from_ptr(id_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        assert!(id.starts_with("kf_"), "fact ID should start with kf_");
        unsafe { kronroe_string_free(id_ptr) };

        // Verify via fact_by_id
        let id_c = c(&id);
        let fact_ptr = unsafe { kronroe_graph_fact_by_id_json(handle, id_c.as_ptr()) };
        assert!(!fact_ptr.is_null());
        let json = unsafe { CStr::from_ptr(fact_ptr) }.to_str().unwrap();
        assert!(json.contains("\"confidence\":0.85"));
        assert!(json.contains("\"source\":\"linkedin\""));
        unsafe { kronroe_string_free(fact_ptr) };

        unsafe { kronroe_graph_close(handle) };
    }

    #[test]
    fn ffi_current_facts_json() {
        let handle = kronroe_graph_open_in_memory();
        assert!(!handle.is_null());

        let subject = c("bob");
        let predicate = c("role");
        let object = c("engineer");
        unsafe {
            kronroe_graph_assert_text(
                handle,
                subject.as_ptr(),
                predicate.as_ptr(),
                object.as_ptr(),
            );
        }

        let entity = c("bob");
        let pred = c("role");
        let json_ptr =
            unsafe { kronroe_graph_current_facts_json(handle, entity.as_ptr(), pred.as_ptr()) };
        assert!(!json_ptr.is_null());
        let json = unsafe { CStr::from_ptr(json_ptr) }.to_str().unwrap();
        assert!(json.contains("engineer"));
        unsafe { kronroe_string_free(json_ptr) };

        unsafe { kronroe_graph_close(handle) };
    }

    #[test]
    fn ffi_search_json() {
        let handle = kronroe_graph_open_in_memory();
        assert!(!handle.is_null());

        let subject = c("carol");
        let predicate = c("works_at");
        let object = c("Globex Corporation");
        unsafe {
            kronroe_graph_assert_text(
                handle,
                subject.as_ptr(),
                predicate.as_ptr(),
                object.as_ptr(),
            );
        }

        let query = c("Globex");
        let json_ptr = unsafe { kronroe_graph_search_json(handle, query.as_ptr(), 10) };
        assert!(!json_ptr.is_null());
        let json = unsafe { CStr::from_ptr(json_ptr) }.to_str().unwrap();
        assert!(json.contains("Globex"), "search should find Globex: {json}");
        unsafe { kronroe_string_free(json_ptr) };

        unsafe { kronroe_graph_close(handle) };
    }

    #[test]
    fn ffi_correct_and_invalidate_fact() {
        let handle = kronroe_graph_open_in_memory();
        assert!(!handle.is_null());

        // Assert initial fact
        let subject = c("dave");
        let predicate = c("lives_in");
        let object = c("London");
        let id_ptr = unsafe {
            kronroe_graph_assert_fact(
                handle,
                subject.as_ptr(),
                predicate.as_ptr(),
                object.as_ptr(),
                -1.0, // default confidence
                ptr::null(),
                ptr::null(),
            )
        };
        assert!(!id_ptr.is_null());
        let old_id = unsafe { CStr::from_ptr(id_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { kronroe_string_free(id_ptr) };

        // Correct it
        let old_id_c = c(&old_id);
        let new_object = c("Paris");
        let new_id_ptr =
            unsafe { kronroe_graph_correct_fact(handle, old_id_c.as_ptr(), new_object.as_ptr()) };
        assert!(!new_id_ptr.is_null(), "correct_fact should return new ID");
        let new_id = unsafe { CStr::from_ptr(new_id_ptr) }
            .to_str()
            .unwrap()
            .to_string();
        assert!(new_id.starts_with("kf_"));
        assert_ne!(old_id, new_id);
        unsafe { kronroe_string_free(new_id_ptr) };

        // Invalidate the new fact
        let new_id_c = c(&new_id);
        let ok = unsafe { kronroe_graph_invalidate_fact(handle, new_id_c.as_ptr()) };
        assert!(ok, "invalidate should succeed");

        // Current facts should be empty
        let entity = c("dave");
        let pred = c("lives_in");
        let json_ptr =
            unsafe { kronroe_graph_current_facts_json(handle, entity.as_ptr(), pred.as_ptr()) };
        assert!(!json_ptr.is_null());
        let json = unsafe { CStr::from_ptr(json_ptr) }.to_str().unwrap();
        assert_eq!(json, "[]", "no current facts after invalidation");
        unsafe { kronroe_string_free(json_ptr) };

        unsafe { kronroe_graph_close(handle) };
    }
}
