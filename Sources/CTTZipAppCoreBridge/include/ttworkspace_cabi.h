// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#ifndef TTWORKSPACE_CABI_H
#define TTWORKSPACE_CABI_H

#include <stdint.h>
#include <stddef.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

// --- Orthogonal Node Flags (Pure State Bits) ---
#define TTWORKSPACE_FLAG_ENCRYPTED       ((uint16_t)(1 << 0)) // 0x0001
#define TTWORKSPACE_FLAG_PINNED          ((uint16_t)(1 << 1)) // 0x0002
#define TTWORKSPACE_FLAG_HIDDEN          ((uint16_t)(1 << 2)) // 0x0004
#define TTWORKSPACE_FLAG_SECURITY_SCOPED ((uint16_t)(1 << 3)) // 0x0008
#define TTWORKSPACE_FLAG_READ_ONLY       ((uint16_t)(1 << 4)) // 0x0010: Read-only permissions
#define TTWORKSPACE_FLAG_INLINE_PAYLOAD  ((uint16_t)(1 << 5)) // 0x0020: Stored inline in StringArena
#define TTWORKSPACE_FLAG_STREAMING       ((uint16_t)(1 << 6)) // 0x0040: Transferred from Android stream
#define TTWORKSPACE_FLAG_NON_STANDARD_NAME ((uint16_t)(1 << 7)) // 0x0080: Non-UTF8 or preserved escape bytes

/** Bits 14-15: aux union discriminator (aux_mode). */
#define TTWORKSPACE_AUX_MODE_SHIFT      14
#define TTWORKSPACE_AUX_MODE_MASK       (0x3u << TTWORKSPACE_AUX_MODE_SHIFT)
#define TTWORKSPACE_AUX_MODE_NONE       (0x0u << TTWORKSPACE_AUX_MODE_SHIFT)  /* aux is zeroed / unused           */
#define TTWORKSPACE_AUX_MODE_COMPRESSED (0x1u << TTWORKSPACE_AUX_MODE_SHIFT)  /* aux.compressed_size is valid      */
#define TTWORKSPACE_AUX_MODE_DEVICE     (0x2u << TTWORKSPACE_AUX_MODE_SHIFT)  /* aux.device_object_id is valid     */

// --- POSIX File Type Constants (Low 16 Bits of file_type) ---
#define TTWORKSPACE_POSIX_MODE_MASK      0x0000FFFFU
#define TTWORKSPACE_POSIX_TYPE_MASK      0x0000F000U
#define TTWORKSPACE_POSIX_PERM_MASK      0x00000FFFU

#define TTWORKSPACE_S_IFIFO              0x00001000U // Named pipe
#define TTWORKSPACE_S_IFCHR              0x00002000U // Character device
#define TTWORKSPACE_S_IFDIR              0x00004000U // Directory
#define TTWORKSPACE_S_IFBLK              0x00006000U // Block device
#define TTWORKSPACE_S_IFREG              0x00008000U // Regular file
#define TTWORKSPACE_S_IFLNK              0x0000A000U // Symbolic link
#define TTWORKSPACE_S_IFSOCK             0x0000C000U // Unix domain socket
#define TTWORKSPACE_S_IFWHT              0x0000E000U // BSD Whiteout

// --- Fast Format Class Enumeration (High 16 Bits of file_type) ---
typedef enum {
    TTWORKSPACE_FORMAT_GENERIC_BINARY  = 0,
    TTWORKSPACE_FORMAT_DIRECTORY       = 1,
    TTWORKSPACE_FORMAT_TEXT_PLAIN      = 2,
    TTWORKSPACE_FORMAT_MEDIA_VIDEO     = 3,
    TTWORKSPACE_FORMAT_MEDIA_AUDIO     = 4,
    TTWORKSPACE_FORMAT_IMAGE_RASTER    = 5,
    TTWORKSPACE_FORMAT_IMAGE_VECTOR    = 6,
    TTWORKSPACE_FORMAT_ARCHIVE_ZIP     = 7,
    TTWORKSPACE_FORMAT_ARCHIVE_SEVEN_Z = 8,
    TTWORKSPACE_FORMAT_ARCHIVE_TAR     = 9,
    TTWORKSPACE_FORMAT_DOCUMENT_PDF    = 10,
#define TT_MAKE_FOURCC(a, b, c, d) \
    (((uint32_t)(uint8_t)(a)) | (((uint32_t)(uint8_t)(b)) << 8) | \
     (((uint32_t)(uint8_t)(c)) << 16) | (((uint32_t)(uint8_t)(d)) << 24))
#define TT_SNAPSHOT_MAGIC                TT_MAKE_FOURCC('T', 'T', 'V', 'S') // 0x53565454U ("TTVS" in Little-Endian)
#define TT_RESUME_MAGIC                  TT_MAKE_FOURCC('T', 'T', 'R', 'E') // 0x45525454U ("TTRE" in Little-Endian)
// --- HDR & Presentation Pixel Format FourCC Constants ---
#define TT_FOURCC_RGhA                   TT_MAKE_FOURCC('R', 'G', 'h', 'A') // 0x41684752U: kCVPixelFormatType_64RGBAHalf (16-bit float)
#define TT_FOURCC_l10r                   TT_MAKE_FOURCC('l', '1', '0', 'r') // 0x7230316cU: kCVPixelFormatType_30RGBLEPackedWideGamut (10-bit LE)
#define TT_FOURCC_R10k                   TT_MAKE_FOURCC('R', '1', '0', 'k') // 0x6b303152U: kCVPixelFormatType_30RGB (10-bit BE)
#define TT_FOURCC_BGRA                   TT_MAKE_FOURCC('B', 'G', 'R', 'A') // 0x41524742U: kCVPixelFormatType_32BGRA
    TTWORKSPACE_FORMAT_SOURCE_CODE     = 11,
    TTWORKSPACE_FORMAT_DATABASE_SQLITE = 12
} TTWorkspaceFormatClass;

