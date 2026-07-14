//! Minimal KiCad-like geometry helpers for Gerber polygon and bounding-box work.

use std::cmp::Ordering;

use crate::clipper_bridge::{ClipperNode, ClipperOperation, ClipperTree};
use crate::coord::Vec2I;

const MIN_SEGCOUNT_FOR_CIRCLE: f64 = 8.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Box2I {
    pub origin: Vec2I,
    pub size: Vec2I,
}

impl Box2I {
    pub const fn new(origin: Vec2I, size: Vec2I) -> Self {
        Self { origin, size }
    }

    pub fn by_corners(a: Vec2I, b: Vec2I) -> Self {
        let mut bbox = Self::new(a, Vec2I::new(b.x - a.x, b.y - a.y));
        bbox.normalize();
        bbox
    }

    pub fn end(&self) -> Vec2I {
        Vec2I::new(self.origin.x + self.size.x, self.origin.y + self.size.y)
    }

    pub fn set_end(&mut self, end: Vec2I) {
        self.size = Vec2I::new(end.x - self.origin.x, end.y - self.origin.y);
    }

    pub fn width(&self) -> i32 {
        self.size.x
    }

    pub fn height(&self) -> i32 {
        self.size.y
    }

    pub fn is_empty(&self) -> bool {
        self.size.x == 0 && self.size.y == 0
    }

    pub fn normalize(&mut self) {
        if self.size.x < 0 {
            self.origin.x += self.size.x;
            self.size.x = -self.size.x;
        }

        if self.size.y < 0 {
            self.origin.y += self.size.y;
            self.size.y = -self.size.y;
        }
    }

    pub fn inflate(&mut self, dx: i32, dy: i32) {
        self.origin.x -= dx;
        self.origin.y -= dy;
        self.size.x += dx.saturating_mul(2);
        self.size.y += dy.saturating_mul(2);
    }

    pub fn merge_point(&mut self, point: Vec2I) {
        self.normalize();
        let end = self.end();
        let min_x = self.origin.x.min(point.x);
        let min_y = self.origin.y.min(point.y);
        let max_x = end.x.max(point.x);
        let max_y = end.y.max(point.y);
        self.origin = Vec2I::new(min_x, min_y);
        self.set_end(Vec2I::new(max_x, max_y));
        self.normalize();
    }

