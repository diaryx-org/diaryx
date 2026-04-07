//! Native-only Extism ABI stubs for unit tests.
//!
//! The guest plugin is built as a `cdylib` for Wasm, but `cargo test` on a
//! native host still attempts to link that target. These symbols are normally
//! supplied by the Extism runtime, so we provide inert native shims to keep
//! host-side unit tests linkable.

#[unsafe(no_mangle)]
pub extern "C" fn alloc(_size: u64) -> u64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn length(_offset: u64) -> u64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn load_u8(_offset: u64) -> u64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn load_u64(_offset: u64) -> u64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn store_u8(_offset: u64, _value: u64) {}

#[unsafe(no_mangle)]
pub extern "C" fn store_u64(_offset: u64, _value: u64) {}

#[unsafe(no_mangle)]
pub extern "C" fn input_length() -> u64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn input_load_u8(_offset: u64) -> u64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn input_load_u64(_offset: u64) -> u64 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn output_set(_offset: u64) {}

#[unsafe(no_mangle)]
pub extern "C" fn error_set(_offset: u64) {}

macro_rules! stub_host_fn {
    ($name:ident) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(_input: u64) -> u64 {
            0
        }
    };
}

stub_host_fn!(host_log);
stub_host_fn!(host_read_file);
stub_host_fn!(host_read_binary);
stub_host_fn!(host_list_files);
stub_host_fn!(host_file_exists);
stub_host_fn!(host_write_file);
stub_host_fn!(host_delete_file);
stub_host_fn!(host_write_binary);
stub_host_fn!(host_emit_event);
stub_host_fn!(host_storage_get);
stub_host_fn!(host_storage_set);
stub_host_fn!(host_get_timestamp);
stub_host_fn!(host_get_now);
stub_host_fn!(host_ws_request);
stub_host_fn!(host_http_request);
stub_host_fn!(host_plugin_command);
stub_host_fn!(host_get_runtime_context);
stub_host_fn!(host_secret_get);
stub_host_fn!(host_secret_set);
stub_host_fn!(host_secret_delete);
stub_host_fn!(host_run_wasi_module);
stub_host_fn!(host_request_file);
stub_host_fn!(host_namespace_put_object);
stub_host_fn!(host_namespace_delete_object);
stub_host_fn!(host_namespace_list_objects);
stub_host_fn!(host_namespace_sync_audience);
stub_host_fn!(host_hash_file);
