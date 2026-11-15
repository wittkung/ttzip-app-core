// Copyright 2026 Witt Kung and TTZip Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pure Rust Bruls Squarified Treemap layout engine with viewport tail merging.
//!
//! Implements the Bruls, Huizing, and van Wijk (2000) squarified treemap algorithm,
//! optimizing aspect ratios toward 1.0 (squares) while ensuring bounded memory usage
//! and hardware-accelerated 120 FPS rasterization via viewport tail merging.

use crate::workspace::types::{
    TileCategory, TreemapTile, TREEMAP_FLAG_IS_DIR, TREEMAP_FLAG_IS_LEAF,
    TREEMAP_FLAG_TAIL_COLLAPSED, TREEMAP_OTHERS_NODE_ID,
};

/// Minimum screen area in square pixels below which elements are merged into `Others`.
pub const MIN_TILE_AREA_PX: f32 = 16.0;

/// Minimum container relative byte fraction (0.1%) below which elements are merged into `Others`.
pub const MIN_CONTAINER_RATIO: f64 = 0.001;

/// Maximum number of tiles emitted per viewport for 120 FPS latency.
pub const MAX_VIEWPORT_TILES: usize = 256;

/// Input element representation for treemap layout computation.
#[derive(Clone, Debug, PartialEq)]
pub struct TreemapInputNode {
    pub node_id: u32,
    pub bytes: u64,
    pub is_dir: bool,
    pub category: TileCategory,
    pub flags: u8,
}

impl TreemapInputNode {
    /// Creates a new input node.
    pub fn new(node_id: u32, bytes: u64, is_dir: bool, category: TileCategory) -> Self {
        let mut flags = 0u8;
        if is_dir {
            flags |= TREEMAP_FLAG_IS_DIR;
        } else {
            flags |= TREEMAP_FLAG_IS_LEAF;
        }
        Self {
            node_id,
            bytes,
            is_dir,
            category,
            flags,
        }
    }
}

/// Hierarchical input node supporting arbitrary depth traversal.
#[derive(Clone, Debug, PartialEq)]
pub struct TreemapHierarchyNode {
    pub node_id: u32,
    pub bytes: u64,
    pub is_dir: bool,
    pub category: TileCategory,
    pub flags: u8,
    pub children: Vec<TreemapHierarchyNode>,
}

#[derive(Clone, Copy, Debug)]
struct LayoutRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Clone, Copy, Debug)]
struct RowState {
    sum_a: f64,
    min_a: f64,
    max_a: f64,
    count: usize,
}

impl RowState {
    #[inline]
    fn empty() -> Self {
        Self {
            sum_a: 0.0,
            min_a: f64::MAX,
            max_a: 0.0,
            count: 0,
        }
    }

    #[inline]
    fn add(&self, a: f64) -> Self {
        Self {
            sum_a: self.sum_a + a,
            min_a: self.min_a.min(a),
            max_a: self.max_a.max(a),
            count: self.count + 1,
        }
    }

    #[inline]
    fn worst_aspect_ratio(&self, s: f64) -> f64 {
        if self.count == 0 || s <= 0.0 || self.sum_a <= 0.0 || self.min_a <= 0.0 {
            return f64::MAX;
        }
        let s2 = s * s;
        let sum2 = self.sum_a * self.sum_a;
        let r1 = (s2 * self.max_a) / sum2;
        let r2 = sum2 / (s2 * self.min_a);
        r1.max(r2)
    }
}

