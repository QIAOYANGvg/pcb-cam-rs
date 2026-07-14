use super::vector::{rotate_point, Vec2I};

const MIN_SEGCOUNT_FOR_CIRCLE: f64 = 8.0;

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
