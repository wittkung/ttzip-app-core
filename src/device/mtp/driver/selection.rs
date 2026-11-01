// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Unambiguous hardware device selection by serial or unique bus/model selector.

use super::{AppCoreError, DeviceErrorKind, MtpDeviceInfo};

fn select_default_device(devices: &[MtpDeviceInfo]) -> Result<&MtpDeviceInfo, AppCoreError> {
    if devices.len() == 1 {
        return Ok(&devices[0]);
    }
    let summary: Vec<String> = devices
        .iter()
        .map(|d| {
            format!(
                "{} ({:04X}:{:04X}@bus{}:addr{})",
                d.serial_number, d.vendor_id, d.product_id, d.bus_number, d.device_address
            )
        })
        .collect();
    Err(AppCoreError::InvalidState {
        reason: format!(
                "Ambiguous empty MTP device selector: {} devices connected [{}]; explicit serial number required",
                devices.len(),
                summary.join(", ")
        ),
    })
}

fn describe_devices(devices: &[&MtpDeviceInfo]) -> String {
    let summary: Vec<String> = devices
        .iter()
        .map(|d| {
            format!(
                "{} ({:04X}:{:04X}@bus{}:addr{})",
                d.serial_number, d.vendor_id, d.product_id, d.bus_number, d.device_address
            )
        })
        .collect();
    summary.join(", ")
}

/// Selects a single unambiguous `MtpDeviceInfo` from a discovered device list.
///
/// Rejects ambiguous selectors (such as `VID:PID` or empty selector when two devices of the same
/// model with different serial numbers are connected) instead of picking the first match.
pub fn select_unique_mtp_device<'a>(
    devices: &'a [MtpDeviceInfo],
    target_selector: &str,
) -> Result<&'a MtpDeviceInfo, AppCoreError> {
    let clean = target_selector
        .trim()
        .trim_start_matches("mtp://")
        .trim_start_matches("mtp:")
        .trim();

    if devices.is_empty() {
        return Err(AppCoreError::usb(
            DeviceErrorKind::Offline,
            format!(
                "No MTP devices available when resolving '{}'",
                target_selector
            ),
        ));
    }

    if clean.is_empty() {
        return select_default_device(devices);
    }

    // 1. Check exact hardware serial_number match first.
    let exact_serial_matches: Vec<&'a MtpDeviceInfo> = devices
        .iter()
        .filter(|d| {
            let vid_pid = format!("{:04X}:{:04X}", d.vendor_id, d.product_id);
            d.serial_number == clean && !clean.eq_ignore_ascii_case(&vid_pid)
        })
        .collect();

    if exact_serial_matches.len() == 1 {
        return Ok(exact_serial_matches[0]);
    }
    if exact_serial_matches.len() > 1 {
        return Err(AppCoreError::InvalidState {
            reason: format!(
                "Ambiguous MTP serial selector '{}': {} connected devices report the same serial",
                target_selector,
                exact_serial_matches.len()
            ),
        });
    }

    // 2. Fallback to bus:addr or VID:PID matching, which MUST be unique across all connected devices.
    let fallback_matches: Vec<&'a MtpDeviceInfo> = devices
        .iter()
        .filter(|d| {
            let vid_pid = format!("{:04X}:{:04X}", d.vendor_id, d.product_id);
            let bus_addr = format!("bus{:03}:addr{:03}", d.bus_number, d.device_address);
            d.serial_number.eq_ignore_ascii_case(clean)
                || vid_pid.eq_ignore_ascii_case(clean)
                || bus_addr.eq_ignore_ascii_case(clean)
        })
        .collect();

    match fallback_matches.len() {
        0 => Err(AppCoreError::usb(
            DeviceErrorKind::Offline,
            format!(
                "MTP hardware device '{}' not found among connected devices",
                target_selector
            ),
        )),
        1 => Ok(fallback_matches[0]),
        count => {
            let summary = describe_devices(&fallback_matches);
            Err(AppCoreError::InvalidState {
                reason: format!(
                    "Ambiguous MTP device selector '{}': matched {} same-model devices [{}]; specify unique hardware serial number",
                    target_selector,
                    count,
                    summary
                ),
            })
        }
    }
}
