/// D-code (Aperture) definition.
/// Ported from KiCad dcode.h / dcode.cpp
///
/// A gerber D-code (also called Aperture) defines a shape that can be flashed.
use crate::coord::Vec2I;
use crate::geometry::{
    PolySet, circle_to_polygon_by_error, rectangle_to_polygon, regular_polygon_to_polygon,
    rotate_point,
};
use crate::types::{ApertureHoleType, ApertureType, FIRST_DCODE};

/// Number of segments to approximate a circle
pub const SEGS_CNT: usize = 64;

/// Default size in IU (0.1mm = 10,000 IU)
const DCODE_DEFAULT_SIZE: i32 = 10_000;

/// A gerber D-code (aperture) definition.
#[derive(Clone, Debug)]
pub struct DCode {
    /// D code value (>= 10)
    pub num: i32,

    /// Horizontal and vertical dimensions
    pub size: Vec2I,

    /// Aperture type (circle, rectangle, oval, polygon, macro)
    pub apert_type: ApertureType,

    /// Dimension of the hole (if any)
    pub drill: Vec2I,

    /// Shape of the hole (no hole, round, rect)
    pub drill_shape: ApertureHoleType,

    /// Shape rotation in degrees * 10 (0.1 degree units)
    pub rotation: f64,

    /// Number of edges for polygon aperture type
    pub edges_count: i32,

    /// false if the aperture (previously defined) is not used to draw something
    pub in_use: bool,

    /// false if the aperture is not defined in the header
    pub defined: bool,

    /// Aperture function attribute from %TA.AperFunction
    pub aper_function: String,

    /// Polygon used to draw APT_POLYGON shape and complex shapes converted to polygon.
    /// Kept for the existing exporter; mirrors the first outline of `polyset`.
    pub polygon: Vec<Vec2I>,

    /// KiCad-like polygon set for expanded aperture geometry.
    pub polyset: PolySet,

    /// Aperture macro name, when `apert_type` is Macro.
    pub macro_name: String,

    /// Parameters for macro customization (1-indexed in C++, 0-indexed here internally)
    pub am_params: Vec<f64>,
}

impl DCode {
    pub fn new(num: i32) -> Self {
        Self {
            num,
            size: Vec2I::new(DCODE_DEFAULT_SIZE, DCODE_DEFAULT_SIZE),
            apert_type: ApertureType::Circle,
            drill: Vec2I::new(0, 0),
            drill_shape: ApertureHoleType::NoHole,
            rotation: 0.0,
            edges_count: 0,
            in_use: false,
            defined: false,
            aper_function: String::new(),
            polygon: Vec::new(),
            polyset: PolySet::new(),
            macro_name: String::new(),
            am_params: Vec::new(),
        }
    }

    /// Test if a D-code value is valid (>= FIRST_DCODE)
    pub fn is_valid_dcode_value(value: i32) -> bool {
        value >= FIRST_DCODE
    }

    /// Clear all D-code data
    pub fn clear(&mut self) {
        self.size = Vec2I::new(DCODE_DEFAULT_SIZE, DCODE_DEFAULT_SIZE);
        self.apert_type = ApertureType::Circle;
        self.drill = Vec2I::new(0, 0);
        self.drill_shape = ApertureHoleType::NoHole;
        self.rotation = 0.0;
        self.edges_count = 0;
        self.in_use = false;
        self.defined = false;
        self.aper_function.clear();
        self.polygon.clear();
        self.polyset.remove_all_contours();
        self.macro_name.clear();
        self.am_params.clear();
    }

    /// Add a parameter for macro customization
    pub fn append_param(&mut self, value: f64) {
        self.am_params.push(value);
    }

    /// Convert this aperture to a polygon set.
    ///
    /// Ported from KiCad `D_CODE::ConvertShapeToPolygon` for standard apertures.
    /// Macro apertures are filled by the aperture macro conversion path.
    pub fn convert_shape_to_polygon(&mut self) {
        self.polyset.remove_all_contours();
        self.polygon.clear();

        match self.apert_type {
            ApertureType::Circle => {
                self.polyset
                    .add_outline(circle_to_polygon_by_error(self.size.x >> 1, 500));
                self.add_hole_to_polygon();
            }
            ApertureType::Rect => {
                self.polyset.add_outline(rectangle_to_polygon(self.size));
                self.add_hole_to_polygon();
            }
            ApertureType::Oval => {
                self.polyset.add_outline(self.oval_to_polygon());
                self.add_hole_to_polygon();
            }
            ApertureType::Polygon => {
                self.edges_count = self.edges_count.clamp(3, 12);
                self.polyset.add_outline(regular_polygon_to_polygon(
                    self.size.x >> 1,
                    self.edges_count,
                    0.0,
                ));
                self.add_hole_to_polygon();

                if self.rotation != 0.0 {
                    self.polyset.rotate(self.rotation);
                }
            }
            ApertureType::Macro => {}
        }

        self.refresh_flat_polygon();
    }

    fn add_hole_to_polygon(&mut self) {
        let mut hole_buffer = PolySet::new();

        match self.drill_shape {
            ApertureHoleType::RoundHole => {
                if self.drill.x > 0 {
                    hole_buffer.add_outline(circle_to_polygon_by_error(self.drill.x / 2, 500));
                }
            }
            ApertureHoleType::RectHole => {
                if self.drill.x > 0 && self.drill.y > 0 {
                    hole_buffer.add_outline(rectangle_to_polygon(self.drill));
                }
            }
            ApertureHoleType::NoHole => {}
        }

        self.polyset.boolean_subtract(&hole_buffer);
        self.polyset.fracture();
    }

