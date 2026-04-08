//! Extism guest plugin: HEIC/HEIF → JPEG converter.
//!
//! Exports:
//! - `manifest()` — returns JSON plugin manifest
//! - `transcode_image(bytes)` — binary export using the wire format described below
//!
//! ## Wire Format
//!
//! **Input** (8-byte header + payload):
//! ```text
//! [u8  output_format]   0=JPEG, 1=PNG, 2=WebP
//! [u8  quality]         0–100
//! [u8  reserved]
//! [u8  reserved]
//! [u32 payload_len]     little-endian
//! [... raw HEIC bytes]
//! ```
//!
//! **Output** (8-byte header + payload):
//! ```text
//! [u8  status]          0=ok, 1=unsupported_format, 2=decode_error, 3=encode_error
//! [u8  output_format]   echo back
//! [u16 reserved]
//! [u32 payload_len]     little-endian
//! [... raw JPEG bytes (or UTF-8 error on failure)]
//! ```

use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;

// Status codes
const STATUS_OK: u8 = 0;
const STATUS_UNSUPPORTED_FORMAT: u8 = 1;
const STATUS_DECODE_ERROR: u8 = 2;
const STATUS_ENCODE_ERROR: u8 = 3;

// Output format codes
const FORMAT_JPEG: u8 = 0;

// FFI bindings for libheif (linked via build.rs)
unsafe extern "C" {
    fn heif_context_alloc() -> *mut core::ffi::c_void;
    fn heif_context_free(ctx: *mut core::ffi::c_void);
    fn heif_context_read_from_memory_without_copy(
        ctx: *mut core::ffi::c_void,
        mem: *const u8,
        size: usize,
        options: *const core::ffi::c_void,
    ) -> HeifError;
    fn heif_context_get_primary_image_handle(
        ctx: *mut core::ffi::c_void,
        handle: *mut *mut core::ffi::c_void,
    ) -> HeifError;
    fn heif_image_handle_get_width(handle: *mut core::ffi::c_void) -> i32;
    fn heif_image_handle_get_height(handle: *mut core::ffi::c_void) -> i32;
    fn heif_image_handle_release(handle: *mut core::ffi::c_void);
    fn heif_decode_image(
        handle: *mut core::ffi::c_void,
        out_img: *mut *mut core::ffi::c_void,
        colorspace: i32,
        chroma: i32,
        options: *const core::ffi::c_void,
    ) -> HeifError;
    fn heif_image_get_plane_readonly(
        img: *mut core::ffi::c_void,
        channel: i32,
        out_stride: *mut i32,
    ) -> *const u8;
    fn heif_image_release(img: *mut core::ffi::c_void);
}

/// Minimal error struct matching libheif's heif_error.
#[repr(C)]
#[derive(Clone, Copy)]
struct HeifError {
    code: i32,
    _subcode: i32,
    _message: *const core::ffi::c_char,
}

impl HeifError {
    fn is_ok(self) -> bool {
        self.code == 0
    }
}

// libheif constants
const HEIF_COLORSPACE_RGB: i32 = 1;
const HEIF_CHROMA_INTERLEAVED_RGB: i32 = 10;
const HEIF_CHANNEL_INTERLEAVED: i32 = 10;

