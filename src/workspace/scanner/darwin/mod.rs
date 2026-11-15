// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Darwin bulk directory scanning engine leveraging `getattrlistbulk(2)`.
//!
//! Provides zero-heap-allocation per-entry directory enumeration using a
//! thread-local 64KB stack buffer, disables cloud dataless file materialization
//! via `setiopolicy_np`, and provides cross-platform standard library fallbacks.

#![allow(unsafe_code, reason = "Direct Darwin getattrlistbulk and setiopolicy_np C FFI invocations")]

use crate::workspace::scanner::classify::classify_tile_category;
use crate::workspace::types::{
    TileCategory, TREEMAP_FLAG_CLOUD_STUB, TREEMAP_FLAG_FIRMLINK, TREEMAP_FLAG_IS_DIR,
    TREEMAP_FLAG_IS_LEAF, TREEMAP_FLAG_RECLAIMABLE,
};

/// 64KB thread-local bulk read buffer size conforming to Darwin XNU cache lines.
pub const SCAN_BUFFER_SIZE: usize = 64 * 1024;

/// Result entry produced by directory scanning.
#[derive(Clone, Debug, PartialEq)]
pub struct ScannedEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub inode: u64,
    pub dev: u64,
    pub nlink: u32,
    pub flags: u8,
    pub category: TileCategory,
}

#[cfg(target_os = "macos")]
pub mod darwin_sys {
    use std::os::raw::{c_char, c_int, c_void};

    pub const ATTR_BIT_MAP_COUNT: u16 = 5;
    pub const ATTR_CMN_NAME: u32 = 0x0000_0001;
    pub const ATTR_CMN_DEVID: u32 = 0x0000_0002;
    pub const ATTR_CMN_OBJTYPE: u32 = 0x0000_0008;
    pub const ATTR_CMN_OBJID: u32 = 0x0000_0020;
    pub const ATTR_CMN_MODTIME: u32 = 0x0000_0400;
    pub const ATTR_CMN_FLAGS: u32 = 0x0004_0000;
    pub const ATTR_CMN_FILEID: u32 = 0x0200_0000;
    pub const ATTR_CMN_ERROR: u32 = 0x2000_0000;
    pub const ATTR_CMN_RETURNED_ATTRS: u32 = 0x8000_0000;

    pub const ATTR_FILE_LINKCOUNT: u32 = 0x0000_0001;
    pub const ATTR_FILE_ALLOCSIZE: u32 = 0x0000_0004;
    pub const ATTR_FILE_DATALENGTH: u32 = 0x0000_0200;

    pub const VREG: u32 = 1;
    pub const VDIR: u32 = 2;
    pub const SF_DATALESS: u32 = 0x4000_0000;
    pub const SF_FIRMLINK: u32 = 0x0080_0000;
    pub const FSOPT_RETURN_REALDEV: u64 = 0x0000_0200;

    pub const O_RDONLY: c_int = 0x0000;
    pub const O_DIRECTORY: c_int = 0x0010_0000;
    pub const O_CLOEXEC: c_int = 0x0100_0000;
    pub const O_NOFOLLOW: c_int = 0x0000_0100;

    pub const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;
    pub const IOPOL_SCOPE_PROCESS: c_int = 0;
    pub const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: c_int = 1;

    #[repr(C)]
    pub struct AttrList {
        pub bitmapcount: u16,
        pub reserved: u16,
        pub commonattr: u32,
        pub volattr: u32,
        pub dirattr: u32,
        pub fileattr: u32,
        pub forkattr: u32,
    }

    #[repr(C)]
    pub struct AttributeSet {
        pub commonattr: u32,
        pub volattr: u32,
        pub dirattr: u32,
        pub fileattr: u32,
        pub forkattr: u32,
    }

    #[repr(C)]
    pub struct AttrReference {
        pub attr_dataoffset: i32,
        pub attr_length: u32,
    }

    #[repr(C)]
    pub struct FsObjId {
        pub fid_objno: u32,
        pub fid_generation: u32,
    }

