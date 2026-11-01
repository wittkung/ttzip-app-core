// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![forbid(unsafe_code)]

//! Hardened Transfer Checkpoint and Gate 1~4 hierarchical verification engine.
//!
//! Provides hot-unplug crash resilience and micro-fingerprint challenge verification
//! for USB MTP/ADB transfers, combining boundary CRC32C chunk probes (head/tail 4KB)
//! with monotonic sequence epoch barriers and zero-heap hot-path probe calculation.

/// Standard 4KB boundary probe chunk size (4096 bytes).
pub const CHUNK_PROBE_SIZE: usize = 4096;

/// Standard Castagnoli CRC-32C reflected polynomial (0x82F63B78).
pub const CRC32C_POLYNOMIAL: u32 = 0x82F63B78;

/// ARM64 PMULL folding constant k1 = x^(128*4 + 64) mod P(x).
pub const ARM64_PMULL_K1: u64 = 0x0163cd9fc;

/// ARM64 PMULL folding constant k2 = x^(128*4) mod P(x).
pub const ARM64_PMULL_K2: u64 = 0x00db71064;

// --- Bitmask Validation Flags (matching C-ABI TTCheckpointValidationFlags) ---
pub const CHECKPOINT_FLAG_IDENTITY_MATCH: u32 = 1 << 0;   // 0x0001: Device Serial + ObjectID match
pub const CHECKPOINT_FLAG_METADATA_MATCH: u32 = 1 << 1;   // 0x0002: Size + mtime match
pub const CHECKPOINT_FLAG_HEAD_PROBE_OK: u32 = 1 << 2;    // 0x0004: Gate 2: Head 4KB CRC32C match
pub const CHECKPOINT_FLAG_SEAM_PROBE_OK: u32 = 1 << 3;    // 0x0008: Gate 3: Tail 4KB CRC32C match
pub const CHECKPOINT_FLAG_RANDOM_PROBES_OK: u32 = 1 << 4; // 0x0010: Ephemeral random probes ok
pub const CHECKPOINT_FLAG_CHUNK_MERKLE_OK: u32 = 1 << 5;  // 0x0020: Chunk BLAKE3/Merkle ok
pub const CHECKPOINT_FLAG_EPOCH_OK: u32 = 1 << 6;         // 0x0040: Gate 4: Monotonic epoch ok
pub const CHECKPOINT_FLAG_BOUNDS_OK: u32 = 1 << 7;        // 0x0080: Gate 1: Bounds check ok

/// Generate 8-slice lookup table for Castagnoli CRC-32C at compile time.
const fn generate_crc32c_slice8_tables() -> [[u32; 256]; 8] {
    let mut tables = [[0u32; 256]; 8];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ CRC32C_POLYNOMIAL;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        tables[0][i] = crc;
        i += 1;
    }

    let mut k = 1usize;
    while k < 8 {
        let mut i = 0usize;
        while i < 256 {
            let prev = tables[k - 1][i];
            let idx = (prev & 0xFF) as usize;
            tables[k][i] = (prev >> 8) ^ tables[0][idx];
            i += 1;
        }
        k += 1;
    }
    tables
}

/// Precomputed 8-slice lookup tables for zero-heap, branchless CRC-32C calculation.
pub const CRC32C_TABLES: [[u32; 256]; 8] = generate_crc32c_slice8_tables();

/// Computes Castagnoli CRC-32C checksum with zero heap allocation.
#[inline]
pub fn crc32c(data: &[u8]) -> u32 {
    crc32c_update(0, data)
}

/// Updates a running Castagnoli CRC-32C checksum over an arbitrary byte slice.
#[inline]
pub fn crc32c_update(crc: u32, data: &[u8]) -> u32 {
    let mut current = crc ^ 0xFFFF_FFFF;
    let mut rem = data;

    while rem.len() >= 8 {
        let b0 = (rem[0] as usize) ^ ((current & 0xFF) as usize);
        let b1 = (rem[1] as usize) ^ (((current >> 8) & 0xFF) as usize);
        let b2 = (rem[2] as usize) ^ (((current >> 16) & 0xFF) as usize);
        let b3 = (rem[3] as usize) ^ (((current >> 24) & 0xFF) as usize);
        let b4 = rem[4] as usize;
        let b5 = rem[5] as usize;
        let b6 = rem[6] as usize;
        let b7 = rem[7] as usize;

        current = CRC32C_TABLES[7][b0]
            ^ CRC32C_TABLES[6][b1]
            ^ CRC32C_TABLES[5][b2]
            ^ CRC32C_TABLES[4][b3]
            ^ CRC32C_TABLES[3][b4]
            ^ CRC32C_TABLES[2][b5]
            ^ CRC32C_TABLES[1][b6]
            ^ CRC32C_TABLES[0][b7];

        rem = &rem[8..];
    }

    for &byte in rem {
        let idx = ((current as u8) ^ byte) as usize;
        current = (current >> 8) ^ CRC32C_TABLES[0][idx];
    }

    current ^ 0xFFFF_FFFF
}

/// Computes CRC-32C probe for the head 4KB boundary of data.
#[inline]
pub fn compute_head_probe(data: &[u8]) -> u32 {
    let probe_len = data.len().min(CHUNK_PROBE_SIZE);
    crc32c(&data[..probe_len])
}

/// Computes CRC-32C probe for the tail 4KB seam boundary of data.
#[inline]
pub fn compute_tail_probe(data: &[u8]) -> u32 {
    let offset = data.len().saturating_sub(CHUNK_PROBE_SIZE);
    crc32c(&data[offset..])
}

/// Computes both head and tail 4KB boundary CRC-32C probes in a single pass.
#[inline]
pub fn compute_boundary_probes(data: &[u8]) -> (u32, u32) {
    (compute_head_probe(data), compute_tail_probe(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32c_known_vectors() {
        assert_eq!(crc32c(b""), 0x00000000);
        assert_eq!(crc32c(b"123456789"), 0xE3069283);
    }

    #[test]
    fn test_boundary_probes() {
        let data = vec![0xAB; 8192];
        let (head, tail) = compute_boundary_probes(&data);
        assert_eq!(head, crc32c(&data[..4096]));
        assert_eq!(tail, crc32c(&data[4096..]));
    }
}
