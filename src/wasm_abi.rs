use std::cell::RefCell;
use std::collections::BTreeSet;
use std::slice;

use crate::dispatch_envelope_json;

thread_local! {
    static LIVE_BUFFERS: RefCell<BTreeSet<(u32, u32)>> = RefCell::new(BTreeSet::new());
}

#[unsafe(no_mangle)]
pub extern "C" fn pomodorough_alloc(length: u32) -> u32 {
    if length == 0 {
        return 0;
    }
    let bytes = vec![0_u8; length as usize].into_boxed_slice();
    register_buffer(bytes)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pomodorough_free(pointer: u32, length: u32) {
    if !remove_buffer(pointer, length) {
        return;
    }
    let slice = std::ptr::slice_from_raw_parts_mut(pointer as *mut u8, length as usize);
    // SAFETY: remove_buffer proves this exact live Box allocation was registered once.
    unsafe { drop(Box::from_raw(slice)) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pomodorough_dispatch(
    operation_pointer: u32,
    operation_length: u32,
    input_pointer: u32,
    input_length: u32,
) -> u64 {
    let operation = checked_bytes(operation_pointer, operation_length, "operation");
    let input = checked_bytes(input_pointer, input_length, "input");
    let result = match (operation, input) {
        (Ok(operation), Ok(input)) => dispatch_bytes(&operation, &input),
        (Err(error), _) | (_, Err(error)) => error_envelope(error),
    };
    pack_result(result.into_bytes().into_boxed_slice())
}

fn checked_bytes(pointer: u32, length: u32, label: &str) -> Result<Vec<u8>, String> {
    if !valid_memory_range(pointer, length) || !live_buffer(pointer, length) {
        return Err(format!("binding {label} range is invalid"));
    }
    // SAFETY: range is inside memory and matches a live Box allocation exactly.
    Ok(unsafe { slice::from_raw_parts(pointer as *const u8, length as usize) }.to_vec())
}

fn dispatch_bytes(operation: &[u8], input: &[u8]) -> String {
    match (std::str::from_utf8(operation), std::str::from_utf8(input)) {
        (Ok(operation), Ok(input)) => dispatch_envelope_json(operation, input),
        _ => error_envelope("binding input is not UTF-8"),
    }
}

fn error_envelope(error: impl Into<String>) -> String {
    serde_json::json!({"ok": false, "error": error.into()}).to_string()
}

fn valid_memory_range(pointer: u32, length: u32) -> bool {
    if pointer == 0 || length == 0 {
        return false;
    }
    let Some(end) = pointer.checked_add(length) else {
        return false;
    };
    end as usize <= core::arch::wasm32::memory_size::<0>() * 65_536
}

fn live_buffer(pointer: u32, length: u32) -> bool {
    LIVE_BUFFERS.with(|buffers| buffers.borrow().contains(&(pointer, length)))
}

fn remove_buffer(pointer: u32, length: u32) -> bool {
    if !valid_memory_range(pointer, length) {
        return false;
    }
    LIVE_BUFFERS.with(|buffers| buffers.borrow_mut().remove(&(pointer, length)))
}

fn register_buffer(bytes: Box<[u8]>) -> u32 {
    let length = bytes.len() as u32;
    let pointer = Box::into_raw(bytes) as *mut u8 as u32;
    LIVE_BUFFERS.with(|buffers| {
        assert!(buffers.borrow_mut().insert((pointer, length)));
    });
    pointer
}

fn pack_result(bytes: Box<[u8]>) -> u64 {
    let length = bytes.len() as u32;
    let pointer = register_buffer(bytes);
    (u64::from(length) << 32) | u64::from(pointer)
}
