// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Physical USB hardware interface scanner and bulk transport driver based on rusb.
//!
//! Conforms strictly to the Q3 non-preemption safety contract: discovery, bus scanning,
//! and device opening never automatically terminate system processes (`ptpcamerad`) or
//! the host ADB daemon (`host:kill`). When a USB interface is exclusively locked by
//! another process, an explicit busy/unauthorized error with recovery guidance is returned.

use rusb::{Context, Device, DeviceDescriptor, DeviceHandle, Direction, TransferType, UsbContext};
use std::time::Duration;

use crate::device::mtp::types::MtpDeviceInfo;
use crate::error::{AppCoreError, DeviceErrorKind};

mod selection;
pub use selection::select_unique_mtp_device;

pub const DEFAULT_USB_TIMEOUT: Duration = Duration::from_millis(5000);

/// Abstract USB transport interface for MTP bulk I/O, allowing physical and mocked hardware.
pub trait UsbBulkTransport: Send + Sync {
    fn write_bulk(&mut self, data: &[u8], timeout: Duration) -> Result<usize, AppCoreError>;
    fn read_bulk(&mut self, buffer: &mut [u8], timeout: Duration) -> Result<usize, AppCoreError>;
    fn endpoint_in(&self) -> u8;
    fn endpoint_out(&self) -> u8;
}

/// Endpoint configuration discovered for an MTP USB interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MtpEndpoints {
    pub interface_number: u8,
    pub bulk_in: u8,
    pub bulk_out: u8,
    pub interrupt_in: Option<u8>,
}

/// Real hardware USB transport backed by `rusb::DeviceHandle`.
pub struct RusbTransport {
    handle: DeviceHandle<Context>,
    endpoints: MtpEndpoints,
}

/// Formats a non-destructive recovery message when a USB MTP device or interface is busy/locked.
pub fn format_usb_busy_recovery_message(context: &str, err: &rusb::Error) -> String {
    let ptpcamerad_hint = if crate::device::mtp_bulk::is_ptpcamerad_active() {
        "macOS ptpcamerad (Photos / Image Capture) is currently active."
    } else {
        "Another process (such as macOS Image Capture/Photos, Android File Transfer, or a composite USB lock held by adb) is claiming the USB device."
    };
    format!(
        "{}: {} ({}). Recovery: close conflicting camera/transfer apps or switch to ADB mode, then retry.",
        context, err, ptpcamerad_hint
    )
}

/// Maps a `rusb::Error` during device open or interface claim into a structured `AppCoreError`
/// without terminating any external system or ADB processes.
pub fn map_rusb_open_error(context: &str, err: rusb::Error) -> AppCoreError {
    match err {
        rusb::Error::NoDevice => AppCoreError::usb(
            DeviceErrorKind::Offline,
            format!("{}: USB device disconnected ({})", context, err),
        ),
        rusb::Error::Access | rusb::Error::Busy => AppCoreError::usb(
            DeviceErrorKind::Unauthorized,
            format_usb_busy_recovery_message(context, &err),
        ),
        other => AppCoreError::usb(
            DeviceErrorKind::Transport,
            format!("{}: {}", context, other),
        ),
    }
}

/// Verifies that the hardware serial number read from an opened USB device handle
/// matches the expected target device identity before any MTP interface claim or session starts.
pub fn verify_opened_usb_device_identity(
    expected_serial: &str,
    opened_serial: Result<&str, &str>,
) -> Result<(), AppCoreError> {
    let expected = expected_serial.trim();
    if expected.is_empty() {
        return Err(AppCoreError::InvalidState {
            reason:
                "Target MTP device serial number is empty; refusing to open unverified USB device"
                    .to_string(),
        });
    }
    match opened_serial {
        Ok(actual) => {
            let actual_trimmed = actual.trim();
            if actual_trimmed.is_empty() {
                return Err(AppCoreError::InvalidState {
                    reason: format!(
                        "Opened USB device returned an empty serial number (expected '{}')",
                        expected
                    ),
                });
            }
            if actual_trimmed != expected {
                return Err(AppCoreError::InvalidState {
                    reason: format!(
                        "Opened USB device identity changed: expected serial '{}', opened handle reported '{}'",
                        expected, actual_trimmed
                    ),
                });
            }
            Ok(())
        }
        Err(err) => Err(AppCoreError::InvalidState {
            reason: format!(
                "Failed to confirm hardware serial number from opened USB handle (expected '{}'): {}",
                expected, err
            ),
        }),
    }
}