/// Computes a squarified treemap layout for a flat list of child items.
///
/// Implements Bruls aspect-ratio optimization and folds sub-threshold or overflow
/// entries into an `Others` aggregate tile with deterministic tie-breaking on `node_id`.
fn filter_and_promote_nodes(
    items: &[TreemapInputNode],
    total_bytes: u64,
    container_area: f64,
) -> (Vec<TreemapInputNode>, Vec<f64>) {
    let mut sorted = items.to_vec();
    sorted.sort_by_key(|b| (std::cmp::Reverse(b.bytes), b.node_id));

    let max_promoted = MAX_VIEWPORT_TILES.saturating_sub(1);
    let mut promoted: Vec<TreemapInputNode> =
        Vec::with_capacity(sorted.len().min(MAX_VIEWPORT_TILES));
    let mut tail_bytes: u64 = 0;

    if total_bytes == 0 {
        for item in sorted.into_iter().take(max_promoted) {
            promoted.push(item);
        }
    } else {
        let w_total = total_bytes as f64;
        for item in sorted {
            let ratio = (item.bytes as f64) / w_total;
            let target_area = ratio * container_area;
            let is_tail =
                target_area < (MIN_TILE_AREA_PX as f64) || ratio < MIN_CONTAINER_RATIO;

            if !is_tail && promoted.len() < max_promoted {
                promoted.push(item);
            } else {
                tail_bytes = tail_bytes.saturating_add(item.bytes);
            }
        }
    }

    if tail_bytes > 0 {
        promoted.push(TreemapInputNode {
            node_id: TREEMAP_OTHERS_NODE_ID,
            bytes: tail_bytes,
            is_dir: false,
            category: TileCategory::Others,
            flags: TREEMAP_FLAG_TAIL_COLLAPSED | TREEMAP_FLAG_IS_LEAF,
        });
    }

    let n = promoted.len();
    if n == 0 {
        return (Vec::new(), Vec::new());
    }

    let mut item_areas = Vec::with_capacity(n);
    if total_bytes == 0 {
        let uniform_area = container_area / (n as f64);
        item_areas.resize(n, uniform_area);
    } else {
        let w_total = (total_bytes as f64).max(1.0);
        for item in &promoted {
            let a = ((item.bytes as f64) / w_total) * container_area;
            item_areas.push(a.max(0.0));
        }
    }

    (promoted, item_areas)
}

/// Computes squarified treemap layout for a single viewport layer.
///
/// Merges items beneath `MIN_TILE_AREA_PX` (16.0 px²) or `MIN_CONTAINER_RATIO` (0.1%)
/// into an `Others` synthetic aggregate tile.
pub fn compute_squarified_treemap(
    items: &[TreemapInputNode],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    depth: u16,
) -> Vec<TreemapTile> {
    if !width.is_finite()
        || !height.is_finite()
        || !x.is_finite()
        || !y.is_finite()
        || width <= 0.0
        || height <= 0.0
        || items.is_empty()
    {
        return Vec::new();
    }

    let total_bytes: u64 = items
        .iter()
        .fold(0u64, |acc, it| acc.saturating_add(it.bytes));
    let container_area = (width as f64) * (height as f64);
    let (promoted, item_areas) = filter_and_promote_nodes(items, total_bytes, container_area);

    if promoted.is_empty() {
        return Vec::new();
    }

    let mut out_tiles = Vec::with_capacity(promoted.len());
    let mut rect = LayoutRect {
        x,
        y,
        w: width,
        h: height,
    };

    partition_squarified_rows(&item_areas, &promoted, depth, &mut rect, &mut out_tiles);
    out_tiles
}

fn partition_squarified_rows(
    item_areas: &[f64],
    promoted: &[TreemapInputNode],
    depth: u16,
    rect: &mut LayoutRect,
    out_tiles: &mut Vec<TreemapTile>,
) {
    let n = promoted.len();
    let mut row_indices = Vec::with_capacity(32);
    let mut row_areas = Vec::with_capacity(32);
    let mut row_state = RowState::empty();

    let mut i = 0;
    while i < n {
        let item_area = item_areas[i];
        let s = (rect.w.min(rect.h) as f64).max(0.001);

        if row_state.count == 0 {
            row_indices.push(i);
            row_areas.push(item_area);
            row_state = RowState::empty().add(item_area);
            i += 1;
            continue;
        }

        let current_worst = row_state.worst_aspect_ratio(s);
        let candidate_state = row_state.add(item_area);
        let candidate_worst = candidate_state.worst_aspect_ratio(s);

        if candidate_worst <= current_worst {
            row_indices.push(i);
            row_areas.push(item_area);
            row_state = candidate_state;
            i += 1;
        } else {
            let row_slice = RowSlice {
                indices: &row_indices,
                areas: &row_areas,
                nodes: promoted,
                depth,
                is_last: false,
            };
            layout_row(&row_slice, rect, out_tiles);
            row_indices.clear();
            row_areas.clear();
            row_state = RowState::empty();
        }
    }

    if !row_indices.is_empty() {
        let row_slice = RowSlice {
            indices: &row_indices,
            areas: &row_areas,
            nodes: promoted,
            depth,
            is_last: true,
        };
        layout_row(&row_slice, rect, out_tiles);
    }
}

