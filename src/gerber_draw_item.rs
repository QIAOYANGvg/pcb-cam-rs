/// Gerber draw item — a single drawable element.
/// Ported from KiCad gerber_draw_item.h / gerber_draw_item.cpp
use crate::dcode::DCode;
use crate::geometry::{
    Box2I, PolySet, Vec2I, add, angle_degrees, distance, ki_round, rotate_point, sub,
};
use crate::netlist_metadata::NetlistMetadata;
use crate::types::{ApertureType, ShapeType};

/// A single drawable element parsed from a Gerber file.
///
/// Can be a line segment, arc, circle, polygon, or a flashed aperture shape.
#[derive(Clone, Debug)]
pub struct DrawItem {
    /// Shape type of this gerber item
    pub shape_type: ShapeType,

    /// Line or arc start point, or position of the shape for flashed items
    pub start: Vec2I,

    /// Line or arc end point
    pub end: Vec2I,

    /// For arcs only: center of arc
    pub arc_centre: Vec2I,

    /// Flashed shapes: size of the shape. Lines: m_Size.x = m_Size.y = line width
    pub size: Vec2I,

    /// True for flashed items
    pub flashed: bool,

    /// D-code used to draw this item (>= 10). 0 for regions (polygons).
    pub dcode: i32,

    /// Aperture function from %TA.AperFunction
    pub aper_function: String,

    /// Polygon shape data from G36/G37 region commands
    pub shape_as_polygon: Vec<Vec<Vec2I>>,

    /// The polygon to draw, in absolute coordinates
    pub absolute_polygon: Vec<Vec<Vec2I>>,

    /// Flashed aperture macro shape in transformed absolute coordinates.
    pub macro_shape_polygon: PolySet,

    /// Net attributes from %TO
    pub net_attributes: NetlistMetadata,

    // Layer parameter snapshot (can change mid-file, so stored per item)
    /// Gerber units (inch/mm)
    pub units_metric: bool,

    /// true = item in negative layer
    pub layer_negative: bool,

    /// false if A=X, B=Y; true if A=Y, B=X
    pub swap_axis: bool,

    /// true: mirror axis A
    pub mirror_a: bool,

    /// true: mirror axis B
    pub mirror_b: bool,

    /// A and B scaling factor
    pub draw_scale: (f64, f64),

    /// Offset for A and B axis, from OF parameter
    pub layer_offset: Vec2I,

    /// Fine rotation in degrees
    pub lyr_rotation: f64,

    /// Image justify offset from IJ command
    pub image_justify_offset: Vec2I,

    /// Image offset from IO command
    pub image_offset: Vec2I,

    /// Image rotation from IR command, in degrees
    pub image_rotation: i32,

    /// Display offset
    pub display_offset: Vec2I,

    /// Display rotation, in degrees
    pub display_rotation: f64,
}

impl DrawItem {
    pub fn new() -> Self {
        Self {
            shape_type: ShapeType::Segment,
            start: Vec2I::new(0, 0),
            end: Vec2I::new(0, 0),
            arc_centre: Vec2I::new(0, 0),
            size: Vec2I::new(0, 0),
            flashed: false,
            dcode: 0,
            aper_function: String::new(),
            shape_as_polygon: Vec::new(),
            absolute_polygon: Vec::new(),
            macro_shape_polygon: PolySet::new(),
            net_attributes: NetlistMetadata::default(),
            units_metric: false,
            layer_negative: false,
            swap_axis: false,
            mirror_a: false,
            mirror_b: false,
            draw_scale: (1.0, 1.0),
            layer_offset: Vec2I::new(0, 0),
            lyr_rotation: 0.0,
            image_justify_offset: Vec2I::new(0, 0),
            image_offset: Vec2I::new(0, 0),
            image_rotation: 0,
            display_offset: Vec2I::new(0, 0),
            display_rotation: 0.0,
        }
    }

    /// Get the layer polarity
    pub fn get_layer_polarity(&self) -> bool {
        self.layer_negative
    }

    /// Set the layer polarity
    pub fn set_layer_polarity(&mut self, negative: bool) {
        self.layer_negative = negative;
    }

    /// Get the position (start point)
    pub fn get_position(&self) -> Vec2I {
        self.start
    }

    /// Set the position
    pub fn set_position(&mut self, pos: Vec2I) {
        self.start = pos;
    }