impl RusbTransport {
    /// Opens the specified USB device, claims its MTP interface, and prepares endpoints
    /// without killing external system processes or the host ADB server.
    pub fn open(device: &Device<Context>, endpoints: MtpEndpoints) -> Result<Self, AppCoreError> {
        let handle = device
            .open()
            .map_err(|e| map_rusb_open_error("Failed to open USB MTP device", e))?;
        Self::claim_and_drain(handle, endpoints)
    }

    /// Opens the specified USB device and verifies that the opened handle's hardware serial number
    /// string descriptor matches `expected_serial` BEFORE claiming the MTP interface.
    pub fn open_verified(
        device: &Device<Context>,
        desc: &rusb::DeviceDescriptor,
        endpoints: MtpEndpoints,
        expected_serial: &str,
    ) -> Result<Self, AppCoreError> {
        let handle = device
            .open()
            .map_err(|e| map_rusb_open_error("Failed to open USB MTP device", e))?;

        let opened_serial_res = handle
            .read_serial_number_string_ascii(desc)
            .map_err(|e| format!("usb serial descriptor read error: {}", e));
        verify_opened_usb_device_identity(
            expected_serial,
            opened_serial_res.as_deref().map_err(String::as_str),
        )?;

        Self::claim_and_drain(handle, endpoints)
    }

    /// Claims one verified directory interface without an unbounded stale-packet drain.
    pub fn open_directory(
        device: &Device<Context>,
        desc: &rusb::DeviceDescriptor,
        endpoints: MtpEndpoints,
        expected_serial: &str,
    ) -> Result<Self, AppCoreError> {
        let handle = device.open().map_err(|err| map_rusb_open_error("Open MTP directory device", err))?;
        let serial = handle.read_serial_number_string_ascii(desc).map_err(|err| err.to_string());
        verify_opened_usb_device_identity(expected_serial, serial.as_deref().map_err(String::as_str))?;
        handle.claim_interface(endpoints.interface_number)
            .map_err(|err| map_rusb_open_error("Claim MTP directory interface", err))?;
        Ok(Self { handle, endpoints })
    }

    fn claim_and_drain(
        handle: DeviceHandle<Context>,
        endpoints: MtpEndpoints,
    ) -> Result<Self, AppCoreError> {
        if let Ok(active) = handle.kernel_driver_active(endpoints.interface_number) {
            if active {
                let _ = handle.detach_kernel_driver(endpoints.interface_number);
            }
        }

        handle
            .claim_interface(endpoints.interface_number)
            .map_err(|e| {
                map_rusb_open_error(
                    &format!(
                        "Failed to claim MTP interface {}",
                        endpoints.interface_number
                    ),
                    e,
                )
            })?;

        let mut drain_buf = [0u8; 512];
        while handle
            .read_bulk(endpoints.bulk_in, &mut drain_buf, Duration::from_millis(15))
            .is_ok_and(|n| n > 0)
        {}

        Ok(Self { handle, endpoints })
    }
}

