// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! MTP 1.1 and PTP standard operation, response, container, and object format codes.

/// PTP/MTP Container Types (Header bytes 4..6).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ContainerType {
    Undefined = 0x0000,
    Command = 0x0001,
    Data = 0x0002,
    Response = 0x0003,
    Event = 0x0004,
}

impl ContainerType {
    pub fn from_u16(val: u16) -> Self {
        match val {
            0x0001 => Self::Command,
            0x0002 => Self::Data,
            0x0003 => Self::Response,
            0x0004 => Self::Event,
            _ => Self::Undefined,
        }
    }
}

/// Standard MTP 1.1 Operation Codes.
pub mod op {
    pub const GET_DEVICE_INFO: u16 = 0x1001;
    pub const OPEN_SESSION: u16 = 0x1002;
    pub const CLOSE_SESSION: u16 = 0x1003;
    pub const GET_STORAGE_IDS: u16 = 0x1004;
    pub const GET_STORAGE_INFO: u16 = 0x1005;
    pub const GET_NUM_OBJECTS: u16 = 0x1006;
    pub const GET_OBJECT_HANDLES: u16 = 0x1007;
    pub const GET_OBJECT_INFO: u16 = 0x1008;
    pub const GET_OBJECT: u16 = 0x1009;
    pub const GET_THUMB: u16 = 0x100A;
    pub const DELETE_OBJECT: u16 = 0x100B;
    pub const SEND_OBJECT_INFO: u16 = 0x100C;
    pub const SEND_OBJECT: u16 = 0x100D;
    pub const INITIATE_CAPTURE: u16 = 0x100E;
    pub const FORMAT_STORE: u16 = 0x100F;
    pub const RESET_DEVICE: u16 = 0x1010;
    pub const SELF_TEST: u16 = 0x1011;
    pub const SET_OBJECT_PROTECTION: u16 = 0x1012;
    pub const POWER_DOWN: u16 = 0x1013;
    pub const GET_DEVICE_PROP_DESC: u16 = 0x1014;
    pub const GET_DEVICE_PROP_VALUE: u16 = 0x1015;
    pub const SET_DEVICE_PROP_VALUE: u16 = 0x1016;
    pub const RESET_DEVICE_PROP_VALUE: u16 = 0x1017;
    pub const TERMINATE_OPEN_CAPTURE: u16 = 0x1018;
    pub const MOVE_OBJECT: u16 = 0x1019;
    pub const COPY_OBJECT: u16 = 0x101A;
    pub const GET_PARTIAL_OBJECT: u16 = 0x101B;
    pub const INITIATE_OPEN_CAPTURE: u16 = 0x101C;
    // MTP 1.1 Enhanced Object and Property Operations
    pub const GET_OBJECT_PROPS_SUPPORTED: u16 = 0x9801;
    pub const GET_OBJECT_PROP_DESC: u16 = 0x9802;
    pub const GET_OBJECT_PROP_VALUE: u16 = 0x9803;
    pub const SET_OBJECT_PROP_VALUE: u16 = 0x9804;
    pub const GET_OBJECT_PROP_LIST: u16 = 0x9805;
    pub const SET_OBJECT_PROP_LIST: u16 = 0x9806;
    pub const GET_PARTIAL_OBJECT_64: u16 = 0x95C1;
    pub const SEND_PARTIAL_OBJECT: u16 = 0x95C2;
    pub const TRUNCATE_OBJECT: u16 = 0x95C3;
}

/// Standard PTP/MTP Event Codes (ISO 15740 §5.5.3 & USB-IF MTP 1.1).
pub mod event {
    pub const UNDEFINED: u16 = 0x4000;
    pub const CANCEL_TRANSACTION: u16 = 0x4001;
    pub const OBJECT_ADDED: u16 = 0x4002;
    pub const OBJECT_REMOVED: u16 = 0x4003;
    pub const STORE_ADDED: u16 = 0x4004;
    pub const STORE_REMOVED: u16 = 0x4005;
    pub const DEVICE_PROP_CHANGED: u16 = 0x4006;
    pub const OBJECT_INFO_CHANGED: u16 = 0x4007;
    pub const DEVICE_INFO_CHANGED: u16 = 0x4008;
    pub const REQUEST_OBJECT_TRANSFER: u16 = 0x4009;
    pub const STORE_FULL: u16 = 0x400A;
    pub const DEVICE_RESET: u16 = 0x400B;
    pub const STORAGE_INFO_CHANGED: u16 = 0x400C;
    pub const CAPTURE_COMPLETE: u16 = 0x400D;
    pub const UNREPORTED_STATUS: u16 = 0x400E;
}

