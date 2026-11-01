// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Canonical C-ABI 2.0 exports matching `Sources/CTTZipAppCoreBridge/include/ttworkspace_cabi.h`
//! (see `docs/twin/02_vfs_data_plane.md` §4).
//!
//! # Safety & Memory Geometry Invariants
//! - All exposed types use `#[repr(C)]` or `#[repr(C, align(N))]` matching C11 headers.
//! - Strict compile-time static assertions verify byte sizes and field alignments.
//! - Entry points check top-level pointer arguments for null before dereferencing; callers must
//!   still uphold the alignment, validity, and lifetime invariants documented on each function.

#![allow(non_camel_case_types)]

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use ttzip_types::{
    current_trace_context, set_trace_context, TTLogTraceContext, TTWorkspaceEntryNode,
    TTWorkspaceSnapshotHeader,
};

pub const TT_SUCCESS: i32 = 0;
pub const TT_ERROR_INVALID_ARGUMENT: i32 = -1;
pub const TT_ERROR_OUT_OF_MEMORY: i32 = -2;
pub const TT_ERROR_IO_FAILURE: i32 = -3;
pub const TT_ERROR_CANCELLED: i32 = -4;
pub const TT_ERROR_CORRUPT_DATA: i32 = -5;

/// 48-byte, 8-byte aligned borrowed snapshot view (`ttworkspace_cabi.h`).
/// Pointers inside this view borrow from the underlying `ttvfs_lease_s` mapping and must not
/// outlive the owning lease reference. `nodes` points to either 24-byte `TTWorkspaceCompactNode`
/// records or 64-byte `TTWorkspaceEntryNode` records as determined by `header.feature_flags`;
/// consumers must inspect the snapshot feature flags before dereferencing `nodes`.
#[repr(C, align(8))]
#[derive(Copy, Clone, Debug)]
pub struct TTWorkspaceVfsView {
    pub header: *const TTWorkspaceSnapshotHeader,
    pub nodes: *const TTWorkspaceEntryNode,
    pub node_count: u64,
    pub string_arena: *const std::os::raw::c_char,
    pub string_arena_len: u64,
    pub generation: u64,
}

// Canonical SSOT: `ttzip_types::vfs_handle::ttvfs_lease_s` (32 bytes, 8-byte aligned).
pub use ttzip_types::vfs_handle::ttvfs_lease_s;

pub type ttvfs_lease_t = *mut ttvfs_lease_s;

#[repr(C, align(8))]
pub struct TTRemuxOptions {
    pub struct_size: u32,              // 0..4:   sizeof(TTRemuxOptions)
    pub src_fd: i32,                   // 4..8:   Source file descriptor (-1 if unused)
    pub dest_fd: i32,                  // 8..12:  Destination file descriptor (-1 if unused)
    pub timebase_num: u32,             // 12..16: Timebase numerator
    pub timebase_den: u32,             // 16..20: Timebase denominator
    pub _pad_timebase: u32,            // 20..24: Explicit padding to 8-byte boundary
    pub start_time_pts: i64,           // 24..32: Start timestamp in timebase units
    pub duration_pts: i64,             // 32..40: Duration in timebase units
    pub src_path: *const std::os::raw::c_char,  // 40..48
    pub dest_path: *const std::os::raw::c_char, // 48..56
    pub apfs_min_retained_bytes: u64,  // 56..64
    pub apfs_min_retention_bp: u32,    // 64..68
    pub faststart_policy: u32,         // 68..72: TTFastStartPolicy
    pub strip_audio: u8,               // 72..73: 1 = strip, 0 = keep
    pub _pad: [u8; 7],                 // 73..80: Alignment padding
    pub user_data: *mut c_void,        // 80..88
    pub cancel_cb: Option<extern "C" fn(user_data: *mut c_void) -> u8>, // 88..96
}

#[repr(C)]
pub struct ttdevice_cancel_token_s {
    pub is_cancelled: AtomicBool,
}

pub type ttdevice_cancel_token_t = *mut ttdevice_cancel_token_s;