    /// Get the image position of a position for this item.
    ///
    /// Image position is the value modified by image parameters:
    /// offsets, axis selection, scale, rotation.
    ///
    /// @param xy_position is position in X,Y gerber axis
    /// @return the given position in plotter A,B axis
    pub fn get_ab_position(&self, xy_position: Vec2I) -> Vec2I {
        let mut pos = add(xy_position, self.image_justify_offset);

        if self.swap_axis {
            std::mem::swap(&mut pos.x, &mut pos.y);
        }

        pos = add(pos, add(self.layer_offset, self.image_offset));
        pos.x = ki_round(pos.x as f64 * self.draw_scale.0);
        pos.y = ki_round(pos.y as f64 * self.draw_scale.1);

        let rotation = self.lyr_rotation + self.image_rotation as f64;
        if rotation != 0.0 {
            pos = rotate_point(pos, -rotation);
        }

        if self.mirror_a {
            pos.x = -pos.x;
        }

        if !self.mirror_b {
            pos.y = -pos.y;
        }

        if self.display_rotation != 0.0 {
            pos = rotate_point(pos, self.display_rotation);
        }

        pos.x += ki_round(self.display_offset.x as f64 * self.draw_scale.0);
        pos.y += ki_round(self.display_offset.y as f64 * self.draw_scale.1);

        pos
    }

    pub fn convert_segment_to_polygon(&self) -> PolySet {
        let mut poly = PolySet::new();
        let mut start = self.start;
        let mut end = self.end;

        if start.x > end.x {
            std::mem::swap(&mut start, &mut end);
        }

        let mut delta = sub(end, start);
        let change = delta.y < 0;

        if change {
            delta.y = -delta.y;
        }

        let mut corner = Vec2I::new(-self.size.x / 2, -self.size.y / 2);
        let close = corner;
        let mut outline = Vec::new();
        outline.push(corner);
        corner.y += self.size.y;
        outline.push(corner);

        if delta.x != 0 || delta.y != 0 {
            corner = add(corner, delta);
            outline.push(corner);
        }

        corner.x += self.size.x;
        outline.push(corner);
        corner.y -= self.size.y;
        outline.push(corner);

        if delta.x != 0 || delta.y != 0 {
            corner = sub(corner, delta);
            outline.push(corner);
        }

        outline.push(close);

        if change {
            for point in &mut outline {
                point.y = -point.y;
            }
        }

        for point in &mut outline {
            *point = add(*point, start);
        }

        poly.add_outline(outline);
        poly
    }

    pub fn shape_as_polyset(&self) -> PolySet {
        PolySet::from_outlines(&self.shape_as_polygon)
    }

    pub fn get_bounding_box(&self, dcode: Option<&DCode>) -> Box2I {
        let mut bbox = Box2I::new(self.start, Vec2I::new(1, 1));

        match self.shape_type {
            ShapeType::Polygon => {
                if let Some(poly_bbox) = self.shape_as_polyset().bbox() {
                    bbox.inflate(poly_bbox.width() / 2, poly_bbox.height() / 2);
                    bbox.origin = poly_bbox.origin;
                }
            }
            ShapeType::Circle => {
                let radius = distance(self.start, self.end).round() as i32;
                bbox.inflate(radius, radius);
            }
            ShapeType::Arc => {
                bbox = self.arc_bounding_box();
                bbox.inflate(self.size.x / 2, self.size.x / 2);
            }
            ShapeType::SpotCircle => {
                if let Some(code) = dcode {
                    let radius = code.size.x >> 1;
                    bbox.inflate(radius, radius);
                }
            }
            ShapeType::SpotRect | ShapeType::SpotOval => {
                if let Some(code) = dcode {
                    bbox.inflate(code.size.x / 2, code.size.y / 2);
                }
            }
            ShapeType::SpotMacro => {
                if let Some(code) = dcode {
                    if let Some(bb) = code
                        .polyset
                        .bbox()
                        .or_else(|| self.macro_shape_polygon.bbox())
                    {
                        bbox.inflate(bb.width() / 2, bb.height() / 2);
                    }
                }
            }
            ShapeType::SpotPoly => {
                if let Some(code) = dcode {
                    if let Some(bb) = code.polyset.bbox() {
                        bbox.inflate(bb.width() / 2, bb.height() / 2);
                    }
                }
            }
            ShapeType::Segment => {
                if let Some(code) = dcode {
                    if code.apert_type == ApertureType::Rect {
                        if self.shape_as_polygon.is_empty() {
                            bbox = self.convert_segment_to_polygon().bbox().unwrap_or(bbox);
                        } else if let Some(poly_bbox) = self.shape_as_polyset().bbox() {
                            bbox = poly_bbox;
                        }
                    } else {
                        bbox = segment_box(self.start, self.end, (self.size.x + 1) / 2);
                    }
                } else {
                    bbox = segment_box(self.start, self.end, (self.size.x + 1) / 2);
                }
            }
        }

        self.transform_bbox(bbox)
    }

