//! Shared graph primitive: one multi-source uniform-cost Dijkstra used by both navigation fields.
//!
//! [`flowfield::FlowField`](crate::flowfield) (an 8-connected floor grid) and
//! [`surface_nav::SurfaceField`](crate::surface_nav) (a wall-patch graph) ran byte-identical copies of
//! the same expansion — a reversed-`Ord` min-heap node, the lazy-deletion pop loop, and the
//! [`UNREACHABLE`] sentinel — differing only in how neighbours are enumerated and how the resulting cost
//! array is turned into a flow. This module holds the one shared expansion; each field supplies its own
//! successor enumeration (a closure) and keeps its own steepest-descent flow extraction. The produced
//! fields are byte-identical to the hand-rolled versions: Dijkstra's cost-to-nearest-source array is
//! unique (independent of how equal-cost heap ties pop), and each caller's flow pass is unchanged.

use std::collections::BinaryHeap;

/// Cost of a node no source can reach. The shared sentinel both fields test against.
pub const UNREACHABLE: u32 = u32::MAX;

/// Min-heap entry ordered so [`BinaryHeap`] pops the lowest cost first (reversed `Ord`). It compares on
/// `cost` only — the produced cost array does not depend on how equal-cost ties pop, so this stays
/// deterministic.
struct HeapNode {
    cost: u32,
    node: usize,
}
impl PartialEq for HeapNode {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}
impl Eq for HeapNode {}
impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.cost.cmp(&self.cost) // reversed → min-heap
    }
}
impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Multi-source uniform-cost Dijkstra over `node_count` integer-indexed nodes. Every index in `sources`
/// is seeded at cost 0 (duplicates ignored), then `for_each_successor(node, &mut relax)` enumerates a
/// node's out-edges, calling `relax(neighbour_index, edge_cost)` for each. Returns the cost-to-nearest-
/// source array ([`UNREACHABLE`] where no source reaches). Callers must yield only indices `< node_count`
/// (both fields already filter their sources/edges to valid indices); an out-of-range index panics.
pub fn dijkstra_multi_source<F>(
    node_count: usize,
    sources: impl IntoIterator<Item = usize>,
    mut for_each_successor: F,
) -> Vec<u32>
where
    F: FnMut(usize, &mut dyn FnMut(usize, u32)),
{
    let mut cost = vec![UNREACHABLE; node_count];
    let mut heap = BinaryHeap::new();
    for s in sources {
        if cost[s] == 0 {
            continue; // duplicate source already seeded
        }
        cost[s] = 0;
        heap.push(HeapNode { cost: 0, node: s });
    }

    while let Some(HeapNode { cost: c, node }) = heap.pop() {
        if c > cost[node] {
            continue; // lazy deletion: a stale entry we've already beaten
        }
        for_each_successor(node, &mut |neighbour, step| {
            let nc = c + step;
            if nc < cost[neighbour] {
                cost[neighbour] = nc;
                heap.push(HeapNode {
                    cost: nc,
                    node: neighbour,
                });
            }
        });
    }
    cost
}