#define TTWORKSPACE_NODE_NONE            0xFFFFFFFFU
#define TTWORKSPACE_STABLE_ID_NONE       0xFFFFFFFFFFFFFFFFULL // UINT64_MAX: Unmodified parent stable_id sentinel
#define TT_DELTA_NO_CHANGE               0xFFFFFFFFU           // Sentinel for name_offset indicating unmodified filename

// --- 1. Memory-Mapped POD Node (64-byte microarchitecture aligned) ---
typedef struct __attribute__((aligned(64))) {
    uint32_t parent_id;          // 0..4:   Parent index (0xFFFFFFFF = root)
    uint32_t first_child_id;     // 4..8:   CSR first child index (0xFFFFFFFF = None)
    uint32_t child_count;        // 8..12:  CSR direct children count in contiguous slice
    uint32_t name_offset;        // 12..16: UTF-8 Base Name offset in StringArena
    uint64_t sort_prefix;        // 16..24: Big-Endian leading 8 bytes of UTF-8 filename (zero-padded)
    uint64_t uncompressed_size;  // 24..32: Original size in bytes (0 for S_IFDIR)
    int64_t  mtime_epoch_s;      // 32..40: Epoch seconds Unix timestamp (ISO 15740 compliant)
    union {
        uint64_t compressed_size;  // Transfer/Archive size in bytes (Archive mode)
        uint64_t device_object_id; // [32..63] StorageID, [0..31] ObjectID / MTP hardware handle
        uint64_t aux_payload;      // Generic 64-bit extension slot
    } aux;                       // 40..48: Mutually exclusive auxiliary payload
    uint64_t stable_id;          // 48..56: Cross-generation stable entry ID (monotonic counter)
    uint32_t file_type;          // 56..60: [0..15] POSIX st_mode, [16..31] TTWorkspaceFormatClass
    uint16_t flags;              // 60..62: Pure filesystem physical flags
    uint16_t name_len;           // 62..64: Base Name length in bytes
} TTWorkspaceEntryNode;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTWorkspaceEntryNode) == 64, "TTWorkspaceEntryNode must be exactly 64 bytes");
_Static_assert(_Alignof(TTWorkspaceEntryNode) == 64, "TTWorkspaceEntryNode must be 64-byte aligned");
#endif

// --- 1.1 Compact POD Node (24-byte, 8-byte aligned) ---
typedef struct __attribute__((aligned(8))) {
    uint32_t name_offset;
    uint16_t name_len;
    uint16_t flags;
    uint64_t size;
    uint32_t mtime;
    uint32_t dir_children_idx;
} TTWorkspaceCompactNode;
#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTWorkspaceCompactNode) == 24, "TTWorkspaceCompactNode must be 24 bytes");
_Static_assert(_Alignof(TTWorkspaceCompactNode) == 8, "TTWorkspaceCompactNode alignment must be 8");
#endif

// Fast branchless comparison of 8-byte big-endian sort prefixes
static inline int32_t ttworkspace_compare_sort_prefix(uint64_t a_prefix, uint64_t b_prefix) {
    return (int32_t)(a_prefix > b_prefix) - (int32_t)(a_prefix < b_prefix);
}

static inline uint64_t ttworkspace_make_sort_prefix(const uint8_t *name, uint16_t len) {
    if (__builtin_expect(len == 0 || name == (const uint8_t *)0, 0)) return 0ULL;
    if (__builtin_expect(len >= 8, 1)) {
        uint64_t raw;
        __builtin_memcpy(&raw, name, sizeof(raw));
        return __builtin_bswap64(raw);
    }
    uint64_t prefix = 0;
    for (int i = 0; i < len; ++i) prefix = (prefix << 8) | (uint64_t)name[i];
    return prefix << (8 * (8 - len));
}

/**
 * Find a child node by name within a parent's CSR child slice.
 *
 * Performs binary search on sort_prefix for O(log K) average case,
 * with mandatory memcmp fallback in StringArena when prefixes collide.
 *
 * @param children   Pointer to the first child node in the CSR slice.
 * @param count      Number of children (parent->child_count).
 * @param arena      Pointer to the snapshot's StringArena base.
 * @param arena_len  Total byte length of the StringArena.
 * @param name       The target file name to search for.
 * @param name_len   Byte length of the target name.
 * @return           Index into children[] if found, or (uint32_t)-1 if not found.
 */