    #[repr(C)]
    pub struct TimeSpec {
        pub tv_sec: isize,
        pub tv_nsec: isize,
    }

    extern "C" {
        pub fn open(path: *const c_char, flags: c_int, ...) -> c_int;
        pub fn close(fd: c_int) -> c_int;
        pub fn getattrlistbulk(
            dirfd: c_int,
            alist: *mut c_void,
            attribute_buffer: *mut c_void,
            buffer_size: usize,
            options: u64,
        ) -> c_int;
        pub fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
    }
}

#[cfg(target_os = "macos")]
#[repr(align(8))]
struct AlignedScanBuffer([u8; SCAN_BUFFER_SIZE]);

#[cfg(target_os = "macos")]
thread_local! {
    static SCAN_BUFFER: std::cell::RefCell<Box<AlignedScanBuffer>> =
        std::cell::RefCell::new(Box::new(AlignedScanBuffer([0u8; SCAN_BUFFER_SIZE])));
}

#[cfg(target_os = "macos")]
struct DirFdGuard(std::os::raw::c_int);

#[cfg(target_os = "macos")]
impl DirFdGuard {
    #[inline]
    fn new(fd: std::os::raw::c_int) -> Self {
        Self(fd)
    }

    #[inline]
    fn raw(&self) -> std::os::raw::c_int {
        self.0
    }
}

#[cfg(target_os = "macos")]
impl Drop for DirFdGuard {
    fn drop(&mut self) {
        if self.0 >= 0 {
            unsafe {
                darwin_sys::close(self.0);
            }
        }
    }
}

