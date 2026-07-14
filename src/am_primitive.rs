/// Aperture macro primitive definitions.
/// Ported from KiCad am_primitive.h / am_primitive.cpp
///
/// Aperture macro primitives are basic shapes which can be combined to create
/// a complex shape that is flashed.
use crate::am_param::AmParam;
use crate::coord::{Vec2I, scale_to_iu};
use crate::geometry::{
    PolySet, add, circle_to_polygon_by_error, euclidean_norm, rotate_point, sub,
};

/// Aperture macro primitive IDs
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AmPrimitiveId {
    /// Not a primitive, this is a comment
    #[default]
    Comment = 0,
    /// Circle (diameter and position)
    Circle = 1,
    /// Line with rectangle ends (width, start and end pos + rotation)
    Line2 = 2,
    /// Same as Line2
    Line20 = 20,
    /// Rectangle (height, width and center pos + rotation)
    LineCenter = 21,
    /// Rectangle (height, width and left bottom corner pos + rotation)
    LineLowerLeft = 22,
    /// Free polyline (n corners + rotation)
    Outline = 4,
    /// Closed regular polygon (diameter, number of vertices 3-10, rotation)
    Polygon = 5,
    /// Cross hair with n concentric circles + rotation (deprecated in 2021)
    Moire = 6,
    /// Thermal shape (pos, outer and inner diameter, cross hair thickness + rotation)
    Thermal = 7,
}

impl AmPrimitiveId {
    pub fn from_i32(val: i32) -> Self {
        match val {
            0 => Self::Comment,
            1 => Self::Circle,
            2 => Self::Line2,
            20 => Self::Line20,
            21 => Self::LineCenter,
            22 => Self::LineLowerLeft,
            4 => Self::Outline,
            5 => Self::Polygon,
            6 => Self::Moire,
            7 => Self::Thermal,
            _ => Self::Comment,
        }
    }
}

/// An aperture macro primitive as defined in the Gerber specification.
///
/// Each primitive defines a simple shape (circle, rect, regular polygon...)
/// with a fixed list of parameters defining size, thickness, number of vertices, etc.
///
/// Each basic shape can be positive or negative. A negative shape is local to
/// the whole shape and acts as a hole, not a standard negative object.
#[derive(Clone, Debug)]
pub struct AmPrimitive {
    /// The primitive type
    pub primitive_id: AmPrimitiveId,

    /// Parameters used by the primitive, preserved as expressions for deferred `$n` evaluation.
    pub params: Vec<AmParam>,

    /// Units for this primitive: false = Inches, true = metric
    pub gerb_metric: bool,

    /// Count of local params defined inside the aperture macro when this primitive
    /// was added to the macro's primitive stack list.
    /// Used for deferred evaluation of local parameters.
    pub local_param_level: i32,
}

impl AmPrimitive {
    pub fn new(gerb_metric: bool, id: AmPrimitiveId) -> Self {
        Self {
            primitive_id: id,
            params: Vec::new(),
            gerb_metric,
            local_param_level: 0,
        }
    }

    /// Returns true if the first parameter is not 0 (exposure control).
    /// Some but not all primitives use the first parameter as exposure control.
    /// Others are always ON.
    /// In an aperture macro shape, a primitive with exposure off is a hole in the shape.
    pub fn is_exposure_on(&self, macro_params: &[f64], default_on: bool) -> bool {
        match self.primitive_id {
            AmPrimitiveId::Circle
            | AmPrimitiveId::Line2
            | AmPrimitiveId::Line20
            | AmPrimitiveId::LineCenter
            | AmPrimitiveId::LineLowerLeft
            | AmPrimitiveId::Outline
            | AmPrimitiveId::Polygon => self
                .param(0, macro_params)
                .map_or(default_on, |value| value != 0.0),
            // Moire and Thermal are always on
            AmPrimitiveId::Moire | AmPrimitiveId::Thermal => true,
            AmPrimitiveId::Comment => false,
        }
    }