static inline uint32_t
ttworkspace_find_child_by_name(
    const TTWorkspaceEntryNode *children,
    uint32_t                    count,
    const char                 *arena,
    size_t                      arena_len,
    const char                 *name,
    uint16_t                    name_len)
{
    const uint64_t target_prefix = ttworkspace_make_sort_prefix(
        (const uint8_t *)name, name_len);
    uint32_t lo = 0, hi = count;
    while (lo < hi) {
        const uint32_t mid = lo + (hi - lo) / 2;
        const int cmp = ttworkspace_compare_sort_prefix(
            target_prefix, children[mid].sort_prefix);
        if (cmp < 0) {
            hi = mid;
        } else if (cmp > 0) {
            lo = mid + 1;
        } else {
            /* Prefix collision: fall back to full StringArena memcmp. */
            if ((uint64_t)children[mid].name_offset + children[mid].name_len > (uint64_t)arena_len) return (uint32_t)-1;
            const char *mid_name = arena + children[mid].name_offset;
            const uint16_t mid_len = children[mid].name_len;
            const uint16_t min_len = name_len < mid_len ? name_len : mid_len;
            const int full_cmp = memcmp(name, mid_name, min_len);
            if (full_cmp < 0 || (full_cmp == 0 && name_len < mid_len)) {
                hi = mid;
            } else if (full_cmp > 0 || (full_cmp == 0 && name_len > mid_len)) {
                lo = mid + 1;
            } else {
                return mid;  /* Exact match. */
            }
        }
    }
    return (uint32_t)-1;  /* Not found. */
}

static inline uint8_t ttworkspace_node_is_dir(const TTWorkspaceEntryNode *node) {
    return ((node->file_type & TTWORKSPACE_POSIX_TYPE_MASK) == TTWORKSPACE_S_IFDIR) ? 1 : 0;
}

static inline uint8_t ttworkspace_node_is_file(const TTWorkspaceEntryNode *node) {
    return ((node->file_type & TTWORKSPACE_POSIX_TYPE_MASK) == TTWORKSPACE_S_IFREG) ? 1 : 0;
}

static inline uint8_t ttworkspace_node_is_symlink(const TTWorkspaceEntryNode *node) {
    return ((node->file_type & TTWORKSPACE_POSIX_TYPE_MASK) == TTWORKSPACE_S_IFLNK) ? 1 : 0;
}

static inline uint16_t ttworkspace_node_format_class(const TTWorkspaceEntryNode *node) {
    return (uint16_t)(node->file_type >> 16);
}

// --- 2. UniVFS Section Header Table & Self-Describing Snapshot Header ---

// Extensible Section Type Enum (Self-describing layout)
typedef enum {
    TT_SECTION_TYPE_UNUSED            = 0,
    TT_SECTION_TYPE_NODES             = 1, // Node records: 64B TTWorkspaceEntryNode[] or 24B TTWorkspaceCompactNode[] per feature_flags
    TT_SECTION_TYPE_STRING_ARENA      = 2, // Contiguous UTF-8 String Arena
    TT_SECTION_TYPE_STABLE_ID_KEYS    = 3, // Sorted uint64_t stable_id array
    TT_SECTION_TYPE_STABLE_ID_INDICES = 4, // uint32_t node_index array
    TT_SECTION_TYPE_DEVICE_ID_KEYS    = 5, // Sorted uint64_t device_object_id array
    TT_SECTION_TYPE_DEVICE_ID_INDICES = 6, // uint32_t device_node_index array
    TT_SECTION_TYPE_CUSTOM_METADATA   = 7, // Extensible custom metadata payload
    TT_SECTION_TYPE_NAME_MASKS        = 8, // Contiguous uint64_t Bloom filter name mask array
    TT_SECTION_TYPE_SUBTREE_END_IDS   = 9  // Contiguous uint32_t DFS subtree end node IDs (also used as TT_SECTION_TYPE_EMBEDDINGS in ttzip-types)
} TTWorkspaceSectionType;

#define TT_SECTION_FLAG_ALIGNED_64 (1U << 0) // Section payload requires 64-byte alignment
#define TT_SECTION_FLAG_READ_ONLY  (1U << 1) // Section payload is write-protected
#define TT_SECTION_FLAG_COMPRESSED (1U << 2) // Section payload is compressed
#define TT_SECTION_FLAG_ENCRYPTED  (1U << 3) // Section payload is encrypted
#define TT_SECTION_FLAG_MACH_SHARED (1U << 4) // Section payload is backed by shared Mach memory

#define TT_FEATURE_FLAG_CSR        (1U << 0) // Flat 64-byte CSR topology enabled
#define TT_FEATURE_FLAG_LEXSORT    (1U << 1) // Lexicographically sorted sibling nodes
#define TT_FEATURE_FLAG_DFS_BLOCK  (1U << 2) // Pre-order DFS Block Layout subtree closure invariant
#define TT_FEATURE_FLAG_NAME_MASK  (1U << 3) // 64-bit character Bloom mask acceleration column
// Note: Bit 4 (1U << 4) is used by Rust (TT_FEATURE_FLAG_COMPACT_NODES) and Swift
// (TTWorkspaceVfsView.featureFlagCompactNodes) to mark 24-byte TTWorkspaceCompactNode payloads
// in TT_SECTION_TYPE_NODES; 64-byte CSR nodes are active only when TT_FEATURE_FLAG_CSR (1U << 0)
// is set and bit 4 is clear.

// 32-byte Self-Describing Section Entry (8-byte aligned)
typedef struct __attribute__((aligned(8))) {
    uint32_t section_type;  // 0..4:   TTWorkspaceSectionType (0..9)
    uint32_t flags;         // 4..8:   TT_SECTION_FLAG_* bitmask
    uint64_t offset;        // 8..16:  Byte offset from start of snapshot mapping (8-byte aligned)
    uint64_t size_bytes;    // 16..24: Total byte size of section payload
    uint64_t entry_count;   // 24..32: Logical item count in section (e.g. node_count)
} TTWorkspaceSectionEntry;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTWorkspaceSectionEntry) == 32, "TTWorkspaceSectionEntry must be exactly 32 bytes");
_Static_assert(_Alignof(TTWorkspaceSectionEntry) == 8, "TTWorkspaceSectionEntry must be 8-byte aligned");
#endif