/// Standard MTP 1.1 Object and Device Property Codes.
pub mod prop {
    pub const STORAGE_ID: u16 = 0xDC01;
    pub const OBJECT_FORMAT: u16 = 0xDC02;
    pub const PROTECTION_STATUS: u16 = 0xDC03;
    /// 64-bit unsigned integer (UINT64) representing file size in bytes for files > 4GB.
    pub const OBJECT_SIZE: u16 = 0xDC04;
    pub const ASSOCIATION_TYPE: u16 = 0xDC05;
    pub const ASSOCIATION_DESC: u16 = 0xDC06;
    pub const OBJECT_FILE_NAME: u16 = 0xDC07;
    pub const DATE_CREATED: u16 = 0xDC08;
    pub const DATE_MODIFIED: u16 = 0xDC09;
    pub const KEYWORDS: u16 = 0xDC0A;
    pub const PARENT_OBJECT: u16 = 0xDC0B;
    pub const PERSISTENT_UNIQUE_OBJECT_IDENTIFIER: u16 = 0xDC41;
    pub const SYNC_ID: u16 = 0xDC42;
    pub const NON_CONSUMABLE: u16 = 0xDC44;
}

/// Standard MTP 1.1 Response Codes.
pub mod resp {
    pub const OK: u16 = 0x2001;
    pub const GENERAL_ERROR: u16 = 0x2002;
    pub const SESSION_NOT_OPEN: u16 = 0x2003;
    pub const INVALID_TRANSACTION_ID: u16 = 0x2004;
    pub const OPERATION_NOT_SUPPORTED: u16 = 0x2005;
    pub const PARAMETER_NOT_SUPPORTED: u16 = 0x2006;
    pub const INCOMPLETE_TRANSFER: u16 = 0x2007;
    pub const INVALID_STORAGE_ID: u16 = 0x2008;
    pub const INVALID_OBJECT_HANDLE: u16 = 0x2009;
    pub const DEVICE_PROP_NOT_SUPPORTED: u16 = 0x200A;
    pub const INVALID_OBJECT_FORMAT_CODE: u16 = 0x200B;
    pub const STORE_FULL: u16 = 0x200C;
    pub const OBJECT_WRITE_PROTECTED: u16 = 0x200D;
    pub const STORE_READ_ONLY: u16 = 0x200E;
    pub const ACCESS_DENIED: u16 = 0x200F;
    pub const NO_THUMBNAIL_PRESENT: u16 = 0x2010;
    pub const SELF_TEST_FAILED: u16 = 0x2011;
    pub const PARTIAL_DELETION: u16 = 0x2012;
    pub const STORE_NOT_AVAILABLE: u16 = 0x2013;
    pub const SPECIFICATION_BY_FORMAT_UNSUPPORTED: u16 = 0x2014;
    pub const NO_VALID_OBJECT_INFO: u16 = 0x2015;
    pub const INVALID_CODE_FORMAT: u16 = 0x2016;
    pub const UNKNOWN_VENDOR_CODE: u16 = 0x2017;
    pub const CAPTURE_ALREADY_TERMINATED: u16 = 0x2018;
    pub const DEVICE_BUSY: u16 = 0x2019;
    pub const INVALID_PARENT_OBJECT: u16 = 0x201A;
    pub const INVALID_DEVICE_PROP_FORMAT: u16 = 0x201B;
    pub const INVALID_DEVICE_PROP_VALUE: u16 = 0x201C;
    pub const INVALID_PARAMETER: u16 = 0x201D;
    pub const SESSION_ALREADY_OPEN: u16 = 0x201E;
    pub const TRANSACTION_CANCELLED: u16 = 0x201F;
    pub const DESTINATION_UNSUPPORTED: u16 = 0x2020;
}