    fn arc_bounding_box(&self) -> Box2I {
        let mut included_angle = if self.end == self.start {
            360.0
        } else {
            normalize_angle(
                angle_degrees(sub(self.end, self.arc_centre))
                    - angle_degrees(sub(self.start, self.arc_centre)),
            )
        };

        let mid = rotate_point_around(self.start, self.arc_centre, included_angle / 2.0);
        let arc_end = rotate_point_around(self.start, self.arc_centre, included_angle);
        let center = calc_arc_center(self.start, mid, arc_end);
        let radius = distance(center, self.start).round() as i32;
        let mut start_angle = normalize_angle(angle_degrees(sub(self.start, center)));

        included_angle = if arc_end == self.start {
            360.0
        } else {
            normalize_angle(
                angle_degrees(sub(arc_end, center)) - angle_degrees(sub(self.start, center)),
            )
        };

        let mut end_angle = start_angle + included_angle;

        if start_angle > end_angle {
            std::mem::swap(&mut start_angle, &mut end_angle);
        }

        let mut bbox = Box2I::new(self.start, Vec2I::new(0, 0));
        bbox.merge_point(arc_end);

        let quad_angle_start = (start_angle / 90.0).ceil() as i32;
        let quad_angle_end = (end_angle / 90.0).floor() as i32;

        if (radius as f64) < (i32::MAX as f64 / 2.0) {
            for quad_angle in quad_angle_start..=quad_angle_end {
                let quad_pt = match quad_angle % 4 {
                    0 => add(center, Vec2I::new(radius, 0)),
                    1 | -3 => add(center, Vec2I::new(0, radius)),
                    2 | -2 => add(center, Vec2I::new(-radius, 0)),
                    3 | -1 => add(center, Vec2I::new(0, -radius)),
                    _ => unreachable!(),
                };

                let near_start = squared_distance(quad_pt, self.start) == 4;
                let near_end = squared_distance(quad_pt, arc_end) == 4;
                let is_left_cardinal = quad_angle.rem_euclid(4) == 2;

                if !is_left_cardinal || (!near_start && !near_end) {
                    bbox.merge_point(quad_pt);
                }
            }
        }

        bbox
    }

    fn transform_bbox(&self, mut bbox: Box2I) -> Box2I {
        bbox.normalize();
        let origin = bbox.origin;
        let end = bbox.end();
        let corners = [
            origin,
            Vec2I::new(end.x, origin.y),
            end,
            Vec2I::new(origin.x, end.y),
        ];
        let mut result = Box2I::new(self.get_ab_position(origin), Vec2I::new(0, 0));

        for corner in corners {
            result.merge_point(self.get_ab_position(corner));
        }

        result.normalize();
        result
    }

    /// Get the X,Y position from an A/B position (inverse of get_ab_position)
    pub fn get_xy_position(&self, ab_position: Vec2I) -> Vec2I {
        let mut pos = ab_position;

        pos.x -= ki_round(self.display_offset.x as f64 * self.draw_scale.0);
        pos.y -= ki_round(self.display_offset.y as f64 * self.draw_scale.1);

        if self.display_rotation != 0.0 {
            pos = rotate_point(pos, -self.display_rotation);
        }

        if self.mirror_a {
            pos.x = -pos.x;
        }

        if !self.mirror_b {
            pos.y = -pos.y;
        }

        let rotation = self.lyr_rotation + self.image_rotation as f64;
        if rotation != 0.0 {
            pos = rotate_point(pos, rotation);
        }

        if self.draw_scale.0 != 0.0 {
            pos.x = ki_round(pos.x as f64 / self.draw_scale.0);
        }

        if self.draw_scale.1 != 0.0 {
            pos.y = ki_round(pos.y as f64 / self.draw_scale.1);
        }

        pos = sub(pos, add(self.layer_offset, self.image_offset));

        if self.swap_axis {
            std::mem::swap(&mut pos.x, &mut pos.y);
        }

        sub(pos, self.image_justify_offset)
    }
}