// 128-byte Self-Describing Snapshot Header (64-byte aligned, generous 56-byte reservation)
typedef struct __attribute__((aligned(64))) {
    uint32_t magic;                    // 0..4:   0x53565454 "TTVS" (Little-Endian)
    uint16_t abi_major;                // 4..6:   0x0006 (v6.0 Section Header Table)
    uint16_t abi_minor;                // 6..8:   0x0000
    uint32_t header_size;              // 8..12:  Snapshot header size in bytes (128)
    uint32_t section_header_size;      // 12..16: Size of each section header entry (32)
    uint32_t section_count;            // 16..20: Number of active sections in section table
    uint32_t root_node_id;             // 20..24: Root node index (strictly 0)
    uint64_t total_mapping_size;       // 24..32: Total mapped Mach VM bytes. MUST be a multiple of 16384 (Apple Silicon VM page size).
    uint64_t generation;               // 32..40: Monotonic snapshot generation (RCU)
    uint64_t snapshot_sequence;        // 40..48: Monotonic snapshot sequence counter
    uint64_t section_headers_offset;   // 48..56: Byte offset to TTWorkspaceSectionEntry[] (8B alignment checked; 128B default)
    uint32_t feature_flags;            // 56..60: Feature flags (bits 0..3: TT_FEATURE_FLAG_*; bit 4: compact 24B nodes in ttzip-engine)
    uint32_t reserved_flags;           // 60..64: Reserved for future feature expansion
    uint64_t snapshot_checksum;        // 64..72: Reserved zero (preserves 128B size; zero-TOCTOU guaranteed by Mach COW)
    uint8_t  _reserved[56];            // 72..128: Generous 56-byte reservation
} TTWorkspaceSnapshotHeader;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTWorkspaceSnapshotHeader) == 128, "TTWorkspaceSnapshotHeader must be exactly 128 bytes");
_Static_assert(_Alignof(TTWorkspaceSnapshotHeader) == 64, "TTWorkspaceSnapshotHeader must be 64-byte aligned");
_Static_assert(
    sizeof(TTWorkspaceSnapshotHeader) % 16384 == 0 ||
    sizeof(TTWorkspaceSnapshotHeader) <= 16384,
    "TTWorkspaceSnapshotHeader must fit within a single VM page or be page-aligned");
#endif

// Section Lookup Helper
static inline const TTWorkspaceSectionEntry *ttworkspace_find_section(
    const TTWorkspaceSnapshotHeader *hdr,
    const void *mapping_base,
    uint32_t section_type
) {
    if (!hdr || !mapping_base || hdr->section_count == 0 || hdr->section_headers_offset == 0) return 0;
    if ((hdr->section_headers_offset & 7) != 0) return 0;
    const uint64_t table_size = (uint64_t)hdr->section_count * sizeof(TTWorkspaceSectionEntry);
    if (hdr->section_headers_offset + table_size > hdr->total_mapping_size) return 0;

    const TTWorkspaceSectionEntry *sections = (const TTWorkspaceSectionEntry *)(
        (const uint8_t *)mapping_base + hdr->section_headers_offset
    );
    for (uint32_t i = 0; i < hdr->section_count; ++i) {
        if (sections[i].section_type == section_type) {
            if ((sections[i].offset & 7) != 0) return 0;
            if (sections[i].offset + sections[i].size_bytes > hdr->total_mapping_size) return 0;
            return &sections[i];
        }
    }
    return 0;
}

// Binary search for node_index by stable_id (O(log N) over sorted keys_sec).
static inline uint32_t ttworkspace_find_node_by_stable_id(
    const TTWorkspaceSnapshotHeader *hdr,
    const void *mapping_base,
    uint64_t target_stable_id
) {
    if (!hdr || !mapping_base) return TTWORKSPACE_NODE_NONE;
    const TTWorkspaceSectionEntry *keys_sec = ttworkspace_find_section(hdr, mapping_base, TT_SECTION_TYPE_STABLE_ID_KEYS);
    const TTWorkspaceSectionEntry *indices_sec = ttworkspace_find_section(hdr, mapping_base, TT_SECTION_TYPE_STABLE_ID_INDICES);
    if (!keys_sec || !indices_sec || keys_sec->entry_count == 0 || keys_sec->entry_count != indices_sec->entry_count) {
        return TTWORKSPACE_NODE_NONE;
    }
    if ((keys_sec->offset & 7) != 0 || (indices_sec->offset & 7) != 0) {
        return TTWORKSPACE_NODE_NONE;
    }
    if (keys_sec->offset + keys_sec->size_bytes > hdr->total_mapping_size ||
        indices_sec->offset + indices_sec->size_bytes > hdr->total_mapping_size) {
        return TTWORKSPACE_NODE_NONE;
    }
    if (keys_sec->entry_count * sizeof(uint64_t) > keys_sec->size_bytes ||
        indices_sec->entry_count * sizeof(uint32_t) > indices_sec->size_bytes) {
        return TTWORKSPACE_NODE_NONE;
    }
    const uint64_t *keys = (const uint64_t *)((const uint8_t *)mapping_base + keys_sec->offset);
    const uint32_t *indices = (const uint32_t *)((const uint8_t *)mapping_base + indices_sec->offset);
    
    int64_t left = 0;
    int64_t right = (int64_t)keys_sec->entry_count - 1;
    while (left <= right) {
        int64_t mid = left + ((right - left) >> 1);
        uint64_t mid_id = keys[mid];
        if (mid_id == target_stable_id) {
            return indices[mid];
        } else if (mid_id < target_stable_id) {
            left = mid + 1;
        } else {
            right = mid - 1;
        }
    }
    return TTWORKSPACE_NODE_NONE;
}

