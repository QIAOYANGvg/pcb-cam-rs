//! Backend-neutral rendering instructions derived from parsed Gerber items.
//!
//! The render plan keeps round strokes analytic and resolves filled geometry
//! to owned polygon contours. Parser geometry is used only while constructing
//! the plan and is not exposed by the public IR.

use std::error::Error;
use std::fmt;

use gerber_parse::dcode::DCode;
use gerber_parse::geometry::{Box2I, PolySet, Vec2I};
use gerber_parse::gerber_draw_item::DrawItem;
use gerber_parse::gerber_file_image::GerberFileImage;
use gerber_parse::types::{ApertureType, ShapeType};

/// A point in render-plan coordinates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderPoint {
    pub x: i32,
    pub y: i32,
}

impl RenderPoint {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned bounding box in render-plan coordinates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderBounds {
    pub origin: RenderPoint,
    pub size: RenderPoint,
}

impl RenderBounds {
    pub const fn new(origin: RenderPoint, size: RenderPoint) -> Self {
        Self { origin, size }
    }

    pub const fn end(self) -> RenderPoint {
        RenderPoint::new(
            self.origin.x.saturating_add(self.size.x),
            self.origin.y.saturating_add(self.size.y),
        )
    }

    pub const fn width(self) -> i32 {
        self.size.x
    }

    pub const fn height(self) -> i32 {
        self.size.y
    }

    pub const fn is_empty(self) -> bool {
        self.size.x == 0 && self.size.y == 0
    }

    fn merge(&mut self, other: Self) {
        let self_end = self.end();
        let other_end = other.end();
        let min_x = self.origin.x.min(other.origin.x);
        let min_y = self.origin.y.min(other.origin.y);
        let max_x = self_end.x.max(other_end.x);
        let max_y = self_end.y.max(other_end.y);

        *self = bounds_from_extents(
            i64::from(min_x),
            i64::from(min_y),
            i64::from(max_x),
            i64::from(max_y),
        );
    }
}

/// One filled polygon with an outer contour and zero or more holes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RenderPolygon {
    pub outline: Vec<RenderPoint>,
    pub holes: Vec<Vec<RenderPoint>>,
}

/// Positive/negative polarity as declared by a Gerber image or layer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Polarity {
    #[default]
    Positive,
    Negative,
}

impl Polarity {
    pub const fn from_negative(negative: bool) -> Self {
        if negative {
            Self::Negative
        } else {
            Self::Positive
        }
    }

    pub const fn is_negative(self) -> bool {
        matches!(self, Self::Negative)
    }

    /// Compose image and layer polarity using Gerber's inversion semantics.
    pub const fn compose(self, other: Self) -> Self {
        Self::from_negative(self.is_negative() ^ other.is_negative())
    }
}

/// Direction of an analytic arc in render-plan coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArcDirection {
    Clockwise,
    CounterClockwise,
}

/// An analytic round-capped line stroke.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrokeLine {
    pub start: RenderPoint,
    pub end: RenderPoint,
    pub width: i32,
}

/// An analytic round-capped circular arc stroke.
///
/// Parsed `DrawItem` arcs are canonicalized to a positive angular sweep. The
/// direction below accounts for whether the item's coordinate transform
/// reverses orientation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrokeArc {
    pub start: RenderPoint,
    pub end: RenderPoint,
    pub center: RenderPoint,
    pub width: i32,
    pub direction: ArcDirection,
    pub full_circle: bool,
}

/// Why a filled path exists in the plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillSource {
    Region,
    Flash,
    ExpandedStroke,
    Circle,
}

/// Filled geometry with explicit outer contours and holes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FillPath {
    pub polygons: Vec<RenderPolygon>,
    pub source: FillSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderGeometry {
    StrokeLine(StrokeLine),
    StrokeArc(StrokeArc),
    FillPath(FillPath),
}

