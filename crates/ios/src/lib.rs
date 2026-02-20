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

    let graph = unsafe { &mut *handle };
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
    let graph = unsafe { &mut *handle };

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
pub extern "C" fn kronroe_last_error_message() -> *const c_char {
    LAST_ERROR.with(|cell| match cell.borrow().as_ref() {
        Some(msg) => msg.as_ptr(),
        None => ptr::null(),
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