// Binary search for node_index by MTP device_object_id (O(log M) over sorted keys_sec).
static inline uint32_t ttworkspace_find_node_by_device_object_id(
    const TTWorkspaceSnapshotHeader *hdr,
    const void *mapping_base,
    uint64_t target_device_object_id
) {
    if (!hdr || !mapping_base) return TTWORKSPACE_NODE_NONE;
    const TTWorkspaceSectionEntry *keys_sec = ttworkspace_find_section(hdr, mapping_base, TT_SECTION_TYPE_DEVICE_ID_KEYS);
    const TTWorkspaceSectionEntry *indices_sec = ttworkspace_find_section(hdr, mapping_base, TT_SECTION_TYPE_DEVICE_ID_INDICES);
    if (!keys_sec || !indices_sec || keys_sec->entry_count == 0 || keys_sec->entry_count != indices_sec->entry_count) {
        return TTWORKSPACE_NODE_NONE;
    }
    if ((keys_sec->offset & 7) != 0 || (indices_sec->offset & 7) != 0) {
        return TTWORKSPACE_NODE_NONE;
    }
    if (keys_sec->offset + keys_sec->size_bytes > hdr->total_mapping_size ||
        indices_sec->offset + indices_sec->size_bytes > hdr->total_mapping_size) {
        return TTWORKSPACE_NODE_NONE;
    }
    if (keys_sec->entry_count * sizeof(uint64_t) > keys_sec->size_bytes ||
        indices_sec->entry_count * sizeof(uint32_t) > indices_sec->size_bytes) {
        return TTWORKSPACE_NODE_NONE;
    }
    const uint64_t *keys = (const uint64_t *)((const uint8_t *)mapping_base + keys_sec->offset);
    const uint32_t *indices = (const uint32_t *)((const uint8_t *)mapping_base + indices_sec->offset);
    
    int64_t left = 0;
    int64_t right = (int64_t)keys_sec->entry_count - 1;
    while (left <= right) {
        int64_t mid = left + ((right - left) >> 1);
        uint64_t mid_id = keys[mid];
        if (mid_id == target_device_object_id) {
            return indices[mid];
        } else if (mid_id < target_device_object_id) {
            left = mid + 1;
        } else {
            right = mid - 1;
        }
    }
    return TTWORKSPACE_NODE_NONE;
}

/**
 * Snapshot topology validator declaration.
 *
 * Note: No Rust or C implementation currently exports `ttworkspace_validate_snapshot`;
 * calling this symbol will fail at link time. `ttvfs_acquire_snapshot` validates 64-byte
 * header alignment, `TTWorkspaceSnapshotHeader::validate` (`total_mapping_size <= lease->size`),
 * section table bounds, and the alignment, payload range (`offset + size_bytes <= total_mapping_size`),
 * and record capacity (`entry_count * stride <= size_bytes`) of the node and string-arena sections,
 * but does not perform full CSR tree-topology or per-node string-range validation.
 */
int32_t ttworkspace_validate_snapshot(
    const void *mapping_base,
    uint64_t mapped_bytes,
    void *err_out
);



// --- 2.26 Linearized Stream Chunk Protocol ---
typedef struct __attribute__((aligned(8))) {
    uint64_t stream_id;       // Unique media stream session ID
    uint64_t stream_epoch;    // Monotonic seek epoch (Seek increments epoch)
    uint64_t request_id;      // Sequential chunk request ID
    uint64_t logical_offset;  // Absolute byte offset in remote file
    uint32_t payload_len;     // Payload bytes in chunk
    uint32_t flags;           // bit 0: EOF, bit 1: Stale/Cancelled, bit 2: Discontinuity
} TTStreamChunkHeader;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTStreamChunkHeader) == 40, "TTStreamChunkHeader must be exactly 40 bytes");
#endif

// --- 2.3 Authoritative Checkpoint Verification Bitmask Flags ---
typedef enum {
    TT_CHECKPOINT_FLAG_IDENTITY_MATCH   = (1 << 0), // 0x0001: Device Serial + ObjectID match
    TT_CHECKPOINT_FLAG_METADATA_MATCH   = (1 << 1), // 0x0002: Size + Epoch-Second mtime match
    TT_CHECKPOINT_FLAG_HEAD_PROBE_OK    = (1 << 2), // 0x0004: Head 4KB direct memcmp passed
    TT_CHECKPOINT_FLAG_SEAM_PROBE_OK    = (1 << 3), // 0x0008: Seam boundary 4KB direct memcmp passed
    TT_CHECKPOINT_FLAG_RANDOM_PROBES_OK = (1 << 4), // 0x0010: Ephemeral random probes passed
    TT_CHECKPOINT_FLAG_CHUNK_MERKLE_OK  = (1 << 5)  // 0x0020: Chunk BLAKE3/Merkle verified
} TTCheckpointValidationFlags;