    pub fn convert_basic_shape_to_polygon(&self, macro_params: &[f64], shape: &mut PolySet) {
        let mut polybuffer = self.convert_shape_to_polygon(macro_params);

        match self.primitive_id {
            AmPrimitiveId::Circle => {
                if let Some(rotation) = self.param(4, macro_params) {
                    rotate_points(&mut polybuffer, rotation);
                }
            }
            AmPrimitiveId::Line2 | AmPrimitiveId::Line20 => {
                rotate_points(&mut polybuffer, self.param_or(6, macro_params));
            }
            AmPrimitiveId::LineCenter | AmPrimitiveId::LineLowerLeft => {
                rotate_points(&mut polybuffer, self.param_or(5, macro_params));
            }
            AmPrimitiveId::Thermal => {
                let subshape = polybuffer;
                let center = self.map_pt(0, 1, macro_params);
                let rotation = self.param_or(5, macro_params);

                for ii in 0..4 {
                    let mut outline = subshape.clone();
                    rotate_points(&mut outline, 90.0 * ii as f64);
                    for point in &mut outline {
                        *point = add(*point, center);
                        *point = rotate_point(*point, rotation);
                    }
                    close_and_add(shape, outline);
                }
                return;
            }
            AmPrimitiveId::Moire => {
                self.add_moire_rings(macro_params, shape);
                let center = self.map_pt(0, 1, macro_params);
                let rotation = self.param_or(8, macro_params);
                for point in &mut polybuffer {
                    *point = add(*point, center);
                    *point = rotate_point(*point, rotation);
                }
            }
            AmPrimitiveId::Outline => {
                let rotation = self
                    .params
                    .last()
                    .map(|param| param.get_value_from_macro(macro_params))
                    .unwrap_or(0.0);
                rotate_points(&mut polybuffer, rotation);
            }
            AmPrimitiveId::Polygon => {
                let center = self.map_pt(2, 3, macro_params);
                let rotation = self.param_or(5, macro_params);
                for point in &mut polybuffer {
                    *point = add(*point, center);
                    *point = rotate_point(*point, rotation);
                }
            }
            AmPrimitiveId::Comment => {}
        }

        close_and_add(shape, polybuffer);
    }

    fn convert_shape_to_polygon(&self, macro_params: &[f64]) -> Vec<Vec2I> {
        match self.primitive_id {
            AmPrimitiveId::Circle => {
                let radius = scale_to_iu(self.param_or(1, macro_params), self.gerb_metric) / 2;
                if radius <= 0 {
                    Vec::new()
                } else {
                    let center = self.map_pt(2, 3, macro_params);
                    let mut outline = Vec::with_capacity(64);

                    for ii in 0..64 {
                        let point =
                            rotate_point(Vec2I::new(radius, 0), -(360.0 * ii as f64 / 64.0));
                        outline.push(add(point, center));
                    }

                    outline
                }
            }
            AmPrimitiveId::Line2 | AmPrimitiveId::Line20 => {
                let width = scale_to_iu(self.param_or(1, macro_params), self.gerb_metric);
                let start = self.map_pt(2, 3, macro_params);
                let end = self.map_pt(4, 5, macro_params);
                let delta = sub(end, start);
                let len = euclidean_norm(delta);
                let mut outline = vec![
                    Vec2I::new(0, width / 2),
                    Vec2I::new(len, width / 2),
                    Vec2I::new(len, -width / 2),
                    Vec2I::new(0, -width / 2),
                ];
                let angle = (delta.y as f64).atan2(delta.x as f64).to_degrees();
                rotate_points(&mut outline, angle);
                for point in &mut outline {
                    *point = add(*point, start);
                }
                outline
            }
            AmPrimitiveId::LineCenter => {
                let size = self.map_pt(1, 2, macro_params);
                let mut pos = self.map_pt(3, 4, macro_params);
                pos.x -= size.x / 2;
                pos.y -= size.y / 2;

                let mut outline = Vec::with_capacity(4);
                outline.push(pos);
                pos.y += size.y;
                outline.push(pos);
                pos.x += size.x;
                outline.push(pos);
                pos.y -= size.y;
                outline.push(pos);
                outline
            }
            AmPrimitiveId::LineLowerLeft => {
                let size = self.map_pt(1, 2, macro_params);
                let lower_left = self.map_pt(3, 4, macro_params);
                vec![
                    lower_left,
                    add(lower_left, Vec2I::new(0, size.y)),
                    add(lower_left, size),
                    add(lower_left, Vec2I::new(size.x, 0)),
                ]
            }
            AmPrimitiveId::Thermal => self.thermal_quadrant(macro_params),
            AmPrimitiveId::Moire => self.moire_cross(macro_params),
            AmPrimitiveId::Outline => self.outline_points(macro_params),
            AmPrimitiveId::Polygon => {
                let mut vertex_count = self.param_or(1, macro_params).round() as i32;
                let radius = scale_to_iu(self.param_or(4, macro_params), self.gerb_metric) / 2;

                if vertex_count < 3 {
                    vertex_count = 3;
                }

                if vertex_count > 10 {
                    vertex_count = 10;
                }

                let mut outline = Vec::with_capacity(vertex_count as usize + 1);

                for ii in 0..=vertex_count {
                    outline.push(rotate_point(
                        Vec2I::new(radius, 0),
                        -(360.0 * ii as f64 / vertex_count as f64),
                    ));
                }

                outline
            }
            AmPrimitiveId::Comment => Vec::new(),
        }
    }