#[repr(C, align(8))]
pub struct TTDeviceReadRequest {
    pub device_id: [u8; 64],                   // 0..64
    pub offset: u64,                           // 64..72
    pub length: u32,                           // 72..76
    pub object_id: u32,                        // 76..80: MTP ObjectHandle
    pub path_offset: u32,                      // 80..84: ADB path offset in string arena
    pub path_len: u16,                         // 84..86: ADB path byte length
    pub _reserved: u16,                        // 86..88: Alignment padding
    pub transport_epoch: u64,                  // 88..96: Transport session epoch
    pub cancel_token: ttdevice_cancel_token_t, // 96..104
}

// Compile-time geometry assertions matching `Sources/CTTZipAppCoreBridge/include/ttworkspace_cabi.h`.
const _: () = {
    assert!(core::mem::size_of::<ttvfs_lease_s>() == 32);
    assert!(core::mem::align_of::<ttvfs_lease_s>() == 8);
    assert!(core::mem::size_of::<TTWorkspaceVfsView>() == 48);
    assert!(core::mem::align_of::<TTWorkspaceVfsView>() == 8);
    assert!(core::mem::size_of::<TTRemuxOptions>() == 96);
    assert!(core::mem::align_of::<TTRemuxOptions>() == 8);
    assert!(core::mem::size_of::<TTDeviceReadRequest>() == 104);
    assert!(core::mem::align_of::<TTDeviceReadRequest>() == 8);
};

#[cfg(target_os = "macos")]
extern "C" {
    fn mach_task_self() -> u32;
    fn mach_vm_deallocate(target: u32, address: u64, size: u64) -> i32;
}

/// Acquires a snapshot view over a shared Mach memory mapping and increments the lease reference count.
///
/// Validates 64-byte header alignment, header self-consistency (`total_mapping_size <= lease.size`),
/// section descriptor table bounds, and the payload range (`offset + size_bytes <= total_mapping_size`),
/// record capacity (`entry_count * stride <= size_bytes`), and alignment (8B compact / 64B CSR) of
/// `TT_SECTION_VFS_NODES` and `TT_SECTION_STRING_ARENA` before constructing any section pointers.
///
/// Returns `1` on success (incrementing `lease.ref_count` by 1, which the caller must balance with
/// `ttvfs_release_snapshot`), or `0` on failure without modifying `ref_count`.
///
/// # Safety
///
/// - `lease` must be null or point to a live, 8-byte aligned `ttvfs_lease_s` whose `mapping` (when
///   non-null) is readable for at least `lease.size` contiguous bytes.
/// - `out_view` must be null or point to writable, 8-byte aligned memory for `TTWorkspaceVfsView`.
/// - Pointers written to `out_view` borrow from the lease mapping and remain valid only while at
///   least one owning reference to `lease` stays alive.
#[no_mangle]
pub unsafe extern "C" fn ttvfs_acquire_snapshot(
    lease: ttvfs_lease_t,
    out_view: *mut TTWorkspaceVfsView,
) -> u8 {
    if lease.is_null() || out_view.is_null() {
        return 0;
    }

    let lease_ref = &*lease;
    let (header_ref, mapping_slice) = match TTWorkspaceSnapshotHeader::validate_mapping(
        lease_ref.mapping,
        lease_ref.size,
    ) {
        Ok(validated) => validated,
        Err(_) => return 0,
    };

    let (vfs_sec, nodes_payload) = match header_ref.validate_nodes_section(mapping_slice) {
        Some(validated) => validated,
        None => return 0,
    };
    let (str_sec, str_payload) = match header_ref.validate_section_bounds(
        mapping_slice,
        ttzip_types::TT_SECTION_STRING_ARENA,
        1,
        1,
    ) {
        Some(validated) => validated,
        None => return 0,
    };

    (*out_view).header = header_ref as *const TTWorkspaceSnapshotHeader;
    (*out_view).nodes = nodes_payload.as_ptr() as *const TTWorkspaceEntryNode;
    (*out_view).node_count = vfs_sec.entry_count;
    (*out_view).string_arena = str_payload.as_ptr() as *const std::os::raw::c_char;
    (*out_view).string_arena_len = str_sec.size_bytes;
    (*out_view).generation = header_ref.generation;

    lease_ref.ref_count.fetch_add(1, Ordering::AcqRel);
    1
}