/// One draw operation. Operations are stored and consumed in ascending
/// `draw_order`; conversion never sorts or combines source items.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderOperation {
    pub draw_order: u64,
    pub dcode: Option<i32>,
    pub layer_polarity: Polarity,
    pub effective_polarity: Polarity,
    pub bbox: RenderBounds,
    pub geometry: RenderGeometry,
}

/// Fully resolved, backend-neutral instructions for one Gerber image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderPlan {
    pub image_polarity: Polarity,
    pub bbox: Option<RenderBounds>,
    pub operations: Vec<RenderOperation>,
}

impl RenderPlan {
    pub fn from_image(image: &GerberFileImage) -> Result<Self, RenderPlanError> {
        let image_polarity = Polarity::from_negative(image.image_negative);
        let mut operations = Vec::with_capacity(image.drawings.len());
        let mut bbox = None;

        for (index, item) in image.drawings.iter().enumerate() {
            let draw_order =
                u64::try_from(index).map_err(|_| RenderPlanError::DrawOrderOverflow { index })?;
            let dcode = image.aperture_list.get(&item.dcode);
            let operation = operation_from_draw_item(image_polarity, draw_order, item, dcode)?;

            merge_optional_bounds(&mut bbox, operation.bbox);
            operations.push(operation);
        }

        Ok(Self {
            image_polarity,
            bbox,
            operations,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

impl TryFrom<&GerberFileImage> for RenderPlan {
    type Error = RenderPlanError;

    fn try_from(image: &GerberFileImage) -> Result<Self, Self::Error> {
        Self::from_image(image)
    }
}

/// Build one render operation while preserving its source draw order.
pub fn operation_from_draw_item(
    image_polarity: Polarity,
    draw_order: u64,
    item: &DrawItem,
    dcode: Option<&DCode>,
) -> Result<RenderOperation, RenderPlanError> {
    let layer_polarity = Polarity::from_negative(item.get_layer_polarity());
    let effective_polarity = image_polarity.compose(layer_polarity);
    let dcode_number = (item.dcode >= 10).then_some(item.dcode);

    let (geometry, bbox) = match item.shape_type {
        ShapeType::Segment => segment_geometry(item, dcode, draw_order)?,
        ShapeType::Arc => {
            let width = stroke_width(item);
            let start = render_point(item.get_ab_position(item.start));
            let end = render_point(item.get_ab_position(item.end));
            let center = render_point(item.get_ab_position(item.arc_centre));
            let bbox = render_bounds(item.get_bounding_box(dcode));

            (
                RenderGeometry::StrokeArc(StrokeArc {
                    start,
                    end,
                    center,
                    width,
                    direction: transformed_arc_direction(item),
                    full_circle: item.start == item.end,
                }),
                bbox,
            )
        }
        ShapeType::Polygon => {
            let polyset = transformed_outlines(item, &item.shape_as_polygon);
            fill_geometry(polyset, FillSource::Region, draw_order, item)?
        }
        ShapeType::SpotCircle
        | ShapeType::SpotRect
        | ShapeType::SpotOval
        | ShapeType::SpotPoly
        | ShapeType::SpotMacro => {
            let dcode = dcode.ok_or(RenderPlanError::MissingAperture {
                draw_order,
                dcode: item.dcode,
            })?;
            let polyset = resolved_flash_geometry(item, dcode);
            fill_geometry(polyset, FillSource::Flash, draw_order, item)?
        }
        ShapeType::Circle => {
            let center = render_point(item.get_ab_position(item.start));
            let edge = render_point(item.get_ab_position(item.end));
            let bbox = render_bounds(item.get_bounding_box(dcode));

            (
                RenderGeometry::StrokeArc(StrokeArc {
                    start: edge,
                    end: edge,
                    center,
                    width: stroke_width(item),
                    direction: transformed_arc_direction(item),
                    full_circle: true,
                }),
                bbox,
            )
        }
    };

    Ok(RenderOperation {
        draw_order,
        dcode: dcode_number,
        layer_polarity,
        effective_polarity,
        bbox,
        geometry,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderPlanError {
    DrawOrderOverflow {
        index: usize,
    },
    MissingAperture {
        draw_order: u64,
        dcode: i32,
    },
    EmptyGeometry {
        draw_order: u64,
        shape_type: ShapeType,
    },
}

impl fmt::Display for RenderPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DrawOrderOverflow { index } => {
                write!(formatter, "draw item index {index} does not fit in u64")
            }
            Self::MissingAperture { draw_order, dcode } => write!(
                formatter,
                "draw operation {draw_order} references missing aperture D{dcode}"
            ),
            Self::EmptyGeometry {
                draw_order,
                shape_type,
            } => write!(
                formatter,
                "draw operation {draw_order} ({shape_type:?}) resolved to empty geometry"
            ),
        }
    }
}

impl Error for RenderPlanError {}

fn segment_geometry(
    item: &DrawItem,
    dcode: Option<&DCode>,
    draw_order: u64,
) -> Result<(RenderGeometry, RenderBounds), RenderPlanError> {
    let needs_fill = !item.shape_as_polygon.is_empty()
        || dcode.is_some_and(|aperture| aperture.apert_type == ApertureType::Rect);

    if needs_fill {
        let source = if item.shape_as_polygon.is_empty() {
            item.convert_segment_to_polygon()
        } else {
            item.shape_as_polyset()
        };
        let polyset = transformed_polyset(item, &source);
        return fill_geometry(polyset, FillSource::ExpandedStroke, draw_order, item);
    }

    let start = render_point(item.get_ab_position(item.start));
    let end = render_point(item.get_ab_position(item.end));
    let width = stroke_width(item);
    let bbox = stroke_line_bounds(start, end, width);

    Ok((
        RenderGeometry::StrokeLine(StrokeLine { start, end, width }),
        bbox,
    ))
}

fn fill_geometry(
    geometry: PolySet,
    source: FillSource,
    draw_order: u64,
    item: &DrawItem,
) -> Result<(RenderGeometry, RenderBounds), RenderPlanError> {
    let bbox = geometry
        .bbox()
        .map(render_bounds)
        .ok_or(RenderPlanError::EmptyGeometry {
            draw_order,
            shape_type: item.shape_type,
        })?;
    let polygons = geometry
        .polygons
        .into_iter()
        .map(|polygon| RenderPolygon {
            outline: polygon.outline.into_iter().map(render_point).collect(),
            holes: polygon
                .holes
                .into_iter()
                .map(|hole| hole.into_iter().map(render_point).collect())
                .collect(),
        })
        .collect();

    Ok((
        RenderGeometry::FillPath(FillPath { polygons, source }),
        bbox,
    ))
}

fn resolved_flash_geometry(item: &DrawItem, dcode: &DCode) -> PolySet {
    if item.shape_type == ShapeType::SpotMacro && item.macro_shape_polygon.outline_count() > 0 {
        return item.macro_shape_polygon.clone();
    }

    let mut aperture = dcode.clone();

    if aperture.polyset.outline_count() == 0 {
        aperture.convert_shape_to_polygon();
    }

    transform_polyset_points(&mut aperture.polyset, |point| {
        item.get_ab_position(saturating_add(item.start, point))
    });
    aperture.polyset
}

fn transformed_outlines(item: &DrawItem, outlines: &[Vec<Vec2I>]) -> PolySet {
    let source = PolySet::from_outlines(outlines);
    transformed_polyset(item, &source)
}

fn transformed_polyset(item: &DrawItem, source: &PolySet) -> PolySet {
    let mut result = source.clone();
    transform_polyset_points(&mut result, |point| item.get_ab_position(point));
    result
}

fn transform_polyset_points<F>(polyset: &mut PolySet, mut transform: F)
where
    F: FnMut(Vec2I) -> Vec2I,
{
    for polygon in &mut polyset.polygons {
        for point in &mut polygon.outline {
            *point = transform(*point);
        }

        for hole in &mut polygon.holes {
            for point in hole {
                *point = transform(*point);
            }
        }
    }
}

fn stroke_width(item: &DrawItem) -> i32 {
    item.size.x.max(item.size.y).max(0)
}

fn transformed_arc_direction(item: &DrawItem) -> ArcDirection {
    let reverses_orientation = item.swap_axis
        ^ item.mirror_a
        ^ !item.mirror_b
        ^ (item.draw_scale.0.is_sign_negative() != item.draw_scale.1.is_sign_negative());

    if reverses_orientation {
        ArcDirection::Clockwise
    } else {
        ArcDirection::CounterClockwise
    }
}

fn stroke_line_bounds(start: RenderPoint, end: RenderPoint, width: i32) -> RenderBounds {
    let radius = (i64::from(width.max(0)) + 1) / 2;
    bounds_from_extents(
        i64::from(start.x.min(end.x)) - radius,
        i64::from(start.y.min(end.y)) - radius,
        i64::from(start.x.max(end.x)) + radius,
        i64::from(start.y.max(end.y)) + radius,
    )
}

fn bounds_from_extents(min_x: i64, min_y: i64, max_x: i64, max_y: i64) -> RenderBounds {
    let min_x = clamp_i64_to_i32(min_x);
    let min_y = clamp_i64_to_i32(min_y);
    let max_x = clamp_i64_to_i32(max_x.max(i64::from(min_x)));
    let max_y = clamp_i64_to_i32(max_y.max(i64::from(min_y)));

    RenderBounds::new(
        RenderPoint::new(min_x, min_y),
        RenderPoint::new(max_x.saturating_sub(min_x), max_y.saturating_sub(min_y)),
    )
}

fn render_point(point: Vec2I) -> RenderPoint {
    RenderPoint::new(point.x, point.y)
}

fn render_bounds(mut bbox: Box2I) -> RenderBounds {
    bbox.normalize();
    RenderBounds::new(render_point(bbox.origin), render_point(bbox.size))
}

fn merge_optional_bounds(target: &mut Option<RenderBounds>, bounds: RenderBounds) {
    if let Some(current) = target {
        current.merge(bounds);
    } else {
        *target = Some(bounds);
    }
}

fn saturating_add(a: Vec2I, b: Vec2I) -> Vec2I {
    Vec2I::new(a.x.saturating_add(b.x), a.y.saturating_add(b.y))
}

fn clamp_i64_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use gerber_parse::geometry::Polygon;

    fn identity_item() -> DrawItem {
        let mut item = DrawItem::new();
        item.mirror_b = true;
        item
    }

    #[test]
    fn flash_resolves_owned_geometry_and_preserves_holes() {
        let mut image = GerberFileImage::default();
        let mut dcode = DCode::new(10);
        dcode.apert_type = ApertureType::Rect;
        dcode.polyset.polygons.push(Polygon {
            outline: vec![
                Vec2I::new(-10, -8),
                Vec2I::new(10, -8),
                Vec2I::new(10, 8),
                Vec2I::new(-10, 8),
            ],
            holes: vec![vec![
                Vec2I::new(-2, -2),
                Vec2I::new(2, -2),
                Vec2I::new(2, 2),
                Vec2I::new(-2, 2),
            ]],
        });
        image.aperture_list.insert(10, dcode);

        let mut item = identity_item();
        item.shape_type = ShapeType::SpotRect;
        item.flashed = true;
        item.dcode = 10;
        item.start = Vec2I::new(100, 200);
        image.drawings.push(item);

        let plan = RenderPlan::from_image(&image).unwrap();
        let RenderGeometry::FillPath(fill) = &plan.operations[0].geometry else {
            panic!("flash was not resolved to a fill path");
        };

        assert_eq!(fill.source, FillSource::Flash);
        assert_eq!(fill.polygons[0].outline[0], RenderPoint::new(90, 192));
        assert_eq!(fill.polygons[0].holes.len(), 1);
        assert_eq!(fill.polygons[0].holes[0][0], RenderPoint::new(98, 198));
        assert_eq!(
            plan.bbox,
            Some(RenderBounds::new(
                RenderPoint::new(90, 192),
                RenderPoint::new(20, 16),
            ))
        );
    }

    #[test]
    fn round_segment_stays_analytic_and_keeps_draw_order() {
        let mut image = GerberFileImage::default();
        let mut item = identity_item();
        item.shape_type = ShapeType::Segment;
        item.start = Vec2I::new(10, 20);
        item.end = Vec2I::new(110, 70);
        item.size = Vec2I::new(12, 12);
        image.drawings.push(item);

        let plan = RenderPlan::from_image(&image).unwrap();
        assert_eq!(plan.operations[0].draw_order, 0);
        assert_eq!(
            plan.operations[0].geometry,
            RenderGeometry::StrokeLine(StrokeLine {
                start: RenderPoint::new(10, 20),
                end: RenderPoint::new(110, 70),
                width: 12,
            })
        );
    }

    #[test]
    fn zero_length_round_segment_is_preserved() {
        let mut image = GerberFileImage::default();
        let mut item = identity_item();
        item.start = Vec2I::new(25, 50);
        item.end = item.start;
        item.size = Vec2I::new(7, 7);
        image.drawings.push(item);

        let plan = RenderPlan::from_image(&image).unwrap();
        let RenderGeometry::StrokeLine(stroke) = &plan.operations[0].geometry else {
            panic!("zero-length segment was not kept analytic");
        };

        assert_eq!(stroke.start, stroke.end);
        assert_eq!(stroke.width, 7);
        assert_eq!(plan.operations[0].bbox.width(), 8);
        assert_eq!(plan.operations[0].bbox.height(), 8);
    }

    #[test]
    fn arc_preserves_start_end_center_width_and_direction() {
        let mut image = GerberFileImage::default();
        let mut item = identity_item();
        item.shape_type = ShapeType::Arc;
        item.start = Vec2I::new(100, 0);
        item.end = Vec2I::new(0, 100);
        item.arc_centre = Vec2I::new(0, 0);
        item.size = Vec2I::new(14, 14);
        image.drawings.push(item);

        let plan = RenderPlan::from_image(&image).unwrap();
        assert_eq!(
            plan.operations[0].geometry,
            RenderGeometry::StrokeArc(StrokeArc {
                start: RenderPoint::new(100, 0),
                end: RenderPoint::new(0, 100),
                center: RenderPoint::new(0, 0),
                width: 14,
                direction: ArcDirection::CounterClockwise,
                full_circle: false,
            })
        );
    }

    #[test]
    fn image_and_layer_negative_polarity_are_explicitly_composed() {
        let mut image = GerberFileImage {
            image_negative: true,
            ..GerberFileImage::default()
        };

        let mut positive_layer_item = identity_item();
        positive_layer_item.shape_type = ShapeType::Segment;
        positive_layer_item.end = Vec2I::new(10, 0);
        positive_layer_item.size = Vec2I::new(2, 2);
        image.drawings.push(positive_layer_item.clone());

        let mut negative_layer_item = positive_layer_item;
        negative_layer_item.start = Vec2I::new(0, 10);
        negative_layer_item.end = Vec2I::new(10, 10);
        negative_layer_item.layer_negative = true;
        image.drawings.push(negative_layer_item);

        let plan = RenderPlan::from_image(&image).unwrap();
        assert_eq!(plan.image_polarity, Polarity::Negative);
        assert_eq!(plan.operations[0].layer_polarity, Polarity::Positive);
        assert_eq!(plan.operations[0].effective_polarity, Polarity::Negative);
        assert_eq!(plan.operations[1].layer_polarity, Polarity::Negative);
        assert_eq!(plan.operations[1].effective_polarity, Polarity::Positive);
    }
}