// --- 2.27 Persistent Multi-Session Transfer Resume Identity (64-byte explicit layout, little-endian) ---
#define TT_RESUME_FORMAT_VERSION_1 1U

#define TT_RESUME_FLAG_REMOVABLE_STORAGE (1U << 0) // 0x0001: Target is on removable medium (SD Card / USB)
#define TT_RESUME_FLAG_CASE_INSENSITIVE   (1U << 1) // 0x0002: Volume filesystem is case-insensitive (APFS / exFAT)
#define TT_RESUME_FLAG_WEAK_IDENTITY     (1U << 2) // 0x0004: Identity derived from fallback storage_id_hint; forces Head & Seam challenge
#define TT_IDENTITY_FLAG_WEAK            TT_RESUME_FLAG_WEAK_IDENTITY

typedef struct {
    uint64_t device_serial_hash;         // 0..8:   Hardware serial xxHash3-64
    uint64_t parent_relative_path_hash;  // 8..16:  Normalized parent relative path xxHash3-64
    uint64_t file_name_hash;             // 16..24: Normalized filename xxHash3-64
    uint64_t uncompressed_size;          // 24..32: Authoritative physical file size in bytes
    int64_t  mtime_epoch_s;              // 32..40: POSIX epoch seconds timestamp
    uint64_t volume_id_hash;             // 40..48: Volume UUID / VolumeIdentifier xxHash3-64 (SD card re-identification)
    uint32_t storage_id_hint;            // 48..52: MTP storage ID session hint (degraded to cache hint for hot-swap healing)
    uint32_t flags;                      // 52..56: Bitmask (bit 0: IS_REMOVABLE_STORAGE, bit 1: CASE_INSENSITIVE)
    uint16_t vendor_id;                  // 56..58: USB Vendor ID (VID)
    uint16_t product_id;                 // 58..60: USB Product ID (PID)
    uint32_t _reserved;                  // 60..64: Explicit zero padding (prevents uninitialized tail bytes)
} TTTransferResumeIdentity;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTTransferResumeIdentity) == 64, "TTTransferResumeIdentity must be exactly 64 bytes");
#endif

// --- 2.28 Persistent Disk Checkpoint & WAL Header Envelope (64-byte explicit layout, little-endian) ---
typedef struct {
    uint32_t magic;                      // 0..4:   0x45525454 ("TTRE" in Little-Endian)
    uint32_t version;                    // 4..8:   Protocol version (1)
    uint32_t header_size;                // 8..12:  Header size (64)
    uint32_t header_crc32c;              // 12..16: CRC32C over offset 16..63
    uint64_t resumed_bytes;              // 16..24: Verified contiguous persisted bytes
    uint64_t wal_sequence_num;           // 24..32: Monotonically increasing WAL transaction sequence
    uint8_t  _reserved[28];              // 32..60: Explicit zeroed padding
    uint32_t payload_crc32c;             // 60..64: CRC32C over following TTTransferResumeIdentity payload (64B)
} TTTransferResumeHeader;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTTransferResumeHeader) == 64, "TTTransferResumeHeader must be exactly 64 bytes");
#endif

// --- 2.29 Path Hash Calculation (Canonical NFC Pipeline) ---
/**
 * Canonical path normalization and xxHash3-64 calculation for remote transfer identity.
 * Strictly decoupled from local volume case-sensitivity:
 * Step 1: Unicode NFC normalization over raw device path bytes (no case folding).
 * Step 2: Lexical path resolution ('\' -> '/', collapse '//', strip './' and '../').
 * Step 3: xxHash3-64 hash generation over normalized UTF-8 byte stream.
 * Note: %XX escaping and case folding only apply to local APFS file creation, NEVER to persistent hash.
 * Returns 0 on success, -1 on illegal path traversal (Zip Slip / escaping root).
 */
int ttworkspace_calculate_path_hash(
    const char *raw_path,
    uint64_t *out_hash
);

// --- 3. Virtual File System Snapshot View & Lease ---
// Borrowed snapshot pointers populated by ttvfs_acquire_snapshot or ttzip_vfs_open_compact_catalog.
// All pointers borrow from the backing lease or Mach VM mapping and must not outlive it.
// Depending on header->feature_flags, `nodes` points either to 64-byte TTWorkspaceEntryNode records
// (when TT_FEATURE_FLAG_CSR is set without bit 4 compact-nodes) or to 24-byte TTWorkspaceCompactNode
// records reinterpreted through this pointer type; inspect header->feature_flags before dereferencing.
typedef struct {
    const TTWorkspaceSnapshotHeader *header; // Snapshot header pointer for metadata access
    const TTWorkspaceEntryNode *nodes;       // Base pointer to 64B TTWorkspaceEntryNode[] or 24B TTWorkspaceCompactNode[]
    uint64_t node_count;
    const char *string_arena;
    uint64_t string_arena_len;
    uint64_t generation;
} TTWorkspaceVfsView;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTWorkspaceVfsView) == 48, "TTWorkspaceVfsView must be exactly 48 bytes");
#endif