    fn oval_to_polygon(&self) -> Vec<Vec2I> {
        let (delta, radius, vertical) = if self.size.x > self.size.y {
            ((self.size.x - self.size.y) / 2, self.size.y / 2, false)
        } else {
            ((self.size.y - self.size.x) / 2, self.size.x / 2, true)
        };

        let initial = Vec2I::new(0, radius);
        let mut outline = Vec::with_capacity(SEGS_CNT + 3);
        outline.push(initial);

        for ii in 0..=SEGS_CNT / 2 {
            let mut curr = rotate_point(initial, 360.0 * ii as f64 / SEGS_CNT as f64);
            curr.x += delta;
            outline.push(curr);
        }

        for ii in SEGS_CNT / 2..=SEGS_CNT {
            let mut curr = rotate_point(initial, 360.0 * ii as f64 / SEGS_CNT as f64);
            curr.x -= delta;
            outline.push(curr);
        }

        outline.push(initial);

        if vertical {
            outline
                .into_iter()
                .map(|point| rotate_point(point, 90.0))
                .collect()
        } else {
            outline
        }
    }

    pub fn refresh_flat_polygon(&mut self) {
        self.polygon = self
            .polyset
            .polygons
            .first()
            .map(|poly| poly.outline.clone())
            .unwrap_or_default();
    }

    /// Return the aperture dimension KiCad uses for flashed text sizing.
    ///
    /// Mirrors `D_CODE::GetShapeDim`: circles use X size, standard non-circles
    /// use the smaller size axis, and macro apertures use the cached polygon bbox.
    pub fn get_shape_dim(&mut self) -> i32 {
        match self.apert_type {
            ApertureType::Circle => self.size.x,
            ApertureType::Rect | ApertureType::Oval | ApertureType::Polygon => {
                self.size.x.min(self.size.y)
            }
            ApertureType::Macro => self
                .polyset
                .bbox()
                .map(|bbox| bbox.width().min(bbox.height()))
                .unwrap_or(0),
        }
    }

    /// Get the number of stored parameters
    pub fn get_param_count(&self) -> usize {
        self.am_params.len()
    }

    /// Get a parameter by index (1-based, matching KiCad convention)
    pub fn get_param(&self, idx: usize) -> f64 {
        if idx >= 1 && idx <= self.am_params.len() {
            self.am_params[idx - 1]
        } else {
            0.0
        }
    }

    /// Get the aperture type name as a string
    pub fn show_aperture_type(apert_type: ApertureType) -> &'static str {
        match apert_type {
            ApertureType::Circle => "CIRCLE",
            ApertureType::Rect => "RECT",
            ApertureType::Oval => "OVAL",
            ApertureType::Polygon => "POLYGON",
            ApertureType::Macro => "MACRO",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_restores_kicad_dcode_defaults() {
        let mut dcode = DCode::new(10);
        dcode.size = Vec2I::new(1, 2);
        dcode.apert_type = ApertureType::Rect;
        dcode.drill = Vec2I::new(3, 4);
        dcode.drill_shape = ApertureHoleType::RectHole;
        dcode.rotation = 45.0;
        dcode.edges_count = 6;
        dcode.in_use = true;
        dcode.defined = true;
        dcode
            .polyset
            .add_outline(vec![Vec2I::new(0, 0), Vec2I::new(1, 0)]);

        dcode.clear();

        assert_eq!(
            dcode.size,
            Vec2I::new(DCODE_DEFAULT_SIZE, DCODE_DEFAULT_SIZE)
        );
        assert_eq!(dcode.apert_type, ApertureType::Circle);
        assert_eq!(dcode.drill, Vec2I::new(0, 0));
        assert_eq!(dcode.drill_shape, ApertureHoleType::NoHole);
        assert_eq!(dcode.rotation, 0.0);
        assert_eq!(dcode.edges_count, 0);
        assert!(!dcode.in_use);
        assert!(!dcode.defined);
        assert_eq!(dcode.polyset.outline_count(), 0);
    }

    #[test]
    fn get_shape_dim_matches_standard_aperture_rules() {
        let mut dcode = DCode::new(10);
        dcode.size = Vec2I::new(200, 100);

        dcode.apert_type = ApertureType::Circle;
        assert_eq!(dcode.get_shape_dim(), 200);

        dcode.apert_type = ApertureType::Rect;
        assert_eq!(dcode.get_shape_dim(), 100);

        dcode.apert_type = ApertureType::Oval;
        assert_eq!(dcode.get_shape_dim(), 100);

        dcode.apert_type = ApertureType::Polygon;
        assert_eq!(dcode.get_shape_dim(), 100);
    }

    #[test]
    fn get_shape_dim_for_macro_uses_cached_polygon_bbox() {
        let mut dcode = DCode::new(10);
        dcode.apert_type = ApertureType::Macro;
        dcode.polyset.add_outline(vec![
            Vec2I::new(-10, -20),
            Vec2I::new(30, -20),
            Vec2I::new(30, 50),
            Vec2I::new(-10, 50),
        ]);

        assert_eq!(dcode.get_shape_dim(), 40);
    }
}
