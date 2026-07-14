#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Vec2I {
    pub x: i32,
    pub y: i32,
}

impl Vec2I {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
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