/// Releases one owning reference to a snapshot lease, deallocating the backing mapping and boxed
/// `ttvfs_lease_s` when the reference count reaches zero.
///
/// # Safety
///
/// - `lease` must be null or point to a `Box`-allocated `ttvfs_lease_s` holding at least one owning
///   reference acquired at creation or via `ttvfs_acquire_snapshot`.
/// - When `lease.alloc_ptr` is non-null, it must originate from `Arc::into_raw` for
///   `ttzip_types::MachVmAllocation`; otherwise `lease.mapping` and `lease.size` must describe a
///   Mach VM region owned by this lease.
#[no_mangle]
pub unsafe extern "C" fn ttvfs_release_snapshot(lease: ttvfs_lease_t) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if lease.is_null() {
            return;
        }
        let lease_ref = unsafe { &*lease };
        if lease_ref.ref_count.fetch_sub(1, Ordering::Release) == 1 {
            core::sync::atomic::fence(Ordering::Acquire);
            if !lease_ref.alloc_ptr.is_null() {
                drop(unsafe {
                    Arc::from_raw(lease_ref.alloc_ptr as *const ttzip_types::MachVmAllocation)
                });
            } else if !lease_ref.mapping.is_null() && lease_ref.size > 0 {
                #[cfg(target_os = "macos")]
                {
                    let _ = unsafe {
                        mach_vm_deallocate(
                            mach_task_self(),
                            lease_ref.mapping as u64,
                            lease_ref.size as u64,
                        )
                    };
                }
            }
            drop(unsafe { Box::from_raw(lease) });
        }
    }));
}

#[no_mangle]
pub extern "C" fn ttdevice_cancel_token_create() -> ttdevice_cancel_token_t {
    Box::into_raw(Box::new(ttdevice_cancel_token_s {
        is_cancelled: AtomicBool::new(false),
    }))
}

/// Destroys a cancel token handle allocated by `ttdevice_cancel_token_create`.
///
/// # Safety
///
/// `token` must be null or a pointer returned by `ttdevice_cancel_token_create` that has not yet
/// been destroyed and is not concurrently accessed by any reader or writer.
#[no_mangle]
pub unsafe extern "C" fn ttdevice_cancel_token_destroy(token: ttdevice_cancel_token_t) {
    if !token.is_null() {
        drop(Box::from_raw(token));
    }
}

/// Signals cancellation on `token` with `Release` ordering.
///
/// # Safety
///
/// `token` must be null or a valid, aligned pointer to a live `ttdevice_cancel_token_s`.
#[no_mangle]
pub unsafe extern "C" fn ttdevice_cancel_token_cancel(token: ttdevice_cancel_token_t) {
    if let Some(t) = token.as_ref() {
        t.is_cancelled.store(true, Ordering::Release);
    }
}

