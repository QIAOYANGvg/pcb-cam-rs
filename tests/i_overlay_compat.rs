use gerber_parse::geometry::{PolySet, Polygon, Vec2I};

#[derive(Clone, Copy)]
enum BooleanOperation {
    Union,
    Difference,
    Simplify,
}

#[test]
fn overlapping_union_matches_clipper2_fixture() {
    let subject = PolySet::from_outlines(&[rect(0, 0, 10, 10)]);
    let clip = PolySet::from_outlines(&[rect(5, -2, 15, 8)]);
    let expected = PolySet::from_outlines(&[vec![
        point(0, 0),
        point(5, 0),
        point(5, -2),
        point(15, -2),
        point(15, 8),
        point(10, 8),
        point(10, 10),
        point(0, 10),
    ]]);

    assert_matches_clipper2_fixture(subject, clip, BooleanOperation::Union, expected);
}

#[test]
fn enclosed_difference_matches_clipper2_hole_topology() {
    let subject = PolySet::from_outlines(&[rect(0, 0, 20, 20)]);
    let clip = PolySet::from_outlines(&[rect(5, 5, 15, 15)]);
    let mut expected = PolySet::new();
    let outline = expected.add_outline(rect(0, 0, 20, 20));
    expected.add_hole(outline, rect(5, 5, 15, 15));

    assert_matches_clipper2_fixture(subject, clip, BooleanOperation::Difference, expected);
}

#[test]
fn overlapping_subject_contours_match_clipper2_fixture() {
    let subject =
        PolySet::from_outlines(&[rect(0, 0, 10, 10), rect(5, 0, 15, 10), rect(7, -5, 12, 15)]);
    let expected = PolySet::from_outlines(&[vec![
        point(0, 0),
        point(7, 0),
        point(7, -5),
        point(12, -5),
        point(12, 0),
        point(15, 0),
        point(15, 10),
        point(12, 10),
        point(12, 15),
        point(7, 15),
        point(7, 10),
        point(0, 10),
    ]]);

    assert_matches_clipper2_fixture(
        subject,
        PolySet::new(),
        BooleanOperation::Simplify,
        expected,
    );
}

#[test]
#[ignore = "i_overlay preserves one extra redundant collinear vertex"]
fn overlapping_subject_contours_have_identical_strict_vertices() {
    let subject =
        PolySet::from_outlines(&[rect(0, 0, 10, 10), rect(5, 0, 15, 10), rect(7, -5, 12, 15)]);
    let actual = run_operation(subject, PolySet::new(), BooleanOperation::Simplify);
    let historical_clipper2 = PolySet {
        polygons: vec![Polygon {
            outline: vec![
                point(12, 0),
                point(15, 0),
                point(15, 10),
                point(12, 10),
                point(12, 15),
                point(7, 15),
                point(7, 10),
                point(5, 10),
                point(0, 10),
                point(0, 0),
                point(7, 0),
                point(7, -5),
                point(12, -5),
            ],
            holes: Vec::new(),
        }],
    };

    assert_eq!(
        strict_canonical_polyset(&actual),
        strict_canonical_polyset(&historical_clipper2)
    );
}

#[test]
fn nested_hole_and_island_match_clipper2_fixture() {
    let mut subject = PolySet::new();
    let outer = subject.add_outline(rect(0, 0, 30, 30));
    subject.add_hole(outer, rect(5, 5, 25, 25));
    subject.add_outline(rect(10, 10, 20, 20));

    let mut expected = PolySet::new();
    let outer = expected.add_outline(rect(0, 0, 30, 30));
    expected.add_hole(outer, rect(5, 5, 25, 25));
    expected.add_outline(rect(10, 10, 20, 20));

    assert_matches_clipper2_fixture(
        subject,
        PolySet::new(),
        BooleanOperation::Simplify,
        expected,
    );
}

#[test]
fn collinear_vertices_are_preserved_like_clipper2() {
    let subject = PolySet::from_outlines(&[vec![
        point(0, 0),
        point(5, 0),
        point(10, 0),
        point(10, 10),
        point(0, 10),
    ]]);
    let actual = run_operation(subject.clone(), PolySet::new(), BooleanOperation::Simplify);

    assert_eq!(
        strict_canonical_polyset(&actual),
        strict_canonical_polyset(&subject)
    );
}

