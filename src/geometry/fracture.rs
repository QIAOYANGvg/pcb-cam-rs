use std::cmp::Ordering;

use super::polyset::{PolySet, Polygon};
use super::vector::Vec2I;

impl PolySet {
    pub fn fracture(&mut self) {
        self.simplify();

        for polygon in &mut self.polygons {
            fracture_polygon(polygon);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct FractureEdge {
    p1: Vec2I,
    p2: Vec2I,
    next: usize,
}

impl FractureEdge {
    fn new(p1: Vec2I, p2: Vec2I, next: usize) -> Self {
        Self { p1, p2, next }
    }

    fn matches(self, y: i32) -> bool {
        (y >= self.p1.y || y >= self.p2.y) && (y <= self.p1.y || y <= self.p2.y)
    }
}

#[derive(Clone, Copy)]
struct FracturePathInfo {
    path_or_provoking_index: usize,
    leftmost: usize,
    x: i32,
    y_or_bridge: i64,
}

fn fracture_polygon(polygon: &mut Polygon) {
    if polygon.holes.is_empty() {
        return;
    }

    let mut paths = Vec::with_capacity(polygon.holes.len() + 1);
    paths.push(polygon.outline.clone());
    paths.extend(polygon.holes.iter().cloned());

    if paths.iter().any(Vec::is_empty) {
        return;
    }

    let total_point_count = paths.iter().map(Vec::len).sum::<usize>();
    let mut edges =
        Vec::with_capacity(total_point_count.saturating_add(paths.len().saturating_mul(3)));
    let mut sorted_paths = Vec::with_capacity(paths.len());

    for (path_index, path) in paths.iter().enumerate() {
        let mut x_min = i32::MAX;
        let mut y_min = i32::MAX;
        let mut leftmost = 0;

        for (point_index, point) in path.iter().enumerate() {
            if point.x < x_min {
                x_min = point.x;
                leftmost = point_index;
            }

            if point.y < y_min {
                y_min = point.y;
            }
        }

        sorted_paths.push(FracturePathInfo {
            path_or_provoking_index: path_index,
            leftmost,
            x: x_min,
            y_or_bridge: y_min as i64,
        });
    }

    sorted_paths[1..].sort_unstable_by(|a, b| match a.x.cmp(&b.x) {
        Ordering::Equal => a.y_or_bridge.cmp(&b.y_or_bridge),
        ordering => ordering,
    });

    let mut edge_index = 0;

    for (sorted_index, path_info) in sorted_paths.iter_mut().enumerate() {
        let path = &paths[path_info.path_or_provoking_index];
        let provoking_edge = edge_index;

        for index in 0..path.len() - 1 {
            edges.push(FractureEdge::new(
                path[index],
                path[index + 1],
                edge_index + 1,
            ));
            edge_index += 1;
        }

        edges.push(FractureEdge::new(
            path[path.len() - 1],
            path[0],
            provoking_edge,
        ));
        edge_index += 1;

        if sorted_index > 0 {
            path_info.path_or_provoking_index = provoking_edge;
            path_info.y_or_bridge = edge_index as i64;
            edge_index += 3;
            edges.resize(edge_index, FractureEdge::default());
        }
    }

    for path_info in sorted_paths.iter().skip(1) {
        let edge_index = path_info.path_or_provoking_index + path_info.leftmost;

        if !process_hole(
            &mut edges,
            path_info.path_or_provoking_index,
            edge_index,
            path_info.y_or_bridge as usize,
        ) {
            return;
        }
    }

    let mut outline = Vec::with_capacity(edges.len());
    let mut current_index = 0;

    loop {
        let edge = edges[current_index];

        if outline.last().copied() != Some(edge.p1) {
            outline.push(edge.p1);
        }

        if edge.next == 0 {
            break;
        }

        current_index = edge.next;
    }

    polygon.outline = outline;
    polygon.holes.clear();
}

fn process_hole(
    edges: &mut [FractureEdge],
    provoking_index: usize,
    edge_index: usize,
    bridge_index: usize,
) -> bool {
    let edge = edges[edge_index];
    let x = edge.p1.x;
    let y = edge.p1.y;
    let mut min_dist = i32::MAX;
    let mut x_nearest = 0;
    let mut nearest_index = None;

    for (index, candidate) in edges.iter().copied().enumerate().take(provoking_index) {
        if !candidate.matches(y) {
            continue;
        }

        let x_intersect = if candidate.p1.y == candidate.p2.y {
            candidate.p1.x.max(candidate.p2.x)
        } else {
            candidate.p1.x
                + rescale_i32(
                    candidate.p2.x - candidate.p1.x,
                    y - candidate.p1.y,
                    candidate.p2.y - candidate.p1.y,
                )
        };
        let dist = x - x_intersect;

        if dist >= 0 && dist < min_dist {
            min_dist = dist;
            x_nearest = x_intersect;
            nearest_index = Some(index);
        }
    }

    let Some(nearest_index) = nearest_index else {
        return false;
    };

    let outline_to_hole = bridge_index;
    let hole_to_outline = bridge_index + 1;
    let split = bridge_index + 2;
    let nearest = edges[nearest_index];
    let bridge_point = Vec2I::new(x_nearest, y);

    edges[outline_to_hole] = FractureEdge::new(bridge_point, edge.p1, edge_index);
    edges[hole_to_outline] = FractureEdge::new(edge.p1, bridge_point, split);
    edges[split] = FractureEdge::new(bridge_point, nearest.p2, nearest.next);
    edges[nearest_index].p2 = bridge_point;
    edges[nearest_index].next = outline_to_hole;

    let mut last_index = edge_index;

    while edges[last_index].next != edge_index {
        last_index = edges[last_index].next;
    }

    edges[last_index].next = hole_to_outline;
    true
}

fn rescale_i32(numerator: i32, value: i32, denominator: i32) -> i32 {
    let product = numerator as i64 * value as i64;
    let denominator = denominator as i64;
    let rounded = if (product < 0) ^ (denominator < 0) {
        product - denominator / 2
    } else {
        product + denominator / 2
    };

    (rounded / denominator) as i32
}