impl UsbBulkTransport for RusbTransport {
    fn write_bulk(&mut self, data: &[u8], timeout: Duration) -> Result<usize, AppCoreError> {
        self.handle
            .write_bulk(self.endpoints.bulk_out, data, timeout)
            .map_err(|e| match e {
                rusb::Error::Timeout => AppCoreError::usb(
                    DeviceErrorKind::Timeout,
                    format!(
                        "USB bulk write timed out after {:?} on endpoint 0x{:02X}",
                        timeout, self.endpoints.bulk_out
                    ),
                ),
                rusb::Error::NoDevice => AppCoreError::usb(
                    DeviceErrorKind::Offline,
                    format!(
                        "USB bulk write failed: device disconnected on ep 0x{:02X}",
                        self.endpoints.bulk_out
                    ),
                ),
                other => AppCoreError::usb(
                    DeviceErrorKind::Transport,
                    format!(
                        "USB bulk write failed on ep 0x{:02X}: {}",
                        self.endpoints.bulk_out, other
                    ),
                ),
            })
    }

    fn read_bulk(&mut self, buffer: &mut [u8], timeout: Duration) -> Result<usize, AppCoreError> {
        self.handle
            .read_bulk(self.endpoints.bulk_in, buffer, timeout)
            .map_err(|e| match e {
                rusb::Error::Timeout => AppCoreError::usb(
                    DeviceErrorKind::Timeout,
                    format!(
                        "USB bulk read timed out after {:?} on endpoint 0x{:02X}",
                        timeout, self.endpoints.bulk_in
                    ),
                ),
                rusb::Error::NoDevice => AppCoreError::usb(
                    DeviceErrorKind::Offline,
                    format!(
                        "USB bulk read failed: device disconnected on ep 0x{:02X}",
                        self.endpoints.bulk_in
                    ),
                ),
                other => AppCoreError::usb(
                    DeviceErrorKind::Transport,
                    format!(
                        "USB bulk read failed on ep 0x{:02X}: {}",
                        self.endpoints.bulk_in, other
                    ),
                ),
            })
    }

    fn endpoint_in(&self) -> u8 {
        self.endpoints.bulk_in
    }

    fn endpoint_out(&self) -> u8 {
        self.endpoints.bulk_out
    }
}

impl Drop for RusbTransport {
    fn drop(&mut self) {
        let _ = self
            .handle
            .release_interface(self.endpoints.interface_number);
    }
}