struct RowSlice<'a> {
    indices: &'a [usize],
    areas: &'a [f64],
    nodes: &'a [TreemapInputNode],
    depth: u16,
    is_last: bool,
}

fn layout_vertical_strip(
    row: &RowSlice<'_>,
    row_area_sum: f64,
    rect: &mut LayoutRect,
    out: &mut Vec<TreemapTile>,
) {
    let row_w = if row.is_last {
        rect.w
    } else {
        let h = (rect.h as f64).max(0.001);
        ((row_area_sum / h) as f32).min(rect.w)
    };

    let mut cur_y = rect.y;
    let row_h_total = rect.h;
    let n_items = row.indices.len();

    for (k, (&idx, &area)) in row.indices.iter().zip(row.areas.iter()).enumerate() {
        let item_h = if k == n_items - 1 {
            (rect.y + row_h_total - cur_y).max(0.0)
        } else if row_area_sum > 0.0 {
            (((area / row_area_sum) * (row_h_total as f64)) as f32)
                .min(rect.y + row_h_total - cur_y)
        } else {
            row_h_total / (n_items as f32)
        };

        let node = &row.nodes[idx];
        out.push(TreemapTile {
            x: rect.x,
            y: cur_y,
            width: row_w.max(0.0),
            height: item_h.max(0.0),
            node_id: node.node_id,
            depth: row.depth,
            category: node.category as u8,
            flags: node.flags,
            bytes: node.bytes,
        });

        cur_y += item_h;
    }

    rect.x += row_w;
    rect.w = (rect.w - row_w).max(0.0);
}

fn layout_horizontal_strip(
    row: &RowSlice<'_>,
    row_area_sum: f64,
    rect: &mut LayoutRect,
    out: &mut Vec<TreemapTile>,
) {
    let row_h = if row.is_last {
        rect.h
    } else {
        let w = (rect.w as f64).max(0.001);
        ((row_area_sum / w) as f32).min(rect.h)
    };

    let mut cur_x = rect.x;
    let row_w_total = rect.w;
    let n_items = row.indices.len();

    for (k, (&idx, &area)) in row.indices.iter().zip(row.areas.iter()).enumerate() {
        let item_w = if k == n_items - 1 {
            (rect.x + row_w_total - cur_x).max(0.0)
        } else if row_area_sum > 0.0 {
            (((area / row_area_sum) * (row_w_total as f64)) as f32)
                .min(rect.x + row_w_total - cur_x)
        } else {
            row_w_total / (n_items as f32)
        };

        let node = &row.nodes[idx];
        out.push(TreemapTile {
            x: cur_x,
            y: rect.y,
            width: item_w.max(0.0),
            height: row_h.max(0.0),
            node_id: node.node_id,
            depth: row.depth,
            category: node.category as u8,
            flags: node.flags,
            bytes: node.bytes,
        });

        cur_x += item_w;
    }

    rect.y += row_h;
    rect.h = (rect.h - row_h).max(0.0);
}

fn layout_row(
    row: &RowSlice<'_>,
    rect: &mut LayoutRect,
    out: &mut Vec<TreemapTile>,
) {
    if row.indices.is_empty() {
        return;
    }

    let row_area_sum: f64 = row.areas.iter().sum();
    if rect.w >= rect.h {
        layout_vertical_strip(row, row_area_sum, rect, out);
    } else {
        layout_horizontal_strip(row, row_area_sum, rect, out);
    }
}