#[test]
fn zero_signed_area_self_intersection_matches_clipper2_fixture() {
    let subject =
        PolySet::from_outlines(&[vec![point(0, 0), point(10, 10), point(10, 0), point(0, 10)]]);
    let actual = run_operation(subject, PolySet::new(), BooleanOperation::Simplify);

    assert!(actual.polygons.is_empty());
}

fn assert_matches_clipper2_fixture(
    subject: PolySet,
    clip: PolySet,
    operation: BooleanOperation,
    expected: PolySet,
) {
    let actual = run_operation(subject, clip, operation);

    assert_eq!(
        canonical_polyset(&actual),
        canonical_polyset(&expected),
        "i_overlay result:\n{actual:#?}\nHistorical Clipper2 fixture:\n{expected:#?}"
    );
}

fn run_operation(mut subject: PolySet, clip: PolySet, operation: BooleanOperation) -> PolySet {
    match operation {
        BooleanOperation::Union => subject.boolean_add(&clip),
        BooleanOperation::Difference => subject.boolean_subtract(&clip),
        BooleanOperation::Simplify => subject.simplify(),
    }

    subject
}

type CanonicalRing = Vec<(i32, i32)>;
type CanonicalPolygon = (CanonicalRing, Vec<CanonicalRing>);

fn canonical_polyset(polyset: &PolySet) -> Vec<CanonicalPolygon> {
    canonical_polyset_with(polyset, canonical_ring)
}

fn strict_canonical_polyset(polyset: &PolySet) -> Vec<CanonicalPolygon> {
    canonical_polyset_with(polyset, strict_canonical_ring)
}

fn canonical_polyset_with(
    polyset: &PolySet,
    canonicalize: fn(&[Vec2I]) -> CanonicalRing,
) -> Vec<CanonicalPolygon> {
    let mut polygons = polyset
        .polygons
        .iter()
        .map(|polygon| {
            let outline = canonicalize(&polygon.outline);
            let mut holes = polygon
                .holes
                .iter()
                .map(|hole| canonicalize(hole))
                .collect::<Vec<_>>();
            holes.sort();
            (outline, holes)
        })
        .collect::<Vec<_>>();

    polygons.sort();
    polygons
}

fn canonical_ring(points: &[Vec2I]) -> CanonicalRing {
    let mut ring = normalized_ring(points);
    remove_redundant_collinear_points(&mut ring);
    canonicalize_ring(ring)
}

fn strict_canonical_ring(points: &[Vec2I]) -> CanonicalRing {
    canonicalize_ring(normalized_ring(points))
}

fn normalized_ring(points: &[Vec2I]) -> CanonicalRing {
    let mut ring = Vec::with_capacity(points.len());

    for point in points {
        let point = (point.x, point.y);
        if ring.last().copied() != Some(point) {
            ring.push(point);
        }
    }

    while ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }

    ring
}

fn canonicalize_ring(ring: CanonicalRing) -> CanonicalRing {
    if ring.is_empty() {
        return ring;
    }

    let mut candidates = rotations(&ring);
    let mut reversed = ring;
    reversed.reverse();
    candidates.extend(rotations(&reversed));
    candidates.into_iter().min().unwrap()
}

fn remove_redundant_collinear_points(ring: &mut CanonicalRing) {
    loop {
        if ring.len() < 3 {
            return;
        }

        let mut filtered = Vec::with_capacity(ring.len());
        let len = ring.len();

        for index in 0..len {
            let previous = ring[(index + len - 1) % len];
            let current = ring[index];
            let next = ring[(index + 1) % len];

            if !is_redundant_collinear(previous, current, next) {
                filtered.push(current);
            }
        }

        if filtered.len() == ring.len() {
            return;
        }

        *ring = filtered;
    }
}

fn is_redundant_collinear(previous: (i32, i32), current: (i32, i32), next: (i32, i32)) -> bool {
    let ax = current.0 as i64 - previous.0 as i64;
    let ay = current.1 as i64 - previous.1 as i64;
    let bx = next.0 as i64 - current.0 as i64;
    let by = next.1 as i64 - current.1 as i64;

    ax * by - ay * bx == 0 && ax * bx + ay * by >= 0
}

fn rotations(ring: &CanonicalRing) -> Vec<CanonicalRing> {
    (0..ring.len())
        .map(|start| {
            ring[start..]
                .iter()
                .chain(&ring[..start])
                .copied()
                .collect()
        })
        .collect()
}

fn point(x: i32, y: i32) -> Vec2I {
    Vec2I::new(x, y)
}

fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<Vec2I> {
    vec![point(x0, y0), point(x1, y0), point(x1, y1), point(x0, y1)]
}