    fn thermal_quadrant(&self, macro_params: &[f64]) -> Vec<Vec2I> {
        let outer_radius =
            (scale_to_iu(self.param_or(2, macro_params), self.gerb_metric) / 2).max(1);
        let inner_radius =
            (scale_to_iu(self.param_or(3, macro_params), self.gerb_metric) / 2).max(1);
        let half_thickness = scale_to_iu(self.param_or(4, macro_params), self.gerb_metric) / 2;
        let mut outline = Vec::new();
        let mut angle_start = ((half_thickness as f64 / inner_radius as f64).clamp(-1.0, 1.0))
            .asin()
            .to_degrees();
        let mut angle_end = 90.0 - angle_start;

        let inner_start = Vec2I::new(inner_radius, 0);
        let mut angle = angle_start;
        while angle < angle_end {
            outline.push(rotate_point(inner_start, -angle));
            angle += 10.0;
        }
        outline.push(rotate_point(inner_start, -angle_end));

        let outer_start = Vec2I::new(outer_radius, 0);
        angle_start = ((half_thickness as f64 / outer_radius as f64).clamp(-1.0, 1.0))
            .asin()
            .to_degrees();
        angle_end = 90.0 - angle_start;
        angle = angle_end;
        while angle > angle_start {
            outline.push(rotate_point(outer_start, -angle));
            angle -= 10.0;
        }
        outline.push(rotate_point(outer_start, -angle_start));
        if let Some(first) = outline.first().copied() {
            outline.push(first);
        }
        outline
    }

    fn moire_cross(&self, macro_params: &[f64]) -> Vec<Vec2I> {
        let thickness = scale_to_iu(self.param_or(6, macro_params), self.gerb_metric);
        let length = scale_to_iu(self.param_or(7, macro_params), self.gerb_metric);
        let mut outline = vec![
            Vec2I::new(thickness / 2, length / 2),
            Vec2I::new(thickness / 2, thickness / 2),
            Vec2I::new(-length / 2, thickness / 2),
            Vec2I::new(-length / 2, -thickness / 2),
        ];

        for jj in 1..=3 {
            for ii in 0..4 {
                outline.push(rotate_point(outline[ii], -(90.0 * jj as f64)));
            }
        }

        outline
    }

    fn add_moire_rings(&self, macro_params: &[f64], shape: &mut PolySet) {
        let mut outer_diam = scale_to_iu(self.param_or(2, macro_params), self.gerb_metric);
        let pen_thickness = scale_to_iu(self.param_or(3, macro_params), self.gerb_metric);
        let gap = scale_to_iu(self.param_or(4, macro_params), self.gerb_metric);
        let num_circles = self.param_or(5, macro_params).round() as i32;
        let center = rotate_point(
            self.map_pt(0, 1, macro_params),
            self.param_or(8, macro_params),
        );
        let diam_adjust = (gap + pen_thickness) * 2;

        for _ in 0..num_circles {
            if outer_diam <= 0 {
                break;
            }

            if outer_diam <= pen_thickness {
                shape.add_outline(
                    circle_to_polygon_by_error(outer_diam / 2, 500)
                        .into_iter()
                        .map(|point| add(point, center))
                        .collect(),
                );
            } else {
                let mut ring = PolySet::new();
                ring.add_outline(
                    circle_to_polygon_by_error(outer_diam / 2, 500)
                        .into_iter()
                        .map(|point| add(point, center))
                        .collect(),
                );
                let mut hole = PolySet::new();
                hole.add_outline(
                    circle_to_polygon_by_error((outer_diam - pen_thickness * 2) / 2, 500)
                        .into_iter()
                        .map(|point| add(point, center))
                        .collect(),
                );
                ring.boolean_subtract(&hole);
                ring.fracture();
                shape.append_polyset(&ring);
            }

            outer_diam -= diam_adjust;
        }
    }