// --- Snapshot Lease Handle (32 bytes, 8-byte aligned) ---
// Owns the snapshot mapping (`mapping` of `size` bytes) and reference count (`ref_count`,
// manipulated atomically in Rust via AtomicU64).
struct ttvfs_lease_s {
    uint8_t    *mapping;
    size_t      size;
    uint64_t    ref_count;
    const void *alloc_ptr;
};

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(struct ttvfs_lease_s) == 32, "struct ttvfs_lease_s must be exactly 32 bytes");
_Static_assert(_Alignof(struct ttvfs_lease_s) == 8, "struct ttvfs_lease_s must be 8-byte aligned");
#endif

typedef struct ttvfs_lease_s *ttvfs_lease_t;

// Validates 64-byte header alignment, `TTWorkspaceSnapshotHeader::validate` (`total_mapping_size <= lease->size`),
// and the alignment, payload bounds (`offset + size_bytes <= total_mapping_size`), and record capacity
// (`entry_count * stride <= size_bytes`) of the node and string-arena sections in `lease->mapping`
// (without validating CSR tree topology or per-node `name_offset + name_len` bounds), populates
// `out_view`, and increments `lease->ref_count` on success (returns 1 on success, 0 on failure).
uint8_t ttvfs_acquire_snapshot(ttvfs_lease_t lease, TTWorkspaceVfsView *out_view);
// Decrements `lease->ref_count` and deallocates the lease and its backing buffer when the count reaches 0.
void ttvfs_release_snapshot(ttvfs_lease_t lease);

// --- 4. Lossless Media Smart Remux Options ---
typedef enum {
    TT_FASTSTART_AUTO     = 0, // In-place moov overwrite with 'free' padding if L_new <= L_old; else tail moov
    TT_FASTSTART_STRICT   = 1, // Require FastStart (head moov); fall back to streaming remux if in-place doesn't fit
    TT_FASTSTART_DISABLED = 2  // Tail moov without padding
} TTFastStartPolicy;

typedef struct {
    uint32_t struct_size;              // sizeof(TTRemuxOptions) for ABI forward compatibility
    int32_t  src_fd;                   // Source file descriptor (-1 if using path)
    int32_t  dest_fd;                  // Destination file descriptor (-1 if using path)
    uint32_t timebase_num;             // Timebase numerator (e.g. 1)
    uint32_t timebase_den;             // Timebase denominator (e.g. 1000 or 90000)
    uint32_t _pad_timebase;            // Explicit alignment padding to 8-byte boundary
    int64_t  start_time_pts;           // Start timestamp in timebase units
    int64_t  duration_pts;             // Duration in timebase units
    const char *src_path;
    const char *dest_path;
    uint64_t apfs_min_retained_bytes;  // Dual-threshold 1: Min retained bytes (e.g. 1GB = 1073741824)
    uint32_t apfs_min_retention_bp;    // Dual-threshold 2: Min retention basis points (e.g. 5000 = 50%)
    uint32_t faststart_policy;         // TTFastStartPolicy (0=Auto, 1=Strict, 2=Disabled)
    uint8_t  strip_audio;              // 1 = strip, 0 = keep
    uint8_t  _pad[7];                  // Explicit alignment padding
    void    *user_data;
    uint8_t (*cancel_cb)(void *user_data);
} TTRemuxOptions;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTRemuxOptions) == 96, "TTRemuxOptions must be exactly 96 bytes");
#endif

int32_t ttmedia_lossless_remux(const TTRemuxOptions *options, void (*progress_cb)(uint32_t progress_basis_points, void *user_data));

// --- 5. Wire Paging Device Read Request & Streaming Protocol ---

// Opaque Cancel Token Handle backed by atomic primitive in microkernel
typedef struct ttdevice_cancel_token_s *ttdevice_cancel_token_t;

ttdevice_cancel_token_t ttdevice_cancel_token_create(void);
void ttdevice_cancel_token_destroy(ttdevice_cancel_token_t token);
void ttdevice_cancel_token_cancel(ttdevice_cancel_token_t token);
uint8_t ttdevice_cancel_token_is_cancelled(ttdevice_cancel_token_t token);

// Opaque Device Stream Request Handle for Swift-safe lifetime and cancellation
typedef struct ttdevice_request_s ttdevice_request_t;

int32_t ttdevice_request_cancel(ttdevice_request_t *request);
void ttdevice_request_release(ttdevice_request_t *request);

typedef struct {
    char     device_id[64];
    uint64_t offset;
    uint32_t length;
    uint32_t object_id;                   // MTP ObjectHandle
    uint32_t path_offset;                 // ADB path offset in string arena
    uint16_t path_len;                    // ADB path byte length
    uint16_t _reserved;                   // Alignment padding
    uint64_t transport_epoch;             // Transport session epoch for invalidation
    ttdevice_cancel_token_t cancel_token; // Atomic cancel token handle (NULL if non-cancellable)
} TTDeviceReadRequest;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTDeviceReadRequest) == 104, "TTDeviceReadRequest must be exactly 104 bytes");
#endif

typedef void (*TTDeviceReadCompletion)(int32_t status, uint32_t bytes_read, void *user_data);

int32_t ttdevice_stream_read(const TTDeviceReadRequest *req, uint8_t *dest_buf, uint32_t *bytes_read);

