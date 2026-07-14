//! Minimal KiCad-like geometry helpers for Gerber polygon and bounding-box work.

use super::vector::{Vec2I, add, rotate_point};
use crate::clipper_bridge::{ClipperNode, ClipperOperation, ClipperTree};

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
    let mut paths = Vec::new();

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