    pub fn merge(&mut self, other: Box2I) {
        let mut other = other;
        other.normalize();
        self.merge_point(other.origin);
        self.merge_point(other.end());
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Polygon {
    pub outline: Vec<Vec2I>,
    pub holes: Vec<Vec<Vec2I>>,
}

impl Polygon {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_outline(outline: Vec<Vec2I>) -> Self {
        Self {
            outline,
            holes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PolySet {
    pub polygons: Vec<Polygon>,
}

impl PolySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_outlines(outlines: &[Vec<Vec2I>]) -> Self {
        Self {
            polygons: outlines
                .iter()
                .cloned()
                .map(Polygon::with_outline)
                .collect(),
        }
    }

    pub fn remove_all_contours(&mut self) {
        self.polygons.clear();
    }

    pub fn outline_count(&self) -> usize {
        self.polygons.len()
    }

    pub fn new_outline(&mut self) -> usize {
        self.polygons.push(Polygon::new());
        self.polygons.len() - 1
    }

    pub fn append(&mut self, point: Vec2I) {
        if self.polygons.is_empty() {
            self.new_outline();
        }

        if let Some(poly) = self.polygons.last_mut() {
            poly.outline.push(point);
        }
    }

    pub fn add_outline(&mut self, outline: Vec<Vec2I>) -> usize {
        self.polygons.push(Polygon::with_outline(outline));
        self.polygons.len() - 1
    }

    pub fn add_hole(&mut self, outline_index: usize, hole: Vec<Vec2I>) {
        if let Some(poly) = self.polygons.get_mut(outline_index) {
            poly.holes.push(hole);
        }
    }

    pub fn append_polyset(&mut self, other: &PolySet) {
        self.polygons.extend(other.polygons.iter().cloned());
    }

    pub fn boolean_add(&mut self, other: &PolySet) {
        *self = clipper_boolean_op(self, other, ClipperOperation::Union);
    }

    pub fn boolean_subtract(&mut self, other: &PolySet) {
        *self = clipper_boolean_op(self, other, ClipperOperation::Difference);
    }

    pub fn simplify(&mut self) {
        *self = clipper_boolean_op(self, &PolySet::new(), ClipperOperation::Union);
    }

    pub fn fracture(&mut self) {
        self.simplify();

        for polygon in &mut self.polygons {
            fracture_polygon(polygon);
        }
    }

    pub fn move_by(&mut self, delta: Vec2I) {
        for poly in &mut self.polygons {
            for point in &mut poly.outline {
                *point = add(*point, delta);
            }

            for hole in &mut poly.holes {
                for point in hole {
                    *point = add(*point, delta);
                }
            }
        }
    }

    pub fn rotate(&mut self, angle_degrees: f64) {
        for poly in &mut self.polygons {
            for point in &mut poly.outline {
                *point = rotate_point(*point, angle_degrees);
            }

            for hole in &mut poly.holes {
                for point in hole {
                    *point = rotate_point(*point, angle_degrees);
                }
            }
        }
    }

    pub fn mirror_top_bottom(&mut self) {
        for poly in &mut self.polygons {
            for point in &mut poly.outline {
                point.y = -point.y;
            }

            for hole in &mut poly.holes {
                for point in hole {
                    point.y = -point.y;
                }
            }
        }
    }

    pub fn bbox(&self) -> Option<Box2I> {
        let mut iter = self
            .polygons
            .iter()
            .flat_map(|poly| poly.outline.iter().copied());
        let first = iter.next()?;
        let mut bbox = Box2I::new(first, Vec2I::new(0, 0));

        for point in iter {
            bbox.merge_point(point);
        }

        Some(bbox)
    }

    pub fn to_outlines(&self) -> Vec<Vec<Vec2I>> {
        self.polygons
            .iter()
            .map(|poly| poly.outline.clone())
            .collect()
    }
}

pub fn add(a: Vec2I, b: Vec2I) -> Vec2I {
    Vec2I::new(a.x + b.x, a.y + b.y)
}

pub fn sub(a: Vec2I, b: Vec2I) -> Vec2I {
    Vec2I::new(a.x - b.x, a.y - b.y)
}

pub fn neg(a: Vec2I) -> Vec2I {
    Vec2I::new(-a.x, -a.y)
}

fn ring_has_area(points: &[Vec2I]) -> bool {
    let point_count = if points.len() > 1 && points.first() == points.last() {
        points.len() - 1
    } else {
        points.len()
    };

    if point_count < 3 {
        return false;
    }

    signed_area2(points) != 0
}

fn signed_area2(points: &[Vec2I]) -> i64 {
    if points.len() < 3 {
        return 0;
    }

    let mut area = 0_i64;

    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        area += (points[index].x as i64 * points[next].y as i64)
            - (points[next].x as i64 * points[index].y as i64);
    }

    area
}

fn polyset_to_clipper_paths(polyset: &PolySet) -> Vec<Vec<Vec2I>> {
    let mut paths = Vec::with_capacity(polyset.contour_count());

    for poly in &polyset.polygons {
        if let Some(outline) = contour_for_clipper(&poly.outline, true) {
            paths.push(outline);
        }

        for hole in &poly.holes {
            if let Some(hole) = contour_for_clipper(hole, false) {
                paths.push(hole);
            }
        }
    }

    paths
}

fn contour_for_clipper(points: &[Vec2I], want_positive_area: bool) -> Option<Vec<Vec2I>> {
    let mut ring = Vec::with_capacity(points.len());

    for point in points.iter().copied() {
        if ring.last().copied() != Some(point) {
            ring.push(point);
        }
    }

    while ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }

    if !ring_has_area(&ring) {
        return None;
    }

    let has_positive_area = signed_area2(&ring) >= 0;

    if has_positive_area != want_positive_area {
        ring.reverse();
    }

    Some(ring)
}

fn clipper_boolean_op(subject: &PolySet, clip: &PolySet, operation: ClipperOperation) -> PolySet {
    let subject_paths = polyset_to_clipper_paths(subject);
    let clip_paths = polyset_to_clipper_paths(clip);
    let tree = ClipperTree::execute(operation, &subject_paths, &clip_paths)
        .expect("KiCad Clipper2 boolean operation failed");
    polyset_from_clipper_tree(&tree)
}

fn polyset_from_clipper_tree(tree: &ClipperTree) -> PolySet {
    let mut polyset = PolySet::new();
    let root = tree.root();

    for child_index in 0..root.child_count() {
        if let Some(child) = root.child(child_index) {
            import_clipper_node(child, &mut polyset);
        }
    }

    polyset
}

fn import_clipper_node(node: ClipperNode<'_>, polyset: &mut PolySet) {
    let mut polygon = Polygon::with_outline(node.points());

    for child_index in 0..node.child_count() {
        if let Some(hole) = node.child(child_index) {
            polygon.holes.push(hole.points());

            for grandchild_index in 0..hole.child_count() {
                if let Some(grandchild) = hole.child(grandchild_index) {
                    import_clipper_node(grandchild, polyset);
                }
            }
        }
    }

    polyset.polygons.push(polygon);
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

impl PolySet {
    fn contour_count(&self) -> usize {
        self.polygons
            .iter()
            .map(|poly| 1_usize.saturating_add(poly.holes.len()))
            .sum()
    }
}

pub fn scale(a: Vec2I, factor: f64) -> Vec2I {
    Vec2I::new(ki_round(a.x as f64 * factor), ki_round(a.y as f64 * factor))
}

pub fn ki_round(value: f64) -> i32 {
    value.round() as i32
}

pub fn distance(a: Vec2I, b: Vec2I) -> f64 {
    let dx = (a.x - b.x) as f64;
    let dy = (a.y - b.y) as f64;
    (dx * dx + dy * dy).sqrt()
}

pub fn euclidean_norm(point: Vec2I) -> i32 {
    ki_round(((point.x as f64).powi(2) + (point.y as f64).powi(2)).sqrt())
}

pub fn angle_degrees(point: Vec2I) -> f64 {
    (point.y as f64).atan2(point.x as f64).to_degrees()
}

pub fn rotate_point(point: Vec2I, angle_degrees: f64) -> Vec2I {
    let angle = angle_degrees.to_radians();
    let sin = angle.sin();
    let cos = angle.cos();
    let x = point.x as f64;
    let y = point.y as f64;

    Vec2I::new(ki_round(x * cos - y * sin), ki_round(x * sin + y * cos))
}

pub fn get_arc_to_segment_count(radius: i32, error_max: i32, arc_angle: f64) -> i32 {
    let radius = radius.max(1);
    let error_max = error_max.max(1);
    let rel_error = error_max as f64 / radius as f64;
    let cos_arg = (1.0 - rel_error).clamp(-1.0, 1.0);
    let arc_increment =
        (180.0 / std::f64::consts::PI * cos_arg.acos() * 2.0).min(360.0 / MIN_SEGCOUNT_FOR_CIRCLE);
    let seg_count = (arc_angle.abs() / arc_increment).round() as i32;

    seg_count.max(2)
}

pub fn circle_to_polygon(radius: i32, seg_count: usize) -> Vec<Vec2I> {
    let count = seg_count.max(8);
    circle_to_polygon_count(radius, count)
}

pub fn circle_to_polygon_by_error(radius: i32, error_max: i32) -> Vec<Vec2I> {
    let mut count = get_arc_to_segment_count(radius, error_max, 360.0).max(8) as usize;
    count = count.div_ceil(8) * 8;
    circle_to_polygon_count(radius, count)
}

fn circle_to_polygon_count(radius: i32, count: usize) -> Vec<Vec2I> {
    let delta = 360.0 / count as f64;
    let mut outline = Vec::with_capacity(count + 1);

    for ii in 0..count {
        let angle = delta / 2.0 + delta * ii as f64;
        outline.push(rotate_point(Vec2I::new(radius, 0), angle));
    }

    if let Some(first) = outline.first().copied() {
        outline.push(first);
    }

    outline
}

pub fn rectangle_to_polygon(size: Vec2I) -> Vec<Vec2I> {
    let mut curr = Vec2I::new(size.x / 2, size.y / 2);
    let initial = curr;

    vec![
        curr,
        {
            curr.x -= size.x;
            curr
        },
        {
            curr.y -= size.y;
            curr
        },
        {
            curr.x += size.x;
            curr
        },
        {
            curr.y += size.y;
            curr
        },
        initial,
    ]
}

pub fn regular_polygon_to_polygon(radius: i32, edges: i32, rotation_degrees: f64) -> Vec<Vec2I> {
    let edges = edges.max(3) as usize;
    let mut outline = Vec::with_capacity(edges);

    for ii in 0..edges {
        let angle = 360.0 * ii as f64 / edges as f64 - rotation_degrees;
        outline.push(rotate_point(Vec2I::new(radius, 0), angle));
    }

    outline
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_normalizes_negative_size() {
        let mut bbox = Box2I::new(Vec2I::new(10, 20), Vec2I::new(-5, -7));
        bbox.normalize();
        assert_eq!(bbox.origin, Vec2I::new(5, 13));
        assert_eq!(bbox.size, Vec2I::new(5, 7));
    }

    #[test]
    fn polyset_bbox_uses_outline_points() {
        let poly =
            PolySet::from_outlines(&[vec![Vec2I::new(5, 4), Vec2I::new(-1, 8), Vec2I::new(2, -3)]]);
        let bbox = poly.bbox().unwrap();
        assert_eq!(bbox.origin, Vec2I::new(-1, -3));
        assert_eq!(bbox.end(), Vec2I::new(5, 8));
    }

    #[test]
    fn polyset_simplify_uses_clipper_representation_without_duplicate_closure() {
        let mut poly = PolySet::from_outlines(&[vec![
            Vec2I::new(0, 0),
            Vec2I::new(10, 0),
            Vec2I::new(10, 0),
            Vec2I::new(10, 10),
            Vec2I::new(0, 10),
        ]]);

        poly.simplify();

        assert_eq!(poly.polygons[0].outline.len(), 4);
        assert_ne!(
            poly.polygons[0].outline.first(),
            poly.polygons[0].outline.last()
        );
        assert_eq!(ring_area_abs(&poly.polygons[0].outline), 100);
    }

    #[test]
    fn polyset_simplify_removes_zero_area_outlines_and_holes() {
        let mut poly = PolySet::from_outlines(&[
            vec![
                Vec2I::new(0, 0),
                Vec2I::new(5, 0),
                Vec2I::new(10, 0),
                Vec2I::new(0, 0),
            ],
            vec![
                Vec2I::new(0, 0),
                Vec2I::new(10, 0),
                Vec2I::new(10, 10),
                Vec2I::new(0, 10),
                Vec2I::new(0, 0),
            ],
        ]);
        poly.add_hole(
            1,
            vec![
                Vec2I::new(1, 1),
                Vec2I::new(2, 1),
                Vec2I::new(3, 1),
                Vec2I::new(1, 1),
            ],
        );

        poly.simplify();

        assert_eq!(poly.polygons.len(), 1);
        assert!(poly.polygons[0].holes.is_empty());
    }

    #[test]
    fn boolean_subtract_creates_real_hole_for_enclosed_clip() {
        let mut subject = PolySet::from_outlines(&[vec![
            Vec2I::new(0, 0),
            Vec2I::new(100, 0),
            Vec2I::new(100, 100),
            Vec2I::new(0, 100),
            Vec2I::new(0, 0),
        ]]);
        let clip = PolySet::from_outlines(&[vec![
            Vec2I::new(25, 25),
            Vec2I::new(75, 25),
            Vec2I::new(75, 75),
            Vec2I::new(25, 75),
            Vec2I::new(25, 25),
        ]]);

        subject.boolean_subtract(&clip);

        assert_eq!(subject.polygons.len(), 1);
        assert_eq!(subject.polygons[0].holes.len(), 1);
        assert!(ring_has_area(&subject.polygons[0].outline));
        assert!(ring_has_area(&subject.polygons[0].holes[0]));
    }

    #[test]
    fn boolean_subtract_trims_overlapping_clip_area() {
        let mut subject = PolySet::from_outlines(&[vec![
            Vec2I::new(0, 0),
            Vec2I::new(100, 0),
            Vec2I::new(100, 100),
            Vec2I::new(0, 100),
            Vec2I::new(0, 0),
        ]]);
        let clip = PolySet::from_outlines(&[vec![
            Vec2I::new(50, 50),
            Vec2I::new(150, 50),
            Vec2I::new(150, 150),
            Vec2I::new(50, 150),
            Vec2I::new(50, 50),
        ]]);

        subject.boolean_subtract(&clip);

        assert_eq!(subject.polygons.len(), 1);
        assert!(subject.polygons[0].holes.is_empty());
        assert_eq!(ring_area_abs(&subject.polygons[0].outline), 7_500);
    }

    #[test]
    fn fracture_bridges_an_enclosed_hole_like_kicad() {
        let mut subject = PolySet::from_outlines(&[vec![
            Vec2I::new(0, 0),
            Vec2I::new(100, 0),
            Vec2I::new(100, 100),
            Vec2I::new(0, 100),
        ]]);
        let clip = PolySet::from_outlines(&[vec![
            Vec2I::new(25, 25),
            Vec2I::new(75, 25),
            Vec2I::new(75, 75),
            Vec2I::new(25, 75),
        ]]);

        subject.boolean_subtract(&clip);
        subject.fracture();

        assert_eq!(subject.polygons.len(), 1);
        assert!(subject.polygons[0].holes.is_empty());
        assert_eq!(subject.polygons[0].outline.len(), 11);
        assert_eq!(ring_area_abs(&subject.polygons[0].outline), 7_500);
    }

    fn ring_area_abs(points: &[Vec2I]) -> i64 {
        signed_area2(points).abs() / 2
    }
}