/**
 * Asynchronous Device Stream Paging Read Contract:
 * 1. Exactly-Once Invocation Invariant:
 *    completion_cb is GUARANTEED to be invoked exactly once under all execution outcomes:
 *    success, I/O failure, timeout, or cancellation.
 * 2. Execution Thread Invariant:
 *    completion_cb is ALWAYS dispatched onto a dedicated background I/O serial queue.
 *    It is NEVER invoked on the main UI thread, and NEVER invoked while holding low-level USB driver locks.
 * 3. Lifecycle & Buffer Ownership Invariant:
 *    dest_buf and user_data must remain valid and unmodified until completion_cb is invoked.
 *    Upon invocation of completion_cb (even if status == TT_ERROR_CANCELLED), ownership is
 *    immediately returned to the caller; the caller may safely free or reuse the buffer.
 */
ttdevice_request_t* ttdevice_stream_read_async(
    const TTDeviceReadRequest *req,
    uint8_t *dest_buf,
    TTDeviceReadCompletion completion_cb,
    void *user_data
);

// --- 6. Telemetry Trace Context ---
typedef struct __attribute__((aligned(8))) {
    uint64_t trace_id;
    uint64_t span_id;
    uint8_t  flags;
    uint8_t  _reserved[7]; // Explicit zero padding to 8-byte boundary
} TTLogTraceContext;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTLogTraceContext) == 24, "TTLogTraceContext must be exactly 24 bytes");
#endif

TTLogTraceContext ttlog_current_context(void);
void ttlog_set_context(TTLogTraceContext ctx);

// --- 7. Rapid Video Transcoding Interface (Hardened V2) ---
#define TTMEDIA_ABI_VERSION_2_0 0x00020000

typedef enum {
    TT_TRANSCODE_LEVEL_AUTO = 0,
    TT_TRANSCODE_LEVEL_REMUX = 1,
    TT_TRANSCODE_LEVEL_AUDIO_ONLY = 2,
    TT_TRANSCODE_LEVEL_HARDWARE = 3
} TTTranscodeLevel;

typedef enum {
    TT_COLOR_PRIMARIES_UNSPECIFIED = 0,
    TT_COLOR_PRIMARIES_BT709 = 1,
    TT_COLOR_PRIMARIES_BT2020 = 2,
    TT_COLOR_PRIMARIES_P3_D65 = 3
} TTColorPrimaries;

typedef enum {
    TT_TRANSFER_FUNC_UNSPECIFIED = 0,
    TT_TRANSFER_FUNC_BT709 = 1,
    TT_TRANSFER_FUNC_SMPTE_ST2084_PQ = 2,
    TT_TRANSFER_FUNC_ARIB_STD_B67_HLG = 3
} TTTransferFunction;

typedef enum {
    TT_AUDIO_DOWNMIX_PASS_THROUGH = 0,
    TT_AUDIO_DOWNMIX_STEREO_ITU_BS775 = 1,
    TT_AUDIO_DOWNMIX_STEREO_VOCAL_BOOST = 2
} TTAudioDownmixMode;

typedef struct {
    TTColorPrimaries   primaries;
    TTTransferFunction transfer;
    uint8_t  is_full_range;
    uint8_t  _pad_color[3];
    uint32_t max_cll;
    uint32_t max_fall;
} TTColorDescriptor;

typedef struct {
    TTAudioDownmixMode downmix_mode;
    float lfe_attenuation_db;     // -144.0 for mute/drop, -10.0 for low-pass mix
    float true_peak_ceiling_dbtp; // Default -1.0 dBTP
    uint32_t sample_rate;         // 0 = keep source
    uint32_t bitrate_bps;         // e.g. 320000
} TTAudioProfileConfig;

typedef struct {
    uint32_t struct_size;         // sizeof(CVideoTranscodeParamsV2)
    uint32_t abi_version;         // TTMEDIA_ABI_VERSION_2_0
    const char *src_path;
    const char *dest_path;
    const char *target_container;
    const char *target_video_codec;
    const char *target_audio_codec;
    TTTranscodeLevel level;
    uint32_t crf;
    uint32_t width;
    uint32_t height;
    TTColorDescriptor color_profile;
    TTAudioProfileConfig audio_profile;
    uint32_t _deprecated_faststart_reserve; // Preserved as 4-byte ABI slot
} CVideoTranscodeParamsV2;

typedef int32_t (*TTTranscodeProgressCallback)(uint32_t progress_basis_points, void *user_data);

// Pure Structured Microkernel Error Payload (64-byte aligned, 0 string allocations)
typedef struct __attribute__((aligned(8))) {
    int32_t  error_code;       // Canonical error code enum (0 = Success)
    int32_t  os_status;        // Operating system error code (Darwin errno or OSStatus)
    uint16_t subsystem;        // Subsystem identifier (0=VFS, 1=USB, 2=Media, 3=Archive)
    uint16_t param_count;      // Number of active 64-bit parameters (0..4)
    uint32_t _reserved;        // Alignment padding
    uint64_t params[4];        // Structured numeric parameters (offsets, byte counts, IDs)
    uint64_t error_tag;        // 64-bit xxHash3 of callsite for telemetry correlation
    uint8_t  reserved[8];      // Future expansion padding (total exactly 64 bytes)
} TTErrorPayload;

#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
_Static_assert(sizeof(TTErrorPayload) == 64, "TTErrorPayload must be exactly 64 bytes");
#endif

int32_t ttmedia_transcode_video_v2(
    const CVideoTranscodeParamsV2 *params,
    TTTranscodeProgressCallback progress_cb,
    void *user_data,
    TTErrorPayload *err_out
);

#ifdef __cplusplus
}
#endif

#endif // TTWORKSPACE_CABI_H
