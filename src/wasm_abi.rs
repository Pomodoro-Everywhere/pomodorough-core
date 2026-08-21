use std::slice;

use crate::dispatch_envelope_json;

#[unsafe(no_mangle)]
pub extern "C" fn pomodorough_alloc(length: u32) -> u32 {
    if length == 0 {
        return 0;
    }
    let bytes = vec![0_u8; length as usize].into_boxed_slice();
    Box::into_raw(bytes) as *mut u8 as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pomodorough_free(pointer: u32, length: u32) {
    if pointer == 0 || length == 0 {
        return;
    }
    let slice = std::ptr::slice_from_raw_parts_mut(pointer as *mut u8, length as usize);
    // SAFETY: callers may only free buffers returned by pomodorough_alloc or
    // pomodorough_dispatch, once, with the original length.
    unsafe { drop(Box::from_raw(slice)) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn pomodorough_dispatch(
    operation_pointer: u32,
    operation_length: u32,
    input_pointer: u32,
    input_length: u32,
) -> u64 {
    // SAFETY: the binding contract requires both ranges to refer to live buffers
    // in this module's linear memory for the duration of this call.
    let operation_bytes =
        unsafe { slice::from_raw_parts(operation_pointer as *const u8, operation_length as usize) };
    // SAFETY: same contract as operation_bytes.
    let input_bytes =
        unsafe { slice::from_raw_parts(input_pointer as *const u8, input_length as usize) };

    let result = match (
        std::str::from_utf8(operation_bytes),
        std::str::from_utf8(input_bytes),
    ) {
        (Ok(operation), Ok(input)) => dispatch_envelope_json(operation, input),
        _ => serde_json::json!({"ok": false, "error": "binding input is not UTF-8"}).to_string(),
    };
    pack_result(result.into_bytes().into_boxed_slice())
}

fn pack_result(bytes: Box<[u8]>) -> u64 {
    let length = bytes.len() as u32;
    let pointer = Box::into_raw(bytes) as *mut u8 as u32;
    (u64::from(length) << 32) | u64::from(pointer)
}