/// Disables automatic materialization of cloud dataless files (iCloud / OneDrive).
///
/// Ensures reading file attributes on network-backed stubs does not trigger
/// background downloading that blocks threads and wastes network bandwidth.
pub fn disable_dataless_materialization() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe {
            darwin_sys::setiopolicy_np(
                darwin_sys::IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
                darwin_sys::IOPOL_SCOPE_PROCESS,
                darwin_sys::IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
            ) == 0
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Reads the immediate directory entries of `dir_path`.
///
/// On macOS, uses `getattrlistbulk(2)` with a thread-local 64KB buffer.
/// Falls back to standard `std::fs::read_dir` if the system call fails or on other platforms.
pub fn scan_directory_entries(dir_path: &str) -> std::io::Result<Vec<ScannedEntry>> {
    #[cfg(target_os = "macos")]
    {
        match scan_darwin_bulk(dir_path) {
            Ok(entries) => Ok(entries),
            Err(_) => scan_fallback(dir_path),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        scan_fallback(dir_path)
    }
}

#[cfg(target_os = "macos")]
fn scan_darwin_bulk(dir_path: &str) -> std::io::Result<Vec<ScannedEntry>> {
    use std::ffi::CString;

    let c_path = CString::new(dir_path).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let fd = unsafe {
        darwin_sys::open(
            c_path.as_ptr(),
            darwin_sys::O_RDONLY | darwin_sys::O_DIRECTORY | darwin_sys::O_CLOEXEC | darwin_sys::O_NOFOLLOW,
        )
    };

    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let _guard = DirFdGuard::new(fd);
    let mut entries = Vec::new();

    SCAN_BUFFER.with(|buf_cell| -> std::io::Result<()> {
        let mut buf_ref = buf_cell.borrow_mut();
        let buf_ptr = buf_ref.0.as_mut_ptr();
        let buf_size = SCAN_BUFFER_SIZE;

        loop {
            let mut attr_list: darwin_sys::AttrList = unsafe { std::mem::zeroed() };
            attr_list.bitmapcount = darwin_sys::ATTR_BIT_MAP_COUNT;
            attr_list.commonattr = darwin_sys::ATTR_CMN_RETURNED_ATTRS
                | darwin_sys::ATTR_CMN_NAME
                | darwin_sys::ATTR_CMN_DEVID
                | darwin_sys::ATTR_CMN_OBJTYPE
                | darwin_sys::ATTR_CMN_OBJID
                | darwin_sys::ATTR_CMN_MODTIME
                | darwin_sys::ATTR_CMN_FLAGS
                | darwin_sys::ATTR_CMN_FILEID
                | darwin_sys::ATTR_CMN_ERROR;
            attr_list.fileattr = darwin_sys::ATTR_FILE_LINKCOUNT
                | darwin_sys::ATTR_FILE_ALLOCSIZE
                | darwin_sys::ATTR_FILE_DATALENGTH;

            let retcount = unsafe {
                darwin_sys::getattrlistbulk(
                    _guard.raw(),
                    &mut attr_list as *mut darwin_sys::AttrList as *mut std::os::raw::c_void,
                    buf_ptr as *mut std::os::raw::c_void,
                    buf_size,
                    darwin_sys::FSOPT_RETURN_REALDEV,
                )
            };

            if retcount < 0 {
                return Err(std::io::Error::last_os_error());
            }

            if retcount == 0 {
                break;
            }

            process_bulk_buffer(buf_ptr, buf_size, retcount, dir_path, &mut entries);
        }

        Ok(())
    })?;

    Ok(entries)
}

#[cfg(target_os = "macos")]
fn process_bulk_buffer(
    buf_ptr: *const u8,
    buf_size: usize,
    retcount: i32,
    dir_path: &str,
    entries: &mut Vec<ScannedEntry>,
) {
    let mut entry_ptr = buf_ptr;
    let end_ptr = unsafe { buf_ptr.add(buf_size) };

    for _ in 0..retcount {
        if entry_ptr >= end_ptr {
            break;
        }

        let entry_len = unsafe { std::ptr::read_unaligned(entry_ptr as *const u32) };
        if entry_len == 0 {
            break;
        }

        let next_entry_ptr = unsafe { entry_ptr.add(entry_len as usize) };
        if next_entry_ptr > end_ptr {
            break;
        }

        if let Some(entry) = unsafe { parse_raw_entry(entry_ptr, dir_path) } {
            entries.push(entry);
        }

        entry_ptr = next_entry_ptr;
    }
}

#[cfg(target_os = "macos")]
unsafe fn parse_raw_entry(entry_ptr: *const u8, dir_path: &str) -> Option<ScannedEntry> {
    use std::ffi::CStr;

    let (ret_attrs, mut field_ptr) = unsafe {
        let field_ptr = entry_ptr.add(std::mem::size_of::<u32>());
        let ret_attrs = std::ptr::read_unaligned(field_ptr as *const darwin_sys::AttributeSet);
        (ret_attrs, field_ptr.add(std::mem::size_of::<darwin_sys::AttributeSet>()))
    };

    if ret_attrs.commonattr & darwin_sys::ATTR_CMN_ERROR != 0 {
        let error_code = unsafe { std::ptr::read_unaligned(field_ptr as *const u32) };
        if error_code != 0 {
            return None;
        }
        field_ptr = unsafe { field_ptr.add(std::mem::size_of::<u32>()) };
    }

    let mut name = String::new();
    if ret_attrs.commonattr & darwin_sys::ATTR_CMN_NAME != 0 {
        let (name_str, next_ptr) = unsafe {
            let ref_val = std::ptr::read_unaligned(field_ptr as *const darwin_sys::AttrReference);
            let name_ptr = field_ptr.offset(ref_val.attr_dataoffset as isize);
            let c_str = CStr::from_ptr(name_ptr as *const std::os::raw::c_char);
            (c_str.to_string_lossy().into_owned(), field_ptr.add(std::mem::size_of::<darwin_sys::AttrReference>()))
        };
        name = name_str;
        field_ptr = next_ptr;
    }

    if name.is_empty() || name == "." || name == ".." {
        return None;
    }

    let meta = unsafe { extract_darwin_metadata(field_ptr, &ret_attrs) };
    let is_dir = meta.obj_type == darwin_sys::VDIR;
    let size = if is_dir { 0 } else { meta.size };
    let mut flags = if is_dir { TREEMAP_FLAG_IS_DIR } else { TREEMAP_FLAG_IS_LEAF };
    if meta.bsd_flags & darwin_sys::SF_DATALESS != 0 {
        flags |= TREEMAP_FLAG_CLOUD_STUB;
    }
    if meta.bsd_flags & darwin_sys::SF_FIRMLINK != 0 {
        flags |= TREEMAP_FLAG_FIRMLINK;
    }
    let category = classify_tile_category(&name, is_dir);
    if category == TileCategory::ReclaimCache {
        flags |= TREEMAP_FLAG_RECLAIMABLE;
    }
    let path = if dir_path == "/" {
        format!("/{}", name)
    } else {
        format!("{}/{}", dir_path.trim_end_matches('/'), name)
    };

    Some(ScannedEntry {
        name,
        path,
        is_dir,
        size,
        inode: meta.inode,
        dev: meta.dev,
        nlink: meta.nlink.max(1),
        flags,
        category,
    })
}

#[derive(Default)]
struct RawDarwinMeta {
    obj_type: u32,
    inode: u64,
    dev: u64,
    nlink: u32,
    bsd_flags: u32,
    size: u64,
}

#[cfg(target_os = "macos")]
unsafe fn extract_darwin_metadata(
    mut field_ptr: *const u8,
    ret_attrs: &darwin_sys::AttributeSet,
) -> RawDarwinMeta {
    let mut meta = RawDarwinMeta::default();

    if ret_attrs.commonattr & darwin_sys::ATTR_CMN_DEVID != 0 {
        let dev_id = unsafe { std::ptr::read_unaligned(field_ptr as *const u32) };
        meta.dev = dev_id as u64;
        field_ptr = unsafe { field_ptr.add(std::mem::size_of::<u32>()) };
    }

    if ret_attrs.commonattr & darwin_sys::ATTR_CMN_OBJTYPE != 0 {
        meta.obj_type = unsafe { std::ptr::read_unaligned(field_ptr as *const u32) };
        field_ptr = unsafe { field_ptr.add(std::mem::size_of::<u32>()) };
    }

    if ret_attrs.commonattr & darwin_sys::ATTR_CMN_OBJID != 0 {
        let obj_id = unsafe { std::ptr::read_unaligned(field_ptr as *const darwin_sys::FsObjId) };
        meta.inode = obj_id.fid_objno as u64;
        field_ptr = unsafe { field_ptr.add(std::mem::size_of::<darwin_sys::FsObjId>()) };
    }

    if ret_attrs.commonattr & darwin_sys::ATTR_CMN_MODTIME != 0 {
        field_ptr = unsafe { field_ptr.add(std::mem::size_of::<darwin_sys::TimeSpec>()) };
    }

    if ret_attrs.commonattr & darwin_sys::ATTR_CMN_FLAGS != 0 {
        meta.bsd_flags = unsafe { std::ptr::read_unaligned(field_ptr as *const u32) };
        field_ptr = unsafe { field_ptr.add(std::mem::size_of::<u32>()) };
    }

    if ret_attrs.commonattr & darwin_sys::ATTR_CMN_FILEID != 0 {
        meta.inode = unsafe { std::ptr::read_unaligned(field_ptr as *const u64) };
        field_ptr = unsafe { field_ptr.add(std::mem::size_of::<u64>()) };
    }

    if ret_attrs.fileattr & darwin_sys::ATTR_FILE_LINKCOUNT != 0 {
        meta.nlink = unsafe { std::ptr::read_unaligned(field_ptr as *const u32) };
        field_ptr = unsafe { field_ptr.add(std::mem::size_of::<u32>()) };
    }

    let mut has_alloc_size = false;
    if ret_attrs.fileattr & darwin_sys::ATTR_FILE_ALLOCSIZE != 0 {
        meta.size = unsafe { std::ptr::read_unaligned(field_ptr as *const u64) };
        has_alloc_size = true;
        field_ptr = unsafe { field_ptr.add(std::mem::size_of::<u64>()) };
    }

    if ret_attrs.fileattr & darwin_sys::ATTR_FILE_DATALENGTH != 0 {
        let data_len = unsafe { std::ptr::read_unaligned(field_ptr as *const u64) };
        if !has_alloc_size {
            meta.size = data_len;
        }
    }

    meta
}

fn scan_fallback(dir_path: &str) -> std::io::Result<Vec<ScannedEntry>> {
    use std::fs;

    let read_dir = fs::read_dir(dir_path)?;
    let mut entries = Vec::new();

    for entry_res in read_dir {
        let entry = entry_res?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "." || name == ".." {
            continue;
        }

        let meta = entry.metadata()?;
        let is_dir = meta.is_dir();
        let size = if is_dir { 0 } else { meta.len() };

        #[cfg(unix)]
        let (inode, dev, nlink) = {
            use std::os::unix::fs::MetadataExt;
            (meta.ino(), meta.dev(), meta.nlink().try_into().unwrap_or(u32::MAX))
        };
        #[cfg(not(unix))]
        let (inode, dev, nlink) = (0u64, 0u64, 1u32);

        let mut flags = 0u8;
        if is_dir {
            flags |= TREEMAP_FLAG_IS_DIR;
        } else {
            flags |= TREEMAP_FLAG_IS_LEAF;
        }

        let category = classify_tile_category(&name, is_dir);
        if category == TileCategory::ReclaimCache {
            flags |= TREEMAP_FLAG_RECLAIMABLE;
        }

        let child_path = entry.path().to_string_lossy().into_owned();
        entries.push(ScannedEntry {
            name,
            path: child_path,
            is_dir,
            size,
            inode,
            dev,
            nlink: nlink.max(1),
            flags,
            category,
        });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_disable_dataless_materialization_smoke() {
        let res = disable_dataless_materialization();
        #[cfg(target_os = "macos")]
        assert!(res);
        #[cfg(not(target_os = "macos"))]
        let _ = res;
    }

    #[test]
    fn test_scan_directory_entries_integration() {
        let temp_dir = tempfile::tempdir().expect("tempdir creation");
        let root = temp_dir.path();

        let sub_dir = root.join("node_modules");
        fs::create_dir(&sub_dir).expect("create dir");

        let file1 = root.join("main.rs");
        let mut f1 = File::create(&file1).expect("create file");
        f1.write_all(b"fn main() {}").expect("write file");

        let file2 = root.join("data.png");
        let mut f2 = File::create(&file2).expect("create file");
        f2.write_all(&[1, 2, 3, 4, 5, 6, 7, 8]).expect("write file");

        let file2_link = root.join("data_hardlink.png");
        fs::hard_link(&file2, &file2_link).expect("create hardlink");

        let entries = scan_directory_entries(root.to_str().unwrap()).expect("scan entries");
        assert_eq!(entries.len(), 4);

        let nm = entries.iter().find(|e| e.name == "node_modules").expect("node_modules");
        assert!(nm.is_dir);
        assert_eq!(nm.category, TileCategory::ReclaimCache);
        assert_ne!(nm.flags & TREEMAP_FLAG_RECLAIMABLE, 0);

        let rs = entries.iter().find(|e| e.name == "main.rs").expect("main.rs");
        assert!(!rs.is_dir);
        assert_eq!(rs.category, TileCategory::GeneralCode);
        assert!(rs.size >= 12);
        assert_eq!(rs.nlink, 1);

        let img = entries.iter().find(|e| e.name == "data.png").expect("data.png");
        assert!(!img.is_dir);
        assert_eq!(img.category, TileCategory::MediaImage);
        assert!(img.size >= 8);
        assert_eq!(img.nlink, 2);

        let img_link = entries.iter().find(|e| e.name == "data_hardlink.png").expect("data_hardlink.png");
        assert_eq!(img_link.nlink, 2);
        assert_eq!(img_link.inode, img.inode);
    }
}