    fn outline_points(&self, macro_params: &[f64]) -> Vec<Vec2I> {
        let num_corners = self.param_or(1, macro_params) as i32;
        let last_prm = self.params.len().saturating_sub(1);
        let mut result = Vec::new();
        let mut prm_idx = 2;

        for _ in 0..=num_corners {
            if prm_idx >= self.params.len() {
                break;
            }

            result.push(self.map_pt(prm_idx, prm_idx + 1, macro_params));
            prm_idx += 2;

            if prm_idx >= last_prm {
                break;
            }
        }

        result
    }

    fn map_pt(&self, x_idx: usize, y_idx: usize, macro_params: &[f64]) -> Vec2I {
        Vec2I::new(
            scale_to_iu(self.param_or(x_idx, macro_params), self.gerb_metric),
            scale_to_iu(self.param_or(y_idx, macro_params), self.gerb_metric),
        )
    }

    fn param(&self, idx: usize, macro_params: &[f64]) -> Option<f64> {
        self.params
            .get(idx)
            .map(|param| param.get_value_from_macro(macro_params))
    }

    fn param_or(&self, idx: usize, macro_params: &[f64]) -> f64 {
        self.param(idx, macro_params).unwrap_or(0.0)
    }
}

fn rotate_points(points: &mut [Vec2I], angle: f64) {
    for point in points {
        *point = rotate_point(*point, angle);
    }
}

fn close_and_add(shape: &mut PolySet, mut outline: Vec<Vec2I>) {
    if outline.len() > 1 {
        let first = outline[0];
        outline.push(first);
        shape.add_outline(outline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::am_param::{AmParamItem, ParmItemType};

    fn value_param(value: f64) -> AmParam {
        AmParam {
            index: 0,
            param_stack: vec![AmParamItem {
                item_type: ParmItemType::PushValue,
                dvalue: value,
                ivalue: 0,
            }],
        }
    }

    fn primitive(id: AmPrimitiveId, params: &[f64]) -> AmPrimitive {
        let mut primitive = AmPrimitive::new(true, id);
        primitive.params = params.iter().copied().map(value_param).collect();
        primitive
    }

    #[test]
    fn circle_polygon_starts_on_positive_x_axis_like_kicad() {
        let primitive = primitive(AmPrimitiveId::Circle, &[1.0, 1.0, 0.0, 0.0]);
        let points = primitive.convert_shape_to_polygon(&[]);

        assert_eq!(points.len(), 64);
        assert_eq!(points[0], Vec2I::new(50_000, 0));
    }

    #[test]
    fn line_center_uses_kicad_corner_order() {
        let primitive = primitive(AmPrimitiveId::LineCenter, &[1.0, 2.0, 4.0, 10.0, 20.0, 0.0]);
        let points = primitive.convert_shape_to_polygon(&[]);

        assert_eq!(
            points,
            vec![
                Vec2I::new(900_000, 1_800_000),
                Vec2I::new(900_000, 2_200_000),
                Vec2I::new(1_100_000, 2_200_000),
                Vec2I::new(1_100_000, 1_800_000),
            ]
        );
    }

    #[test]
    fn polygon_shape_is_closed_before_basic_shape_closes_again() {
        let primitive = primitive(AmPrimitiveId::Polygon, &[1.0, 4.0, 0.0, 0.0, 2.0, 0.0]);
        let mut shape = PolySet::new();

        primitive.convert_basic_shape_to_polygon(&[], &mut shape);

        let outline = &shape.polygons[0].outline;
        assert_eq!(outline.len(), 6);
        assert_eq!(outline[0], Vec2I::new(100_000, 0));
        assert_eq!(outline[4], outline[0]);
        assert_eq!(outline[5], outline[0]);
    }

    #[test]
    fn outline_pushes_point_before_malformed_guard() {
        let primitive = primitive(
            AmPrimitiveId::Outline,
            &[1.0, 3.0, 0.0, 0.0, 1.0, 0.0, 45.0],
        );
        let points = primitive.outline_points(&[]);

        assert_eq!(points.len(), 2);
        assert_eq!(points[1], Vec2I::new(100_000, 0));
    }

    #[test]
    fn thermal_quadrant_is_closed_inside_convert_shape_like_kicad() {
        let primitive = primitive(AmPrimitiveId::Thermal, &[0.0, 0.0, 1.0, 0.4, 0.1, 0.0]);
        let points = primitive.convert_shape_to_polygon(&[]);

        assert!(points.len() > 2);
        assert_eq!(points.last().copied(), Some(points[0]));
    }
}