/// Standard MTP Object Format Codes.
pub mod format {
    pub const UNDEFINED: u16 = 0x3000;
    pub const ASSOCIATION: u16 = 0x3001; // Folder / Directory
    pub const SCRIPT: u16 = 0x3002;
    pub const EXECUTABLE: u16 = 0x3003;
    pub const TEXT: u16 = 0x3004;
    pub const HTML: u16 = 0x3005;
    pub const DPOF: u16 = 0x3006;
    pub const AIFF: u16 = 0x3007;
    pub const WAV: u16 = 0x3008;
    pub const MP3: u16 = 0x3009;
    pub const AVI: u16 = 0x300A;
    pub const MPEG: u16 = 0x300B;
    pub const ASF: u16 = 0x300C;
    pub const EXIF_JPEG: u16 = 0x3801;
    pub const TIFF_EP: u16 = 0x3802;
    pub const FLASHPIX: u16 = 0x3803;
    pub const BMP: u16 = 0x3804;
    pub const CIFF: u16 = 0x3805;
    pub const GIF: u16 = 0x3807;
    pub const JFIF: u16 = 0x3808;
    pub const PCD: u16 = 0x3809;
    pub const PNG: u16 = 0x380B;
    pub const TIFF: u16 = 0x380D;
    pub const TIFF_IT: u16 = 0x380E;
    pub const JP2: u16 = 0x380F;
    pub const JPX: u16 = 0x3810;
    pub const MP4: u16 = 0xB982;
    pub const THREE_GP: u16 = 0xB984; // USB-IF MTP 1.1 & AOSP MtpConstants FORMAT_3GP_CONTAINER
    pub const MATROSKA: u16 = 0xBA03; // Standard Matroska container format code
}

/// Helper returning human-readable string for MTP response code.
pub fn response_code_to_str(code: u16) -> &'static str {
    match code {
        resp::OK => "OK",
        resp::GENERAL_ERROR => "General Error",
        resp::SESSION_NOT_OPEN => "Session Not Open",
        resp::INVALID_TRANSACTION_ID => "Invalid Transaction ID",
        resp::OPERATION_NOT_SUPPORTED => "Operation Not Supported",
        resp::PARAMETER_NOT_SUPPORTED => "Parameter Not Supported",
        resp::INCOMPLETE_TRANSFER => "Incomplete Transfer",
        resp::INVALID_STORAGE_ID => "Invalid Storage ID",
        resp::INVALID_OBJECT_HANDLE => "Invalid Object Handle",
        resp::DEVICE_PROP_NOT_SUPPORTED => "Device Property Not Supported",
        resp::INVALID_OBJECT_FORMAT_CODE => "Invalid Object Format Code",
        resp::STORE_FULL => "Store Full",
        resp::OBJECT_WRITE_PROTECTED => "Object Write Protected",
        resp::STORE_READ_ONLY => "Store Read Only",
        resp::ACCESS_DENIED => "Access Denied",
        resp::NO_THUMBNAIL_PRESENT => "No Thumbnail Present",
        resp::SELF_TEST_FAILED => "Self Test Failed",
        resp::PARTIAL_DELETION => "Partial Deletion",
        resp::STORE_NOT_AVAILABLE => "Store Not Available",
        resp::SPECIFICATION_BY_FORMAT_UNSUPPORTED => "Specification By Format Unsupported",
        resp::NO_VALID_OBJECT_INFO => "No Valid Object Info",
        resp::INVALID_CODE_FORMAT => "Invalid Code Format",
        resp::UNKNOWN_VENDOR_CODE => "Unknown Vendor Code",
        resp::CAPTURE_ALREADY_TERMINATED => "Capture Already Terminated",
        resp::DEVICE_BUSY => "Device Busy",
        resp::INVALID_PARENT_OBJECT => "Invalid Parent Object",
        resp::INVALID_DEVICE_PROP_FORMAT => "Invalid Device Property Format",
        resp::INVALID_DEVICE_PROP_VALUE => "Invalid Device Property Value",
        resp::INVALID_PARAMETER => "Invalid Parameter",
        resp::SESSION_ALREADY_OPEN => "Session Already Open",
        resp::TRANSACTION_CANCELLED => "Transaction Cancelled",
        resp::DESTINATION_UNSUPPORTED => "Destination Unsupported",
        _ => "Unknown Response Code",
    }
}