fn build_response(status: u8, output_format: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.push(status);
    out.push(output_format);
    out.push(0); // reserved
    out.push(0); // reserved
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn build_error_response(status: u8, output_format: u8, msg: &str) -> Vec<u8> {
    build_response(status, output_format, msg.as_bytes())
}

/// Decode a HEIC image and return raw RGB pixels + dimensions.
///
/// # Safety
/// Calls libheif C FFI. All pointers are managed with proper alloc/free pairing.
unsafe fn decode_heic(data: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    unsafe {
        let ctx = heif_context_alloc();
        if ctx.is_null() {
            return Err("Failed to allocate heif context".into());
        }

        let err = heif_context_read_from_memory_without_copy(
            ctx,
            data.as_ptr(),
            data.len(),
            core::ptr::null(),
        );
        if !err.is_ok() {
            heif_context_free(ctx);
            return Err("Failed to read HEIC data".into());
        }

        let mut handle: *mut core::ffi::c_void = core::ptr::null_mut();
        let err = heif_context_get_primary_image_handle(ctx, &mut handle);
        if !err.is_ok() {
            heif_context_free(ctx);
            return Err("Failed to get primary image handle".into());
        }

        let width = heif_image_handle_get_width(handle) as u32;
        let height = heif_image_handle_get_height(handle) as u32;

        let mut img: *mut core::ffi::c_void = core::ptr::null_mut();
        let err = heif_decode_image(
            handle,
            &mut img,
            HEIF_COLORSPACE_RGB,
            HEIF_CHROMA_INTERLEAVED_RGB,
            core::ptr::null(),
        );
        if !err.is_ok() {
            heif_image_handle_release(handle);
            heif_context_free(ctx);
            return Err("Failed to decode image".into());
        }

        let mut stride: i32 = 0;
        let plane = heif_image_get_plane_readonly(img, HEIF_CHANNEL_INTERLEAVED, &mut stride);
        if plane.is_null() {
            heif_image_release(img);
            heif_image_handle_release(handle);
            heif_context_free(ctx);
            return Err("Failed to get pixel data".into());
        }

        // Copy pixel data — each row is `stride` bytes, but we only want `width * 3`
        let row_bytes = (width * 3) as usize;
        let mut pixels = Vec::with_capacity(row_bytes * height as usize);
        for y in 0..height as isize {
            let row_start = plane.offset(y * stride as isize);
            let row = core::slice::from_raw_parts(row_start, row_bytes);
            pixels.extend_from_slice(row);
        }

        heif_image_release(img);
        heif_image_handle_release(handle);
        heif_context_free(ctx);

        Ok((pixels, width, height))
    }
}

/// Encode RGB pixels to JPEG bytes.
fn encode_jpeg(pixels: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let encoder = jpeg_encoder::Encoder::new(&mut buf, quality);
    encoder
        .encode(
            pixels,
            width as u16,
            height as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|e| format!("JPEG encode failed: {e}"))?;
    Ok(buf)
}

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.heic-converter",
        "HEIC Image Converter",
        env!("CARGO_PKG_VERSION"),
        "Converts HEIC/HEIF images to JPEG for browser display",
        vec!["media_transcoder".into()],
    )
    .conversions(vec!["heic:jpeg".into(), "heif:jpeg".into()])
    .min_app_version("1.4.0");
    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn transcode_image(input: Vec<u8>) -> FnResult<Vec<u8>> {
    if input.len() < 8 {
        return Ok(build_error_response(
            STATUS_DECODE_ERROR,
            FORMAT_JPEG,
            "Input too short",
        ));
    }

    let output_format = input[0];
    let quality = input[1];
    let payload_len = u32::from_le_bytes([input[4], input[5], input[6], input[7]]) as usize;

    if output_format != FORMAT_JPEG {
        return Ok(build_error_response(
            STATUS_UNSUPPORTED_FORMAT,
            output_format,
            "Only JPEG output is currently supported",
        ));
    }

    if 8 + payload_len > input.len() {
        return Ok(build_error_response(
            STATUS_DECODE_ERROR,
            output_format,
            "Payload length exceeds input size",
        ));
    }

    let heic_data = &input[8..8 + payload_len];

    // Decode HEIC to raw RGB
    let (pixels, width, height) = match unsafe { decode_heic(heic_data) } {
        Ok(result) => result,
        Err(msg) => {
            return Ok(build_error_response(
                STATUS_DECODE_ERROR,
                output_format,
                &msg,
            ));
        }
    };

    // Encode to JPEG
    let jpeg_quality = if quality == 0 { 92 } else { quality };
    match encode_jpeg(&pixels, width, height, jpeg_quality) {
        Ok(jpeg_bytes) => Ok(build_response(STATUS_OK, output_format, &jpeg_bytes)),
        Err(msg) => Ok(build_error_response(
            STATUS_ENCODE_ERROR,
            output_format,
            &msg,
        )),
    }
}