/// Recursively computes hierarchical treemap tiles down to `max_depth`.
pub fn compute_hierarchical_treemap(
    root_children: &[TreemapHierarchyNode],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    current_depth: u16,
    max_depth: u16,
) -> Vec<TreemapTile> {
    if root_children.is_empty() || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }

    let flat_inputs: Vec<TreemapInputNode> = root_children
        .iter()
        .map(|node| TreemapInputNode {
            node_id: node.node_id,
            bytes: node.bytes,
            is_dir: node.is_dir,
            category: node.category,
            flags: node.flags,
        })
        .collect();

    let tiles = compute_squarified_treemap(&flat_inputs, x, y, width, height, current_depth);

    if current_depth >= max_depth {
        return tiles;
    }

    let mut result = Vec::with_capacity(tiles.len() * 2);
    for tile in tiles {
        let is_dir = (tile.flags & TREEMAP_FLAG_IS_DIR) != 0;
        let is_tail = (tile.flags & TREEMAP_FLAG_TAIL_COLLAPSED) != 0;

        if is_dir && !is_tail && tile.width >= 16.0 && tile.height >= 16.0 {
            if let Some(hier_node) = root_children.iter().find(|n| n.node_id == tile.node_id) {
                if !hier_node.children.is_empty() {
                    let child_tiles = compute_hierarchical_treemap(
                        &hier_node.children,
                        tile.x,
                        tile.y,
                        tile.width,
                        tile.height,
                        current_depth + 1,
                        max_depth,
                    );
                    result.extend(child_tiles);
                    continue;
                }
            }
        }
        result.push(tile);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_treemap() {
        let tiles = compute_squarified_treemap(&[], 0.0, 0.0, 1000.0, 800.0, 1);
        assert!(tiles.is_empty());
    }

    #[test]
    fn test_zero_dimension_treemap() {
        let items = vec![TreemapInputNode::new(1, 1000, false, TileCategory::GeneralCode)];
        let t1 = compute_squarified_treemap(&items, 0.0, 0.0, 0.0, 800.0, 1);
        let t2 = compute_squarified_treemap(&items, 0.0, 0.0, 1000.0, 0.0, 1);
        assert!(t1.is_empty());
        assert!(t2.is_empty());
    }

    #[test]
    fn test_single_node_treemap() {
        let items = vec![TreemapInputNode::new(42, 500_000, false, TileCategory::MediaVideo)];
        let tiles = compute_squarified_treemap(&items, 10.0, 20.0, 800.0, 600.0, 1);
        assert_eq!(tiles.len(), 1);
        let tile = &tiles[0];
        assert_eq!(tile.node_id, 42);
        assert_eq!(tile.x, 10.0);
        assert_eq!(tile.y, 20.0);
        assert_eq!(tile.width, 800.0);
        assert_eq!(tile.height, 600.0);
        assert_eq!(tile.depth, 1);
        assert_eq!(tile.category, TileCategory::MediaVideo as u8);
        assert_eq!(tile.bytes, 500_000);
    }

    #[test]
    fn test_tail_merging_and_others_tile() {
        let mut items = Vec::new();
        // 1 large file: 1,000,000 bytes
        items.push(TreemapInputNode::new(1, 1_000_000, false, TileCategory::CompressedArchive));
        // 500 tiny files: 1 byte each (0.0001% of total bytes, well below 0.05% cutoff)
        for i in 2..=501 {
            items.push(TreemapInputNode::new(i, 1, false, TileCategory::GeneralCode));
        }

        let tiles = compute_squarified_treemap(&items, 0.0, 0.0, 1000.0, 1000.0, 1);
        // Only 2 tiles: the large file and the aggregated "Others" tile!
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].node_id, 1);
        assert_eq!(tiles[1].node_id, TREEMAP_OTHERS_NODE_ID);
        assert_eq!(tiles[1].category, TileCategory::Others as u8);
        assert_eq!(tiles[1].bytes, 500);
        assert_ne!(tiles[1].flags & TREEMAP_FLAG_TAIL_COLLAPSED, 0);
    }

    #[test]
    fn test_multi_layer_nested_tree() {
        let tree = vec![
            TreemapHierarchyNode {
                node_id: 1,
                bytes: 800_000,
                is_dir: true,
                category: TileCategory::GeneralCode,
                flags: TREEMAP_FLAG_IS_DIR,
                children: vec![
                    TreemapHierarchyNode {
                        node_id: 10,
                        bytes: 400_000,
                        is_dir: false,
                        category: TileCategory::GeneralCode,
                        flags: TREEMAP_FLAG_IS_LEAF,
                        children: vec![],
                    },
                    TreemapHierarchyNode {
                        node_id: 11,
                        bytes: 400_000,
                        is_dir: false,
                        category: TileCategory::DocumentText,
                        flags: TREEMAP_FLAG_IS_LEAF,
                        children: vec![],
                    },
                ],
            },
            TreemapHierarchyNode {
                node_id: 2,
                bytes: 200_000,
                is_dir: false,
                category: TileCategory::MediaImage,
                flags: TREEMAP_FLAG_IS_LEAF,
                children: vec![],
            },
        ];

        let tiles = compute_hierarchical_treemap(&tree, 0.0, 0.0, 1000.0, 1000.0, 1, 2);
        assert_eq!(tiles.len(), 3);
        assert!(tiles.iter().any(|t| t.node_id == 10 && t.depth == 2));
        assert!(tiles.iter().any(|t| t.node_id == 11 && t.depth == 2));
        assert!(tiles.iter().any(|t| t.node_id == 2 && t.depth == 1));
    }

    #[test]
    fn test_synthetic_50k_nodes_performance() {
        let mut items = Vec::with_capacity(50_000);
        // Distribute byte sizes with Zipf-like distribution
        for i in 0..50_000u32 {
            let bytes = if i < 100 {
                (50_000 - i as u64) * 100_000
            } else {
                (i % 50) as u64 + 1
            };
            items.push(TreemapInputNode::new(
                i,
                bytes,
                i % 10 == 0,
                TileCategory::from_u8((i % 9) as u8),
            ));
        }

        let start = std::time::Instant::now();
        let tiles = compute_squarified_treemap(&items, 0.0, 0.0, 1920.0, 1080.0, 1);
        let elapsed = start.elapsed();

        assert!(!tiles.is_empty());
        assert!(tiles.len() <= MAX_VIEWPORT_TILES);
        for t in &tiles {
            assert!(t.width >= 0.0);
            assert!(t.height >= 0.0);
            assert!(t.x >= 0.0 && t.x <= 1920.1);
            assert!(t.y >= 0.0 && t.y <= 1080.1);
        }

        // Must run smoothly without memory exhaustion
        println!("50,000 node treemap layout took: {:?}", elapsed);
    }

    #[test]
    fn test_byte_conservation_and_deterministic_tie_breaking() {
        let items_a = vec![
            TreemapInputNode::new(30, 100_000, false, TileCategory::GeneralCode),
            TreemapInputNode::new(10, 100_000, false, TileCategory::MediaImage),
            TreemapInputNode::new(20, 100_000, true, TileCategory::CompressedArchive),
            TreemapInputNode::new(40, 50, false, TileCategory::DocumentText),
            TreemapInputNode::new(50, 30, false, TileCategory::MediaAudio),
        ];
        let items_b = vec![
            items_a[4].clone(),
            items_a[2].clone(),
            items_a[0].clone(),
            items_a[3].clone(),
            items_a[1].clone(),
        ];

        let tiles_a = compute_squarified_treemap(&items_a, 0.0, 0.0, 800.0, 600.0, 1);
        let tiles_b = compute_squarified_treemap(&items_b, 0.0, 0.0, 800.0, 600.0, 1);

        assert_eq!(tiles_a, tiles_b, "Layout must be deterministic regardless of input permutation");

        let input_sum: u64 = items_a.iter().map(|n| n.bytes).sum();
        let output_sum: u64 = tiles_a.iter().map(|t| t.bytes).sum();
        assert_eq!(input_sum, output_sum, "Tile byte sum must exactly equal input byte sum");
        assert_eq!(tiles_a[0].node_id, 10);
        assert_eq!(tiles_a[1].node_id, 20);
        assert_eq!(tiles_a[2].node_id, 30);
    }

    #[test]
    fn test_non_finite_dimensions_rejected() {
        let items = vec![TreemapInputNode::new(1, 1024, false, TileCategory::GeneralCode)];
        assert!(compute_squarified_treemap(&items, 0.0, 0.0, f32::NAN, 500.0, 1).is_empty());
        assert!(compute_squarified_treemap(&items, 0.0, 0.0, 500.0, f32::INFINITY, 1).is_empty());
        assert!(compute_squarified_treemap(&items, f32::NEG_INFINITY, 0.0, 500.0, 500.0, 1).is_empty());
    }
}