/// Inspects USB device interfaces to locate valid MTP endpoints.
pub fn inspect_device_endpoints<T: UsbContext>(
    device: &Device<T>,
    desc: &DeviceDescriptor,
) -> Result<Option<MtpEndpoints>, AppCoreError> {
    for config_idx in 0..desc.num_configurations() {
        let config_desc = match device.config_descriptor(config_idx) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for interface in config_desc.interfaces() {
            for interface_desc in interface.descriptors() {
                let is_ptp_class = interface_desc.class_code() == 0x06
                    && interface_desc.sub_class_code() == 0x01
                    && interface_desc.protocol_code() == 0x01;
                let is_adb = interface_desc.class_code() == 0xFF
                    && interface_desc.sub_class_code() == 0x42
                    && interface_desc.protocol_code() == 0x01;
                let is_vendor_mtp = interface_desc.class_code() == 0xFF && !is_adb;

                if is_ptp_class || is_vendor_mtp {
                    let mut bulk_in = None;
                    let mut bulk_out = None;
                    let mut interrupt_in = None;

                    for ep in interface_desc.endpoint_descriptors() {
                        match (ep.direction(), ep.transfer_type()) {
                            (Direction::In, TransferType::Bulk) => bulk_in = Some(ep.address()),
                            (Direction::Out, TransferType::Bulk) => bulk_out = Some(ep.address()),
                            (Direction::In, TransferType::Interrupt) => {
                                interrupt_in = Some(ep.address())
                            }
                            _ => {}
                        }
                    }

                    if let (Some(b_in), Some(b_out)) = (bulk_in, bulk_out) {
                        return Ok(Some(MtpEndpoints {
                            interface_number: interface_desc.interface_number(),
                            bulk_in: b_in,
                            bulk_out: b_out,
                            interrupt_in,
                        }));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Scans the physical USB bus for real MTP-compatible hardware devices non-destructively.
///
/// Never kills `ptpcamerad` or `adb` server. If a specific USB device is locked by another
/// process (`device.open()` returns `Err`), its descriptor metadata (`vendor_id`, `product_id`,
/// `bus_number`, `device_address`) is still collected without aborting enumeration of other devices.
pub fn scan_physical_mtp_devices() -> Result<Vec<MtpDeviceInfo>, AppCoreError> {
    let context = Context::new().map_err(|e| {
        AppCoreError::usb(
            DeviceErrorKind::Transport,
            format!("Failed to initialize libusb context: {}", e),
        )
    })?;

    let devices = context.devices().map_err(|e| {
        AppCoreError::usb(
            DeviceErrorKind::Transport,
            format!("Failed to enumerate USB devices: {}", e),
        )
    })?;

    let mut result = Vec::new();

    for device in devices.iter() {
        let desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if let Ok(Some(_)) = inspect_device_endpoints(&device, &desc) {
            let handle = device.open().ok();

            let manufacturer = handle
                .as_ref()
                .and_then(|h| h.read_manufacturer_string_ascii(&desc).ok())
                .unwrap_or_else(|| "Unknown".to_string());

            let model = handle
                .as_ref()
                .and_then(|h| h.read_product_string_ascii(&desc).ok())
                .unwrap_or_else(|| "MTP Device".to_string());

            let serial_number = handle
                .as_ref()
                .and_then(|h| h.read_serial_number_string_ascii(&desc).ok())
                .unwrap_or_else(|| format!("{:04X}:{:04X}", desc.vendor_id(), desc.product_id()));

            result.push(MtpDeviceInfo {
                vendor_id: desc.vendor_id(),
                product_id: desc.product_id(),
                bus_number: device.bus_number(),
                device_address: device.address(),
                manufacturer,
                model,
                serial_number,
                device_version: format!(
                    "{}.{}",
                    desc.device_version().major(),
                    desc.device_version().minor()
                ),
                ..Default::default()
            });
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha256_hex(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    #[test]
    fn test_non_preemptive_busy_error_preserves_adb_session_and_returns_recovery_guide() {
        assert!(crate::device::mtp_bulk::preempt_ptpcamerad().is_ok());

        let access_err = map_rusb_open_error("Failed to open USB MTP device", rusb::Error::Access);
        match access_err {
            AppCoreError::Usb { kind, reason } => {
                assert_eq!(kind, DeviceErrorKind::Unauthorized);
                assert!(reason.contains("Recovery:"));
            }
            other => panic!("Expected Usb Unauthorized error, got {:?}", other),
        }

        let busy_err = map_rusb_open_error("Failed to claim MTP interface 0", rusb::Error::Busy);
        match busy_err {
            AppCoreError::Usb { kind, reason } => {
                assert_eq!(kind, DeviceErrorKind::Unauthorized);
                assert!(reason.contains("Recovery:"));
            }
            other => panic!("Expected Usb Unauthorized error, got {:?}", other),
        }
    }

    #[test]
    fn test_select_unique_mtp_device_rejects_same_model_vid_pid_ambiguity_and_binds_exact_serial() {
        let simulated_devices = vec![
            MtpDeviceInfo {
                vendor_id: 0x0E8D,
                product_id: 0x201D,
                bus_number: 20,
                device_address: 12,
                manufacturer: "iFLYTEK".to_string(),
                model: "AINOTE X5".to_string(),
                serial_number: "XFDXT510EF46P04061".to_string(),
                ..Default::default()
            },
            MtpDeviceInfo {
                vendor_id: 0x0E8D,
                product_id: 0x201D,
                bus_number: 20,
                device_address: 19,
                manufacturer: "iFLYTEK".to_string(),
                model: "AINOTE X5".to_string(),
                serial_number: "XFDXT510EF46P99999".to_string(),
                ..Default::default()
            },
        ];

        // Ambiguous VID:PID selector when two devices of the same model are connected MUST be rejected.
        let err_vid_pid = select_unique_mtp_device(&simulated_devices, "0E8D:201D")
            .expect_err("VID:PID selector must fail when two same-model devices are connected");
        assert!(err_vid_pid
            .to_string()
            .contains("Ambiguous MTP device selector"));

        let err_uri = select_unique_mtp_device(&simulated_devices, "mtp://0E8D:201D").expect_err(
            "mtp://VID:PID selector must fail when two same-model devices are connected",
        );
        assert!(err_uri
            .to_string()
            .contains("Ambiguous MTP device selector"));

        let err_empty = select_unique_mtp_device(&simulated_devices, "")
            .expect_err("Empty selector must fail when multiple devices are connected");
        assert!(err_empty
            .to_string()
            .contains("Ambiguous empty MTP device selector"));

        // Selecting by exact serial number of the SECOND device must bind to bus 20, addr 19 (never picking the first).
        let bound_second = select_unique_mtp_device(&simulated_devices, "mtp://XFDXT510EF46P99999")
            .expect("Unique serial selector for second device must succeed");
        assert_eq!(bound_second.serial_number, "XFDXT510EF46P99999");
        assert_eq!(bound_second.bus_number, 20);
        assert_eq!(bound_second.device_address, 19);

        let bound_first = select_unique_mtp_device(&simulated_devices, "XFDXT510EF46P04061")
            .expect("Unique serial selector for first device must succeed");
        assert_eq!(bound_first.serial_number, "XFDXT510EF46P04061");
        assert_eq!(bound_first.bus_number, 20);
        assert_eq!(bound_first.device_address, 12);
    }

    #[test]
    fn test_verify_opened_usb_device_identity_rejects_mismatch_and_unreadable_serial() {
        // 1. Exact hardware serial match succeeds.
        assert!(
            verify_opened_usb_device_identity("XFDXT510EF46P04061", Ok("XFDXT510EF46P04061"))
                .is_ok()
        );

        // 2. Identity changed after bus/address re-enumeration (different serial on same port) -> rejected.
        let mismatch_err =
            verify_opened_usb_device_identity("XFDXT510EF46P04061", Ok("XFDXT510EF46P99999"))
                .expect_err("Mismatched opened USB serial must abort before interface claim");
        assert!(mismatch_err
            .to_string()
            .contains("Opened USB device identity changed"));

        // 3. Serial number unreadable or empty -> rejected.
        let read_err =
            verify_opened_usb_device_identity("XFDXT510EF46P04061", Err("descriptor read timeout"))
                .expect_err("Unreadable opened USB serial must abort");
        assert!(read_err
            .to_string()
            .contains("Failed to confirm hardware serial number"));

        let empty_err = verify_opened_usb_device_identity("XFDXT510EF46P04061", Ok("   "))
            .expect_err("Empty opened USB serial must abort");
        assert!(empty_err
            .to_string()
            .contains("returned an empty serial number"));
    }

    #[test]
    fn test_scan_physical_mtp_devices_and_preempt() {
        // Default test suite must NEVER touch or write to connected physical devices.
        // Only run hardware E2E when explicitly enabled with target device identity.
        let enabled = std::env::var("TTZIP_ENABLE_REAL_DEVICE_TESTS").as_deref() == Ok("1");
        let target_serial = std::env::var("TTZIP_TEST_MTP_SERIAL").unwrap_or_default();
        if !enabled || target_serial.trim().is_empty() {
            println!(
                "SKIPPED [UNTESTED IN DEFAULT RUN]: Real-device MTP write test requires TTZIP_ENABLE_REAL_DEVICE_TESTS=1 and TTZIP_TEST_MTP_SERIAL=<serial>."
            );
            return;
        }

        let devices = scan_physical_mtp_devices().expect("scan_physical_mtp_devices must succeed");
        println!("Scanned physical MTP devices: {:?}", devices);

        let matched_info = select_unique_mtp_device(&devices, &target_serial)
            .expect("Explicitly requested real MTP device must resolve unambiguously");

        let ctx = Context::new().expect("rusb context");
        let mut executed = false;
        for dev in ctx.devices().expect("usb devices").iter() {
            if dev.bus_number() != matched_info.bus_number
                || dev.address() != matched_info.device_address
            {
                continue;
            }
            let Ok(desc) = dev.device_descriptor() else {
                continue;
            };
            if desc.vendor_id() != matched_info.vendor_id
                || desc.product_id() != matched_info.product_id
            {
                continue;
            }
            let Ok(Some(ep)) = inspect_device_endpoints(&dev, &desc) else {
                continue;
            };
            let transport =
                RusbTransport::open_verified(&dev, &desc, ep, &matched_info.serial_number)
                    .expect("RusbTransport::open_verified must confirm opened USB serial");
            let opened_serial = matched_info.serial_number.clone();

            let mut session = crate::device::mtp::session::MtpSession::new(transport);
            session
                .open_session(1)
                .expect("open_session(1) on physical MTP device");
            if let Ok(dev_info) = session.get_device_info() {
                println!(
                    "DEVICE_INFO: manufacturer='{}' model='{}' version='{}' serial='{}'",
                    dev_info.manufacturer,
                    dev_info.model,
                    dev_info.device_version,
                    dev_info.serial_number
                );
            }

            let sids = session.get_storage_ids().expect("get_storage_ids");
            let sid = *sids.first().expect("at least one storage partition");
            if let Ok(s_info) = session.get_storage_info(sid) {
                println!(
                    "STORAGE_INFO: sid=0x{:08X} desc='{}' total_bytes={} free_bytes={}",
                    sid,
                    s_info.storage_description,
                    s_info.max_capacity,
                    s_info.free_space_in_bytes
                );
            }

            let run_id = uuid::Uuid::new_v4().simple().to_string();
            let root_test_dir_name = format!("TTZip_Q3_Verify_{}", &run_id[..8]);
            let dir_h = session
                .create_directory(sid, 0, &root_test_dir_name)
                .expect("create unique verification folder");
            let subdir_h = session
                .create_directory(sid, dir_h, "子目录 Level 2 ✨")
                .expect("create nested subdirectory");

            let small_payload: Vec<u8> = (0..1024).map(|i| ((i * 37 + 13) & 0xFF) as u8).collect();
            let large_16m_payload: Vec<u8> = (0..(16 * 1024 * 1024))
                .map(|i| ((i * 29 + 11) & 0xFF) as u8)
                .collect();

            let mut created_files = Vec::new();
            for (parent, name, payload) in [
                (dir_h, "空文件 0B.txt", &[][..]),
                (dir_h, "测试 小文件 空格 🚀.txt", small_payload.as_slice()),
                (
                    subdir_h,
                    "大文件 16MB 校验 📦.bin",
                    large_16m_payload.as_slice(),
                ),
            ] {
                let expected_hash = sha256_hex(payload);
                let mut reader = std::io::Cursor::new(payload);
                let mut stream_hasher = Sha256::new();
                let file_h = session
                    .send_object_from_reader(
                        sid,
                        parent,
                        name,
                        payload.len() as u64,
                        &mut reader,
                        |chunk, _sent| {
                            stream_hasher.update(chunk);
                            Ok(())
                        },
                    )
                    .expect("MTP send_object_from_reader upload must succeed");
                let streamed_hash = format!("{:x}", stream_hasher.finalize());
                assert_eq!(streamed_hash, expected_hash);
                created_files.push(file_h);

                let downloaded = session.get_object(file_h).expect("MTP get_object download");
                let actual_hash = sha256_hex(&downloaded);
                println!(
                    "RAW_HARDWARE_LOG serial='{}' bus={} addr={} file='{}' handle={} bytes={} sha256={}",
                    opened_serial,
                    matched_info.bus_number,
                    matched_info.device_address,
                    name,
                    file_h,
                    downloaded.len(),
                    actual_hash
                );
                assert_eq!(actual_hash, expected_hash);
            }

            // 1. Directory Browsing & Refresh Verification
            let listed_handles = session
                .get_object_handles(sid, 0, dir_h)
                .expect("list dir_h");
            assert!(listed_handles.contains(&subdir_h));
            let sub_info = session.get_object_info(subdir_h).expect("get subdir info");
            assert_eq!(sub_info.filename, "子目录 Level 2 ✨");

            // 2. Conflict Policies & Target Protection Verification
            use crate::device::mtp::transfer::{push_reader_with_session, MtpConflictPolicy};
            let conflict_file = "冲突策略探测.txt";
            let c_data1 = b"initial content 111";
            let out1 = push_reader_with_session(
                &mut session,
                sid,
                &root_test_dir_name,
                conflict_file,
                c_data1.len() as u64,
                &mut std::io::Cursor::new(c_data1),
                None,
                MtpConflictPolicy::Overwrite,
                10,
                |_, _| Ok(()),
            )
            .expect("initial upload");
            created_files.push(out1.created_handle);

            let c_data2 = b"skip candidate 222";
            let out_skip = push_reader_with_session(
                &mut session,
                sid,
                &root_test_dir_name,
                conflict_file,
                c_data2.len() as u64,
                &mut std::io::Cursor::new(c_data2),
                None,
                MtpConflictPolicy::Skip,
                10,
                |_, _| Ok(()),
            )
            .expect("skip upload");
            assert!(out_skip.skipped);
            assert_eq!(out_skip.created_handle, out1.created_handle);

            let out_kb = push_reader_with_session(
                &mut session,
                sid,
                &root_test_dir_name,
                conflict_file,
                c_data2.len() as u64,
                &mut std::io::Cursor::new(c_data2),
                None,
                MtpConflictPolicy::KeepBoth,
                10,
                |_, _| Ok(()),
            )
            .expect("keepBoth upload");
            assert_eq!(out_kb.final_filename, "冲突策略探测 (1).txt");
            created_files.push(out_kb.created_handle);

            let c_data3 = b"overwrite candidate 333";
            let out_ow = push_reader_with_session(
                &mut session,
                sid,
                &root_test_dir_name,
                conflict_file,
                c_data3.len() as u64,
                &mut std::io::Cursor::new(c_data3),
                None,
                MtpConflictPolicy::Overwrite,
                10,
                |_, _| Ok(()),
            )
            .expect("overwrite upload");
            assert_eq!(out_ow.final_filename, conflict_file);
            created_files.push(out_ow.created_handle);

            // 3. Mid-Flight Transfer Cancellation & Teardown Verification
            let cancel_res = push_reader_with_session(
                &mut session,
                sid,
                &root_test_dir_name,
                "cancelled.bin",
                1024,
                &mut std::io::Cursor::new(&[0u8; 1024]),
                None,
                MtpConflictPolicy::Overwrite,
                10,
                |_sent, _tot| {
                    Err(crate::error::AppCoreError::Transfer {
                        reason: crate::device::mtp::transfer::MTP_TRANSFER_CANCELLED_REASON
                            .to_string(),
                    })
                },
            );
            assert!(
                cancel_res.is_err(),
                "Cancelled transfer must yield error and clean up"
            );

            for file_h in created_files.into_iter().rev() {
                let _ = session.delete_object(file_h);
            }
            let _ = session.delete_object(subdir_h);
            let _ = session.delete_object(dir_h);
            let _ = session.close_session();
            drop(session);

            // 4. Disconnect & Reconnect Session Lifecycle Verification
            let mut session2 = crate::device::mtp::session::MtpSession::new(
                RusbTransport::open_verified(&dev, &desc, ep, &matched_info.serial_number)
                    .expect("reconnect open_verified must succeed without bus reset"),
            );
            session2
                .open_session(1)
                .expect("reconnect open_session(1) must succeed");
            let reconnected_sids = session2.get_storage_ids().expect("reconnected storage IDs");
            assert!(
                !reconnected_sids.is_empty(),
                "Reconnected session must access storage"
            );
            let _ = session2.close_session();

            executed = true;
            break;
        }
        assert!(executed, "Target MTP device session must be executed");
    }
}
