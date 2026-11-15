// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Deterministic protocol evidence for the actual GUI directory provider.

use super::budget::{DirectoryClock, DirectoryTransport};
use super::*;
use crate::device::mtp::{
    codes::{format, op, resp},
    container::encode_ptp_string,
    tests::MockUsbTransport,
    MtpContainer,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

const STORAGE: u32 = 0x10001;

struct Script {
    usb: MockUsbTransport,
    tid: u32,
}
impl Script {
    fn new() -> Self {
        Self {
            usb: MockUsbTransport::new(),
            tid: 0,
        }
    }
    fn data(&mut self, opcode: u16, bytes: &[u8]) {
        self.tid += 1;
        self.usb.push_data(opcode, self.tid, bytes);
        self.usb.push_response(resp::OK, self.tid, &[]);
    }
    fn array(&mut self, opcode: u16, values: &[u32]) {
        let mut bytes = (values.len() as u32).to_le_bytes().to_vec();
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.data(opcode, &bytes);
    }
    fn info(&mut self, name: &str, parent: u32, storage: u32, directory: bool) {
        let mut bytes = vec![0; 52];
        bytes[..4].copy_from_slice(&storage.to_le_bytes());
        bytes[4..6].copy_from_slice(
            &(if directory {
                format::ASSOCIATION
            } else {
                format::UNDEFINED
            })
            .to_le_bytes(),
        );
        bytes[8..12].copy_from_slice(&17u32.to_le_bytes());
        bytes[38..42].copy_from_slice(&parent.to_le_bytes());
        bytes.extend_from_slice(&encode_ptp_string(name));
        bytes.extend_from_slice(&encode_ptp_string(""));
        bytes.extend_from_slice(&encode_ptp_string(""));
        self.data(op::GET_OBJECT_INFO, &bytes);
    }
    fn session(self) -> MtpSession<MockUsbTransport> {
        MtpSession::new(self.usb)
    }
}

#[test]
fn root_returns_lossless_handles_and_only_confirmed_empty_is_empty() {
    let mut script = Script::new();
    script.array(op::GET_STORAGE_IDS, &[STORAGE]);
    script.array(op::GET_OBJECT_HANDLES, &[42]);
    script.info("Books", 0, STORAGE, true);
    let mut session = script.session();
    let entries = list_session_directory(&mut session, "/").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, "/Books");
    assert_eq!(entries[0].object_id, Some(42));
    assert!(entries[0].is_dir);
    let command = MtpContainer::parse(&session.transport().out_packets()[1]).unwrap();
    assert_eq!(command.extract_params(), vec![STORAGE, 0, u32::MAX]);
    let mut script = Script::new();
    script.array(op::GET_STORAGE_IDS, &[STORAGE]);
    script.array(op::GET_OBJECT_HANDLES, &[]);
    assert!(list_session_directory(&mut script.session(), "/")
        .unwrap()
        .is_empty());
}

#[test]
fn absolute_numeric_folder_is_a_name_and_nested_listing_uses_its_handle() {
    let mut script = Script::new();
    script.array(op::GET_STORAGE_IDS, &[STORAGE]);
    script.array(op::GET_OBJECT_HANDLES, &[42]);
    script.info("123", 0, STORAGE, true);
    script.array(op::GET_OBJECT_HANDLES, &[71]);
    script.info("book.epub", 42, STORAGE, false);
    let mut session = script.session();
    let entries = list_session_directory(&mut session, "/123").unwrap();
    assert_eq!(entries[0].path, "/123/book.epub");
    assert_eq!(entries[0].object_id, Some(71));
    let command = MtpContainer::parse(&session.transport().out_packets()[3]).unwrap();
    assert_eq!(command.extract_params(), vec![STORAGE, 0, 42]);
}

#[test]
fn explicit_handle_is_verified_before_listing() {
    let mut script = Script::new();
    script.array(op::GET_STORAGE_IDS, &[STORAGE]);
    script.info("Books", 0, STORAGE, true);
    script.array(op::GET_OBJECT_HANDLES, &[]);
    let mut session = script.session();
    assert!(list_session_directory(&mut session, "42")
        .unwrap()
        .is_empty());
    let command = MtpContainer::parse(&session.transport().out_packets()[1]).unwrap();
    assert_eq!(command.extract_params(), vec![42]);
}

#[test]
fn nonexistent_or_invalid_text_path_never_falls_back_to_root() {
    for path in ["/missing", "/../Books", "Books"] {
        let mut script = Script::new();
        script.array(op::GET_STORAGE_IDS, &[STORAGE]);
        if path == "/missing" {
            script.array(op::GET_OBJECT_HANDLES, &[]);
        }
        let mut session = script.session();
        assert!(
            list_session_directory(&mut session, path).is_err(),
            "{path}"
        );
    }
}

