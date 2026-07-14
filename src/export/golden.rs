use serde_json::{Value, json};

use crate::dcode::DCode;
use crate::gerber_draw_item::DrawItem;
use crate::gerber_file_image::GerberFileImage;
use crate::geometry::{Box2I, PolySet, Vec2I};
use crate::netlist_metadata::NetlistMetadata;
use crate::types::{ApertureHoleType, ApertureType, ShapeType};

pub fn export_golden_json(image: &GerberFileImage) -> Value {
    json!({
        "metadata": metadata_json(image),
        "dcodes": dcodes_json(image),
        "items": items_json(image),
    })
}

fn metadata_json(image: &GerberFileImage) -> Value {
    json!({
        "fileName": "",
        "isMetric": image.gerb_metric,
        "isX2": image.is_x2_file,
        "imageNegative": image.image_negative,
        "fileFunction": image.file_function.as_ref().map(|ff| ff.get_file_type().to_string()),
        "itemCount": image.drawings.len(),
        "dcodeCount": image.aperture_list.values().filter(|dc| dc.in_use || dc.defined).count(),
        "imageOffset": vec_json(image.image_offset),
        "imageRotation": image.image_rotation,
        "localRotation": image.local_rotation,
        "offset": vec_json(image.offset),
        "scale": json!({"x": image.scale.0, "y": image.scale.1}),
        "swapAxis": image.swap_axis,
        "mirrorA": image.mirror_a,
        "mirrorB": image.mirror_b,
        "imageJustifyOffset": vec_json(image.image_justify_offset),
        "imageJustifyXCenter": image.image_justify_x_center,
        "imageJustifyYCenter": image.image_justify_y_center,
        "fmtScale": vec_json(image.fmt_scale),
        "fmtLen": vec_json(image.fmt_len),
        "noTrailingZeros": image.no_trailing_zeros,
        "relative": image.relative,
    })
}

fn dcodes_json(image: &GerberFileImage) -> Vec<Value> {
    image
        .aperture_list
        .values()
        .filter(|dc| dc.in_use || dc.defined)
        .map(dcode_json)
        .collect()
}

fn dcode_json(dcode: &DCode) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("num".to_string(), json!(dcode.num));
    object.insert(
        "type".to_string(),
        json!(aperture_type_str(dcode.apert_type)),
    );
    object.insert("size".to_string(), vec_json(dcode.size));
    object.insert("drill".to_string(), vec_json(dcode.drill));
    object.insert(
        "drillShape".to_string(),
        json!(hole_type_str(dcode.drill_shape)),
    );
    object.insert("rotation".to_string(), json!(dcode.rotation));
    object.insert("edgesCount".to_string(), json!(dcode.edges_count));
    object.insert("inUse".to_string(), json!(dcode.in_use));
    object.insert("defined".to_string(), json!(dcode.defined));
    object.insert("aperFunction".to_string(), json!(dcode.aper_function));

    if dcode.apert_type == ApertureType::Macro {
        object.insert("macroName".to_string(), json!(dcode.macro_name));
        object.insert("macroParams".to_string(), json!(dcode.am_params));
    }

    Value::Object(object)
}

fn items_json(image: &GerberFileImage) -> Vec<Value> {
    image
        .drawings
        .iter()
        .map(|item| item_json(item, image.aperture_list.get(&item.dcode)))
        .collect()
}

fn item_json(item: &DrawItem, dcode: Option<&DCode>) -> Value {
    let mut object = serde_json::Map::new();
    object.insert(
        "shapeType".to_string(),
        json!(shape_type_str(item.shape_type)),
    );
    object.insert("start".to_string(), vec_json(item.start));
    object.insert("end".to_string(), vec_json(item.end));
    object.insert("size".to_string(), vec_json(item.size));
    object.insert("dcode".to_string(), json!(item.dcode));
    object.insert("flashed".to_string(), json!(item.flashed));
    object.insert("unitsMetric".to_string(), json!(item.units_metric));

    if item.shape_type == ShapeType::Arc {
        object.insert("arcCentre".to_string(), vec_json(item.arc_centre));
    }

    if let Some(dcode) = dcode {
        object.insert(
            "aperture".to_string(),
            json!({
                "type": aperture_type_str(dcode.apert_type),
                "size": vec_json(dcode.size),
            }),
        );
    }

    object.insert("layerNegative".to_string(), json!(item.layer_negative));
    object.insert("aperFunction".to_string(), json!(item.aper_function));
    object.insert(
        "netAttributes".to_string(),
        net_attributes_json(&item.net_attributes),
    );

    if !item.shape_as_polygon.is_empty() {
        object.insert(
            "shapeAsPolygon".to_string(),
            outlines_json(&item.shape_as_polygon),
        );
    }

    if item.macro_shape_polygon.outline_count() > 0 {
        object.insert(
            "macroShapePolygon".to_string(),
            polyset_json(&item.macro_shape_polygon),
        );
    }

    object.insert(
        "boundingBox".to_string(),
        bbox_json(item.get_bounding_box(dcode)),
    );

    Value::Object(object)
}

fn vec_json(vec: Vec2I) -> Value {
    json!({"x": vec.x, "y": vec.y})
}

fn bbox_json(bbox: Box2I) -> Value {
    json!({
        "origin": vec_json(bbox.origin),
        "size": vec_json(bbox.size),
    })
}

fn net_attributes_json(net: &NetlistMetadata) -> Value {
    json!({
        "netAttribType": net.net_attrib_type,
        "netname": net.netname,
        "cmpref": net.cmpref,
        "padname": net.padname,
        "pinFunction": net.pad_pin_function,
    })
}

fn outlines_json(outlines: &[Vec<Vec2I>]) -> Value {
    Value::Array(
        outlines
            .iter()
            .map(|outline| json!({"outline": points_json(outline), "holes": []}))
            .collect(),
    )
}

fn polyset_json(polyset: &PolySet) -> Value {
    Value::Array(
        polyset
            .polygons
            .iter()
            .map(|poly| {
                json!({
                    "outline": points_json(&poly.outline),
                    "holes": poly.holes.iter().map(|hole| points_json(hole)).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

fn points_json(points: &[Vec2I]) -> Value {
    Value::Array(points.iter().map(|point| vec_json(*point)).collect())
}

fn aperture_type_str(aperture: ApertureType) -> &'static str {
    match aperture {
        ApertureType::Circle => "C",
        ApertureType::Rect => "R",
        ApertureType::Oval => "O",
        ApertureType::Polygon => "P",
        ApertureType::Macro => "M",
    }
}

fn hole_type_str(hole: ApertureHoleType) -> &'static str {
    match hole {
        ApertureHoleType::NoHole => "NO_HOLE",
        ApertureHoleType::RoundHole => "ROUND_HOLE",
        ApertureHoleType::RectHole => "RECT_HOLE",
    }
}

fn shape_type_str(shape: ShapeType) -> &'static str {
    match shape {
        ShapeType::Segment => "SEGMENT",
        ShapeType::Arc => "ARC",
        ShapeType::Circle => "CIRCLE",
        ShapeType::Polygon => "POLYGON",
        ShapeType::SpotCircle => "SPOT_CIRCLE",
        ShapeType::SpotRect => "SPOT_RECT",
        ShapeType::SpotOval => "SPOT_OVAL",
        ShapeType::SpotPoly => "SPOT_POLY",
        ShapeType::SpotMacro => "SPOT_MACRO",
    }
}