/// Returns `1` if cancellation was signaled on `token` (`Acquire` load), or `0` if not cancelled or null.
///
/// # Safety
///
/// `token` must be null or a valid, aligned pointer to a live `ttdevice_cancel_token_s`.
#[no_mangle]
pub unsafe extern "C" fn ttdevice_cancel_token_is_cancelled(token: ttdevice_cancel_token_t) -> u8 {
    if let Some(t) = token.as_ref() {
        if t.is_cancelled.load(Ordering::Acquire) { 1 } else { 0 }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn ttlog_current_context() -> TTLogTraceContext {
    current_trace_context()
}

#[no_mangle]
pub extern "C" fn ttlog_set_context(ctx: TTLogTraceContext) {
    set_trace_context(ctx);
}



#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::AtomicU64;

    #[test]
    fn test_cabi_struct_geometries() {
        assert_eq!(core::mem::size_of::<ttvfs_lease_s>(), 32);
        assert_eq!(core::mem::align_of::<ttvfs_lease_s>(), 8);
        assert_eq!(core::mem::size_of::<TTWorkspaceVfsView>(), 48);
        assert_eq!(core::mem::align_of::<TTWorkspaceVfsView>(), 8);
        assert_eq!(core::mem::size_of::<TTRemuxOptions>(), 96);
        assert_eq!(core::mem::align_of::<TTRemuxOptions>(), 8);
        assert_eq!(core::mem::size_of::<TTDeviceReadRequest>(), 104);
        assert_eq!(core::mem::align_of::<TTDeviceReadRequest>(), 8);
    }

    #[test]
    fn test_ttvfs_release_snapshot_lifecycle() {
        unsafe {
            ttvfs_release_snapshot(std::ptr::null_mut());
        }
        let lease = Box::into_raw(Box::new(ttvfs_lease_s {
            mapping: std::ptr::null_mut(),
            size: 0,
            ref_count: AtomicU64::new(2),
            alloc_ptr: std::ptr::null(),
        }));
        unsafe {
            ttvfs_release_snapshot(lease);
            assert_eq!((*lease).ref_count.load(Ordering::Acquire), 1);
            ttvfs_release_snapshot(lease);
        }
    }

    #[test]
    fn test_cancel_token_lifecycle() {
        let token = ttdevice_cancel_token_create();
        assert!(!token.is_null());
        unsafe {
            assert_eq!(ttdevice_cancel_token_is_cancelled(token), 0);
            ttdevice_cancel_token_cancel(token);
            assert_eq!(ttdevice_cancel_token_is_cancelled(token), 1);
            ttdevice_cancel_token_destroy(token);
        }
    }

    #[test]
    fn test_telemetry_cabi_context() {
        let ctx = TTLogTraceContext::new(0xABCD, 0x1234, 0x01);
        ttlog_set_context(ctx);
        let current = ttlog_current_context();
        assert_eq!(current.trace_id, 0xABCD);
        assert_eq!(current.span_id, 0x1234);
    }

    #[repr(C, align(64))]
    struct AlignedPage([u8; ttzip_types::MACH_VM_PAGE_SIZE_16KB]);

    fn write_test_snapshot(
        buf: &mut [u8],
        header: TTWorkspaceSnapshotHeader,
        sections: &[ttzip_types::TTWorkspaceSectionEntry],
    ) {
        unsafe {
            core::ptr::write(buf.as_mut_ptr() as *mut TTWorkspaceSnapshotHeader, header);
            core::ptr::copy_nonoverlapping(
                sections.as_ptr(),
                buf.as_mut_ptr().add(128) as *mut ttzip_types::TTWorkspaceSectionEntry,
                sections.len(),
            );
        }
    }

    #[test]
    fn test_ttvfs_acquire_snapshot_valid_and_r1_to_r4_boundaries() {
        let mut page = Box::new(AlignedPage([0u8; ttzip_types::MACH_VM_PAGE_SIZE_16KB]));
        let buf = &mut page.0[..];

        let valid_header = TTWorkspaceSnapshotHeader {
            total_mapping_size: 16384,
            section_count: 2,
            section_headers_offset: 128,
            feature_flags: ttzip_types::TT_FEATURE_FLAG_CSR,
            ..TTWorkspaceSnapshotHeader::default()
        };
        let valid_sections = [
            ttzip_types::TTWorkspaceSectionEntry {
                section_type: ttzip_types::TT_SECTION_VFS_NODES,
                flags: ttzip_types::TT_SECTION_FLAG_ALIGNED_64,
                offset: 256,
                size_bytes: 64,
                entry_count: 1,
            },
            ttzip_types::TTWorkspaceSectionEntry {
                section_type: ttzip_types::TT_SECTION_STRING_ARENA,
                flags: ttzip_types::TT_SECTION_FLAG_READ_ONLY,
                offset: 512,
                size_bytes: 16,
                entry_count: 16,
            },
        ];
        write_test_snapshot(buf, valid_header, &valid_sections);

        let mut lease = ttvfs_lease_s {
            mapping: buf.as_mut_ptr(),
            size: buf.len(),
            ref_count: AtomicU64::new(1),
            alloc_ptr: core::ptr::null(),
        };
        let mut view = core::mem::MaybeUninit::<TTWorkspaceVfsView>::uninit();

        unsafe {
            // Valid snapshot succeeds and increments ref_count to 2
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 1);
            assert_eq!(lease.ref_count.load(Ordering::Acquire), 2);
            let populated = view.assume_init();
            assert_eq!(populated.node_count, 1);
            assert_eq!(populated.string_arena_len, 16);
            lease.ref_count.store(1, Ordering::Release);

            // R1: header.total_mapping_size (65536) > lease.size (16384)
            let mut r1_header = valid_header;
            r1_header.total_mapping_size = 65536;
            write_test_snapshot(buf, r1_header, &valid_sections);
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 0);
            assert_eq!(lease.ref_count.load(Ordering::Acquire), 1);

            // R2: section offset + size_bytes > total_mapping_size (and u64::MAX overflow)
            let r2_sections = [
                valid_sections[0],
                ttzip_types::TTWorkspaceSectionEntry {
                    section_type: ttzip_types::TT_SECTION_STRING_ARENA,
                    flags: ttzip_types::TT_SECTION_FLAG_READ_ONLY,
                    offset: 16320,
                    size_bytes: 128, // 16448 > 16384
                    entry_count: 16,
                },
            ];
            write_test_snapshot(buf, valid_header, &r2_sections);
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 0);

            let r2_overflow_sections = [
                valid_sections[0],
                ttzip_types::TTWorkspaceSectionEntry {
                    section_type: ttzip_types::TT_SECTION_STRING_ARENA,
                    flags: ttzip_types::TT_SECTION_FLAG_READ_ONLY,
                    offset: u64::MAX - 8,
                    size_bytes: 64,
                    entry_count: 0,
                },
            ];
            write_test_snapshot(buf, valid_header, &r2_overflow_sections);
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 0);

            // R3: entry_count * stride > size_bytes (insufficient capacity) and entry_count = u64::MAX (multiplication overflow)
            let mut r3_csr_sections = [
                ttzip_types::TTWorkspaceSectionEntry {
                    section_type: ttzip_types::TT_SECTION_VFS_NODES,
                    flags: ttzip_types::TT_SECTION_FLAG_ALIGNED_64,
                    offset: 256,
                    size_bytes: 64,
                    entry_count: 2, // 2 * 64 = 128 > 64
                },
                valid_sections[1],
            ];
            write_test_snapshot(buf, valid_header, &r3_csr_sections);
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 0);

            r3_csr_sections[0].entry_count = u64::MAX; // checked_mul(64) overflow with payload [256 .. 320] inside mapping
            write_test_snapshot(buf, valid_header, &r3_csr_sections);
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 0);

            let mut compact_header = valid_header;
            compact_header.feature_flags = ttzip_types::TT_FEATURE_FLAG_COMPACT_NODES;
            let mut r3_compact_sections = [
                ttzip_types::TTWorkspaceSectionEntry {
                    section_type: ttzip_types::TT_SECTION_VFS_NODES,
                    flags: 0,
                    offset: 256,
                    size_bytes: 24,
                    entry_count: 2, // 2 * 24 = 48 > 24
                },
                valid_sections[1],
            ];
            write_test_snapshot(buf, compact_header, &r3_compact_sections);
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 0);

            r3_compact_sections[0].entry_count = u64::MAX; // checked_mul(24) overflow with payload [256 .. 280] inside mapping
            write_test_snapshot(buf, compact_header, &r3_compact_sections);
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 0);

            // R4(a): Unaligned lease.mapping (+8 bytes from 64B-aligned base)
            write_test_snapshot(buf, valid_header, &valid_sections);
            let unaligned_lease = ttvfs_lease_s {
                mapping: buf.as_mut_ptr().add(8),
                size: buf.len() - 8,
                ref_count: AtomicU64::new(1),
                alloc_ptr: core::ptr::null(),
            };
            assert_eq!(
                ttvfs_acquire_snapshot(&unaligned_lease as *const _ as *mut _, view.as_mut_ptr()),
                0
            );

            // R4(b): Unaligned node section offset (260 not 64B-aligned for CSR, 258 not 8B-aligned for compact)
            let r4_csr_sections = [
                ttzip_types::TTWorkspaceSectionEntry {
                    section_type: ttzip_types::TT_SECTION_VFS_NODES,
                    flags: ttzip_types::TT_SECTION_FLAG_ALIGNED_64,
                    offset: 260,
                    size_bytes: 64,
                    entry_count: 1,
                },
                valid_sections[1],
            ];
            write_test_snapshot(buf, valid_header, &r4_csr_sections);
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 0);

            let r4_compact_sections = [
                ttzip_types::TTWorkspaceSectionEntry {
                    section_type: ttzip_types::TT_SECTION_VFS_NODES,
                    flags: 0,
                    offset: 258,
                    size_bytes: 24,
                    entry_count: 1,
                },
                valid_sections[1],
            ];
            write_test_snapshot(buf, compact_header, &r4_compact_sections);
            assert_eq!(ttvfs_acquire_snapshot(&mut lease, view.as_mut_ptr()), 0);
            assert_eq!(lease.ref_count.load(Ordering::Acquire), 1);
        }
    }
}