#[test]
fn zero_or_multiple_storages_are_explicit_errors() {
    for storages in [vec![], vec![STORAGE, STORAGE + 1]] {
        let mut script = Script::new();
        script.array(op::GET_STORAGE_IDS, &storages);
        let mut session = script.session();
        assert!(list_session_directory(&mut session, "/").is_err());
        assert_eq!(session.transport().out_packets().len(), 1);
    }
}

#[test]
fn metadata_failure_is_not_empty_or_partial_success_and_stops_iteration() {
    let mut script = Script::new();
    script.array(op::GET_STORAGE_IDS, &[STORAGE]);
    script.array(op::GET_OBJECT_HANDLES, &[42, 71]);
    script.usb.push_response(resp::ACCESS_DENIED, 3, &[]);
    let mut session = script.session();
    assert!(list_session_directory(&mut session, "/").is_err());
    assert_eq!(session.transport().out_packets().len(), 3);
}

#[test]
fn foreign_storage_parent_and_path_components_are_rejected() {
    for (name, parent, storage) in [
        ("book", 42, STORAGE),
        ("book", 0, STORAGE + 1),
        ("../book", 0, STORAGE),
    ] {
        let mut script = Script::new();
        script.array(op::GET_STORAGE_IDS, &[STORAGE]);
        script.array(op::GET_OBJECT_HANDLES, &[71]);
        script.info(name, parent, storage, false);
        assert!(list_session_directory(&mut script.session(), "/").is_err());
    }
}

#[test]
fn already_open_session_is_not_closed_or_taken_over() {
    let mut usb = MockUsbTransport::new();
    usb.push_response(resp::SESSION_ALREADY_OPEN, 1, &[]);
    let mut session = MtpSession::new(usb);
    assert!(session.open_directory_session(1).is_err());
    assert_eq!(session.transport().out_packets().len(), 1);
    assert_eq!(
        MtpContainer::parse(&session.transport().out_packets()[0])
            .unwrap()
            .code,
        op::OPEN_SESSION
    );
}

#[derive(Clone)]
struct Clock(Arc<AtomicU64>);
impl DirectoryClock for Clock {
    fn elapsed(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}
struct TimedTransport {
    clock: Clock,
    timeouts: Arc<Mutex<Vec<Duration>>>,
}
impl UsbBulkTransport for TimedTransport {
    fn write_bulk(&mut self, data: &[u8], timeout: Duration) -> Result<usize, AppCoreError> {
        self.timeouts.lock().unwrap().push(timeout);
        self.clock.0.fetch_add(2, Ordering::SeqCst);
        Ok(data.len())
    }
    fn read_bulk(&mut self, _: &mut [u8], timeout: Duration) -> Result<usize, AppCoreError> {
        self.write_bulk(&[0], timeout)
    }
    fn endpoint_in(&self) -> u8 {
        0x81
    }
    fn endpoint_out(&self) -> u8 {
        2
    }
}

#[test]
fn shared_budget_caps_each_io_and_exhaustion_prevents_further_io() {
    let clock = Clock(Arc::new(AtomicU64::new(0)));
    let timeouts = Arc::new(Mutex::new(Vec::new()));
    let inner = TimedTransport {
        clock: clock.clone(),
        timeouts: timeouts.clone(),
    };
    let mut transport = DirectoryTransport::with_clock(inner, Duration::from_millis(3), clock);
    assert_eq!(
        transport.write_bulk(&[1], Duration::from_secs(5)).unwrap(),
        1
    );
    assert!(transport
        .read_bulk(&mut [0], Duration::from_secs(5))
        .is_err());
    assert!(transport.write_bulk(&[1], Duration::from_secs(5)).is_err());
    assert_eq!(
        *timeouts.lock().unwrap(),
        vec![Duration::from_millis(3), Duration::from_millis(1)]
    );
}

#[test]
fn exhausted_budget_rejects_protocol_before_transport_side_effects() {
    let clock = Clock(Arc::new(AtomicU64::new(15)));
    let timeouts = Arc::new(Mutex::new(Vec::new()));
    let inner = TimedTransport {
        clock: clock.clone(),
        timeouts: timeouts.clone(),
    };
    let bounded = DirectoryTransport::with_clock(inner, Duration::from_millis(15), clock);
    let mut session = MtpSession::new(bounded);
    assert!(session.open_directory_session(1).is_err());
    assert!(timeouts.lock().unwrap().is_empty());
}
