#![cfg(target_os = "macos")]

use base64::Engine;
use std::ffi::c_void;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

type Boolean = u8;
type CFAllocatorRef = *const c_void;
type CFArrayRef = *const c_void;
type CFDataRef = *const c_void;
type CFErrorRef = *mut c_void;
type CFIndex = isize;
type CFOptionFlags = u64;
type CFTypeRef = *const c_void;
type CFURLRef = *const c_void;

type CFURLBookmarkCreationOptions = CFOptionFlags;
type CFURLBookmarkResolutionOptions = CFOptionFlags;

const K_CFURL_BOOKMARK_CREATION_WITH_SECURITY_SCOPE: CFURLBookmarkCreationOptions = 1 << 11;
const K_CFURL_BOOKMARK_RESOLUTION_WITHOUT_UI_MASK: CFURLBookmarkResolutionOptions = 1 << 8;
const K_CFURL_BOOKMARK_RESOLUTION_WITH_SECURITY_SCOPE: CFURLBookmarkResolutionOptions = 1 << 10;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFDataCreate(allocator: CFAllocatorRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
    fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    fn CFDataGetLength(data: CFDataRef) -> CFIndex;
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: CFAllocatorRef,
        buffer: *const u8,
        buf_len: CFIndex,
        is_directory: Boolean,
    ) -> CFURLRef;
    fn CFURLGetFileSystemRepresentation(
        url: CFURLRef,
        resolve_against_base: Boolean,
        buffer: *mut u8,
        max_buf_len: CFIndex,
    ) -> Boolean;
    fn CFURLCreateBookmarkData(
        allocator: CFAllocatorRef,
        url: CFURLRef,
        options: CFURLBookmarkCreationOptions,
        resource_properties_to_include: CFArrayRef,
        relative_to_url: CFURLRef,
        error: *mut CFErrorRef,
    ) -> CFDataRef;
    fn CFURLCreateByResolvingBookmarkData(
        allocator: CFAllocatorRef,
        bookmark: CFDataRef,
        options: CFURLBookmarkResolutionOptions,
        relative_to_url: CFURLRef,
        resource_properties_to_include: CFArrayRef,
        is_stale: *mut Boolean,
        error: *mut CFErrorRef,
    ) -> CFURLRef;
    fn CFURLStartAccessingSecurityScopedResource(url: CFURLRef) -> Boolean;
    fn CFURLStopAccessingSecurityScopedResource(url: CFURLRef);
}

fn cf_url_from_path(path: &Path, is_directory: bool) -> Result<CFURLRef, String> {
    let bytes = path.as_os_str().as_bytes();
    let url = unsafe {
        CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as CFIndex,
            if is_directory { 1 } else { 0 },
        )
    };
    if url.is_null() {
        Err(format!(
            "Failed to create file URL for '{}'",
            path.display()
        ))
    } else {
        Ok(url)
    }
}

fn cf_data_to_vec(data: CFDataRef) -> Vec<u8> {
    let len = unsafe { CFDataGetLength(data) };
    let ptr = unsafe { CFDataGetBytePtr(data) };
    if ptr.is_null() || len <= 0 {
        return Vec::new();
    }

    unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec()
}

fn cf_url_to_path(url: CFURLRef) -> Result<PathBuf, String> {
    for size in [4096usize, 16384usize, 65536usize] {
        let mut buffer = vec![0u8; size];
        let ok = unsafe {
            CFURLGetFileSystemRepresentation(url, 1, buffer.as_mut_ptr(), buffer.len() as CFIndex)
        };
        if ok != 0 {
            let nul_pos = buffer
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(buffer.len());
            let path_bytes = buffer[..nul_pos].to_vec();
            return Ok(PathBuf::from(std::ffi::OsString::from_vec(path_bytes)));
        }
    }

    Err("Failed to convert resolved bookmark URL into a filesystem path".to_string())
}

fn create_bookmark_from_url(url: CFURLRef) -> Result<String, String> {
    let bookmark = unsafe {
        CFURLCreateBookmarkData(
            std::ptr::null(),
            url,
            K_CFURL_BOOKMARK_CREATION_WITH_SECURITY_SCOPE,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if bookmark.is_null() {
        return Err("Failed to create security-scoped bookmark data".to_string());
    }

    let bytes = cf_data_to_vec(bookmark);
    unsafe { CFRelease(bookmark as CFTypeRef) };
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

pub struct ActiveSecurityScopedAccess {
    resolved_path: PathBuf,
    refreshed_bookmark: Option<String>,
    url: CFURLRef,
}

impl ActiveSecurityScopedAccess {
    pub fn resolved_path(&self) -> &Path {
        &self.resolved_path
    }

    pub fn refreshed_bookmark(&self) -> Option<&str> {
        self.refreshed_bookmark.as_deref()
    }
}

unsafe impl Send for ActiveSecurityScopedAccess {}
unsafe impl Sync for ActiveSecurityScopedAccess {}

impl Drop for ActiveSecurityScopedAccess {
    fn drop(&mut self) {
        unsafe {
            CFURLStopAccessingSecurityScopedResource(self.url);
            CFRelease(self.url as CFTypeRef);
        }
    }
}

pub fn create_security_scoped_bookmark(path: &Path) -> Result<String, String> {
    let url = cf_url_from_path(path, true)?;
    let bookmark = create_bookmark_from_url(url);
    unsafe { CFRelease(url as CFTypeRef) };
    bookmark
}

pub fn activate_security_scoped_bookmark(
    bookmark_base64: &str,
) -> Result<ActiveSecurityScopedAccess, String> {
    let bookmark_bytes = base64::engine::general_purpose::STANDARD
        .decode(bookmark_base64)
        .map_err(|e| format!("Invalid base64 bookmark data: {e}"))?;

    let bookmark_data = unsafe {
        CFDataCreate(
            std::ptr::null(),
            bookmark_bytes.as_ptr(),
            bookmark_bytes.len() as CFIndex,
        )
    };
    if bookmark_data.is_null() {
        return Err("Failed to create bookmark data buffer".to_string());
    }

    let mut is_stale: Boolean = 0;
    let url = unsafe {
        CFURLCreateByResolvingBookmarkData(
            std::ptr::null(),
            bookmark_data,
            K_CFURL_BOOKMARK_RESOLUTION_WITHOUT_UI_MASK
                | K_CFURL_BOOKMARK_RESOLUTION_WITH_SECURITY_SCOPE,
            std::ptr::null(),
            std::ptr::null(),
            &mut is_stale,
            std::ptr::null_mut(),
        )
    };
    unsafe { CFRelease(bookmark_data as CFTypeRef) };
    if url.is_null() {
        return Err("Failed to resolve security-scoped bookmark".to_string());
    }

    let started = unsafe { CFURLStartAccessingSecurityScopedResource(url) };
    if started == 0 {
        unsafe { CFRelease(url as CFTypeRef) };
        return Err("Resolved bookmark but could not start security-scoped access".to_string());
    }

    let resolved_path = match cf_url_to_path(url) {
        Ok(path) => path,
        Err(error) => {
            unsafe {
                CFURLStopAccessingSecurityScopedResource(url);
                CFRelease(url as CFTypeRef);
            }
            return Err(error);
        }
    };

    let refreshed_bookmark = if is_stale != 0 {
        Some(create_bookmark_from_url(url).map_err(|e| {
            unsafe {
                CFURLStopAccessingSecurityScopedResource(url);
                CFRelease(url as CFTypeRef);
            }
            e
        })?)
    } else {
        None
    };

    Ok(ActiveSecurityScopedAccess {
        resolved_path,
        refreshed_bookmark,
        url,
    })
}
