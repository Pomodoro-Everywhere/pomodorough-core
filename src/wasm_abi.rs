#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::slice;

#[cfg(target_arch = "wasm32")]
use crate::dispatch_envelope_json;

#[cfg(target_arch = "wasm32")]
/// Maximum UTF-8 operation name accepted by the JSON dispatch ABI.
const MAX_ABI_OPERATION_BYTES: u32 = 256;
/// Maximum JSON input, output, or host allocation shared by ABI consumers.
const MAX_ABI_BUFFER_BYTES: u32 = 16 * 1024 * 1024;

#[cfg(target_arch = "wasm32")]
struct LiveBuffer {
    pointer: u32,
    bytes: Vec<u8>,
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static LIVE_BUFFERS: RefCell<Vec<LiveBuffer>> = const { RefCell::new(Vec::new()) };
}

/// Allocates a byte-aligned host transfer buffer that must be freed once with
/// its exact pointer and length. Returns the stable null sentinel `0` for an
/// empty, over-16 MiB, conversion-impossible, or failed allocation.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn pomodorough_alloc(length: u32) -> u32 {
    allocate_buffer(length)
        .and_then(register_buffer)
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pomodorough_free(pointer: u32, length: u32) {
    let _ = release_buffer(pointer, length);
}

#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pomodorough_free_v2(pointer: u32, length: u32) -> u32 {
    u32::from(release_buffer(pointer, length))
}

#[cfg(target_arch = "wasm32")]
fn release_buffer(pointer: u32, length: u32) -> bool {
    if !valid_memory_range(pointer, length) {
        return false;
    }
    LIVE_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        let Some(index) = buffers
            .iter()
            .position(|buffer| buffer.pointer == pointer && buffer.bytes.len() == length as usize)
        else {
            return false;
        };
        buffers.swap_remove(index);
        true
    })
}

#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pomodorough_dispatch(
    operation_pointer: u32,
    operation_length: u32,
    input_pointer: u32,
    input_length: u32,
) -> u64 {
    let operation = checked_bytes(
        operation_pointer,
        operation_length,
        MAX_ABI_OPERATION_BYTES,
        "operation",
    );
    let input = checked_bytes(input_pointer, input_length, MAX_ABI_BUFFER_BYTES, "input");
    let result = match (operation, input) {
        (Ok(operation), Ok(input)) => dispatch_bytes(&operation, &input),
        (Err(error), _) | (_, Err(error)) => error_envelope(error),
    };
    pack_result(result.into_bytes())
}

#[cfg(target_arch = "wasm32")]
fn checked_bytes(pointer: u32, length: u32, maximum: u32, label: &str) -> Result<Vec<u8>, String> {
    if length > maximum || !valid_memory_range(pointer, length) || !live_buffer(pointer, length) {
        return Err(format!("binding {label} range is invalid"));
    }
    // SAFETY: range is inside memory and matches a live Vec allocation exactly.
    Ok(unsafe { slice::from_raw_parts(pointer as *const u8, length as usize) }.to_vec())
}

#[cfg(target_arch = "wasm32")]
fn dispatch_bytes(operation: &[u8], input: &[u8]) -> String {
    match (std::str::from_utf8(operation), std::str::from_utf8(input)) {
        (Ok(operation), Ok(input)) => dispatch_envelope_json(operation, input),
        _ => error_envelope("binding input is not UTF-8"),
    }
}

#[cfg(target_arch = "wasm32")]
fn error_envelope(error: impl Into<String>) -> String {
    serde_json::json!({"ok": false, "error": error.into()}).to_string()
}

fn allocation_length(length: u32) -> Option<usize> {
    if length == 0 || length > MAX_ABI_BUFFER_BYTES {
        return None;
    }
    usize::try_from(length).ok()
}

fn allocate_buffer(length: u32) -> Option<Vec<u8>> {
    let length = allocation_length(length)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).ok()?;
    bytes.resize(length, 0);
    Some(bytes)
}

#[cfg(target_arch = "wasm32")]
fn valid_memory_range(pointer: u32, length: u32) -> bool {
    if pointer == 0 || length == 0 {
        return false;
    }
    let Some(end) = pointer.checked_add(length) else {
        return false;
    };
    end as usize <= core::arch::wasm32::memory_size::<0>() * 65_536
}

#[cfg(target_arch = "wasm32")]
fn live_buffer(pointer: u32, length: u32) -> bool {
    LIVE_BUFFERS.with(|buffers| {
        buffers
            .borrow()
            .iter()
            .any(|buffer| buffer.pointer == pointer && buffer.bytes.len() == length as usize)
    })
}

#[cfg(target_arch = "wasm32")]
fn register_buffer(mut bytes: Vec<u8>) -> Option<u32> {
    let length = u32::try_from(bytes.len()).ok()?;
    if length == 0 || length > MAX_ABI_BUFFER_BYTES {
        return None;
    }
    let pointer = u32::try_from(bytes.as_mut_ptr() as usize).ok()?;
    if pointer == 0 || pointer.checked_add(length).is_none() {
        return None;
    }
    LIVE_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        if buffers.iter().any(|buffer| buffer.pointer == pointer) || buffers.try_reserve(1).is_err()
        {
            return None;
        }
        buffers.push(LiveBuffer { pointer, bytes });
        Some(pointer)
    })
}

#[cfg(target_arch = "wasm32")]
fn pack_result(bytes: Vec<u8>) -> u64 {
    let Ok(length) = u32::try_from(bytes.len()) else {
        return 0;
    };
    let Some(pointer) = register_buffer(bytes) else {
        return 0;
    };
    (u64::from(length) << 32) | u64::from(pointer)
}

#[cfg(test)]
mod tests {
    use super::{MAX_ABI_BUFFER_BYTES, allocate_buffer, allocation_length};

    #[test]
    fn c4_allocation_length_enforces_protocol_boundaries() {
        assert_eq!(allocation_length(0), None);
        assert_eq!(allocation_length(1), Some(1));
        assert_eq!(
            allocation_length(MAX_ABI_BUFFER_BYTES - 1),
            Some((MAX_ABI_BUFFER_BYTES - 1) as usize)
        );
        assert_eq!(
            allocation_length(MAX_ABI_BUFFER_BYTES),
            Some(MAX_ABI_BUFFER_BYTES as usize)
        );
        assert_eq!(allocation_length(MAX_ABI_BUFFER_BYTES + 1), None);
        assert_eq!(allocation_length(u32::MAX), None);
    }

    #[test]
    fn c4_rejected_lengths_do_not_prevent_later_allocation() {
        for length in [0, MAX_ABI_BUFFER_BYTES + 1, u32::MAX] {
            assert!(allocate_buffer(length).is_none());
        }
        let bytes = allocate_buffer(32).expect("valid allocation must succeed");
        assert_eq!(bytes, vec![0; 32]);
    }
}