fn segment_box(start: Vec2I, end: Vec2I, radius: i32) -> Box2I {
    let ymax = start.y.max(end.y) + radius;
    let xmax = start.x.max(end.x) + radius;
    let ymin = start.y.min(end.y) - radius;
    let xmin = start.x.min(end.x) - radius;

    Box2I::new(
        Vec2I::new(xmin, ymin),
        Vec2I::new(xmax - xmin + 1, ymax - ymin + 1),
    )
}

fn normalize_angle(mut angle: f64) -> f64 {
    while angle < -0.0 {
        angle += 360.0;
    }

    while angle >= 360.0 {
        angle -= 360.0;
    }

    angle
}

fn rotate_point_around(point: Vec2I, center: Vec2I, angle_degrees: f64) -> Vec2I {
    add(center, rotate_point(sub(point, center), angle_degrees))
}

fn squared_distance(a: Vec2I, b: Vec2I) -> i64 {
    let dx = a.x as i64 - b.x as i64;
    let dy = a.y as i64 - b.y as i64;
    dx * dx + dy * dy
}

fn calc_arc_center(start: Vec2I, mid: Vec2I, end: Vec2I) -> Vec2I {
    let start_x = start.x as f64;
    let start_y = start.y as f64;
    let mid_x = mid.x as f64;
    let mid_y = mid.y as f64;
    let end_x = end.x as f64;
    let end_y = end.y as f64;

    let y_delta_21 = mid_y - start_y;
    let mut x_delta_21 = mid_x - start_x;
    let y_delta_32 = end_y - mid_y;
    let mut x_delta_32 = end_x - mid_x;

    if (x_delta_21 == 0.0 && y_delta_32 == 0.0) || (y_delta_21 == 0.0 && x_delta_32 == 0.0) {
        return Vec2I::new(
            ki_round((start_x + end_x) / 2.0),
            ki_round((start_y + end_y) / 2.0),
        );
    }

    if x_delta_21 == 0.0 {
        x_delta_21 = f64::EPSILON;
    }

    if x_delta_32 == 0.0 {
        x_delta_32 = -f64::EPSILON;
    }

    let mut a_slope = y_delta_21 / x_delta_21;
    let mut b_slope = y_delta_32 / x_delta_32;
    let da_slope = a_slope * (0.5 / y_delta_21).hypot(0.5 / x_delta_21);
    let db_slope = b_slope * (0.5 / y_delta_32).hypot(0.5 / x_delta_32);

    if a_slope == b_slope {
        if start == end {
            return Vec2I::new(
                ki_round((start_x + mid_x) / 2.0),
                ki_round((start_y + mid_y) / 2.0),
            );
        } else {
            a_slope += f64::EPSILON;
            b_slope -= f64::EPSILON;
        }
    }

    if a_slope == 0.0 {
        a_slope = 1e-10;
    }

    if b_slope == 0.0 {
        b_slope = 1e-10;
    }

    let sqrt_1_2 = std::f64::consts::FRAC_1_SQRT_2;
    let ab_slope_start_end_y = a_slope * b_slope * (start_y - end_y);
    let dab_slope_start_end_y = ab_slope_start_end_y
        * ((da_slope / a_slope * da_slope / a_slope)
            + (db_slope / b_slope * db_slope / b_slope)
            + (sqrt_1_2 / (start_y - end_y) * sqrt_1_2 / (start_y - end_y)))
            .sqrt();

    let b_slope_start_mid_x = b_slope * (start_x + mid_x);
    let db_slope_start_mid_x = b_slope_start_mid_x
        * ((db_slope / b_slope * db_slope / b_slope)
            + (sqrt_1_2 / (start_x + mid_x) * sqrt_1_2 / (start_x + mid_x)))
            .sqrt();

    let a_slope_mid_end_x = a_slope * (mid_x + end_x);
    let da_slope_mid_end_x = a_slope_mid_end_x
        * ((da_slope / a_slope * da_slope / a_slope)
            + (sqrt_1_2 / (mid_x + end_x) * sqrt_1_2 / (mid_x + end_x)))
            .sqrt();

    let twice_ba_slope_diff = 2.0 * (b_slope - a_slope);
    let dtwice_ba_slope_diff = 2.0 * (db_slope * db_slope + da_slope * da_slope).sqrt();
    let center_numerator_x = ab_slope_start_end_y + b_slope_start_mid_x - a_slope_mid_end_x;
    let dcenter_numerator_x = (dab_slope_start_end_y * dab_slope_start_end_y
        + db_slope_start_mid_x * db_slope_start_mid_x
        + da_slope_mid_end_x * da_slope_mid_end_x)
        .sqrt();

    let center_x = center_numerator_x / twice_ba_slope_diff;
    let dcenter_x = center_x
        * ((dcenter_numerator_x / center_numerator_x * dcenter_numerator_x / center_numerator_x)
            + (dtwice_ba_slope_diff / twice_ba_slope_diff * dtwice_ba_slope_diff
                / twice_ba_slope_diff))
            .sqrt();

    let center_numerator_y = (start_x + mid_x) / 2.0 - center_x;
    let dcenter_numerator_y = (1.0 / 8.0 + dcenter_x * dcenter_x).sqrt();
    let center_first_term = center_numerator_y / a_slope;
    let dcenter_first_term_y = center_first_term
        * ((dcenter_numerator_y / center_numerator_y * dcenter_numerator_y / center_numerator_y)
            + (da_slope / a_slope * da_slope / a_slope))
            .sqrt();

    let center_y = center_first_term + (start_y + mid_y) / 2.0;
    let dcenter_y = (dcenter_first_term_y * dcenter_first_term_y + 1.0 / 8.0).sqrt();
    let rounded100_center_x = ((center_x + 50.0) / 100.0).floor() * 100.0;
    let rounded100_center_y = ((center_y + 50.0) / 100.0).floor() * 100.0;
    let rounded10_center_x = ((center_x + 5.0) / 10.0).floor() * 10.0;
    let rounded10_center_y = ((center_y + 5.0) / 10.0).floor() * 10.0;

    let (center_x, center_y) = if (rounded100_center_x - center_x).abs() < dcenter_x
        && (rounded100_center_y - center_y).abs() < dcenter_y
    {
        (rounded100_center_x, rounded100_center_y)
    } else if (rounded10_center_x - center_x).abs() < dcenter_x
        && (rounded10_center_y - center_y).abs() < dcenter_y
    {
        (rounded10_center_x, rounded10_center_y)
    } else {
        (center_x, center_y)
    };

    Vec2I::new(ki_round(center_x), ki_round(center_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_ab_position_uses_kicad_transform_order() {
        let mut item = DrawItem::new();
        item.image_justify_offset = Vec2I::new(1, 2);
        item.swap_axis = true;
        item.layer_offset = Vec2I::new(3, 4);
        item.image_offset = Vec2I::new(5, 6);
        item.draw_scale = (2.0, 3.0);
        item.mirror_a = true;
        item.mirror_b = false;
        item.display_offset = Vec2I::new(7, 8);

        assert_eq!(
            item.get_ab_position(Vec2I::new(10, 20)),
            Vec2I::new(-46, -39)
        );
    }

    #[test]
    fn get_xy_position_inverts_get_ab_position_for_kicad_order() {
        let mut item = DrawItem::new();
        item.image_justify_offset = Vec2I::new(1, 2);
        item.swap_axis = true;
        item.layer_offset = Vec2I::new(3, 4);
        item.image_offset = Vec2I::new(5, 6);
        item.draw_scale = (2.0, 3.0);
        item.mirror_a = true;
        item.mirror_b = false;
        item.display_offset = Vec2I::new(7, 8);

        let xy = Vec2I::new(10, 20);
        assert_eq!(item.get_xy_position(item.get_ab_position(xy)), xy);
    }

    #[test]
    fn arc_bbox_uses_near_cardinal_endpoint_instead_of_rounding_overshoot() {
        let mut item = DrawItem::new();
        item.shape_type = ShapeType::Arc;
        item.start = Vec2I::new(8_984_512, -5_997_057);
        item.end = Vec2I::new(8_982_861, -6_007_100);
        item.arc_centre = Vec2I::new(9_014_230, -6_007_100);
        item.size = Vec2I::new(20_320, 20_320);

        assert_eq!(
            item.get_bounding_box(None),
            Box2I::new(Vec2I::new(8_972_701, 5_986_897), Vec2I::new(21_971, 30_363))
        );
    }
}
