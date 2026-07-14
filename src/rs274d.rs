//! RS-274D command execution.
//! Ported from KiCad gerbview/rs274d.cpp.

use crate::coord::{GerberFileImage, Vec2I, scale_to_iu};
use crate::draw_item::DrawItem;
use crate::rs274x::{CommandResult, execute_rs274x_command, read_x_command_id};
use crate::types::{ApertureType, FIRST_DCODE, Interpolation, ShapeType};

pub const GC_MOVE: i32 = 0;
pub const GC_LINEAR_INTERPOL_1X: i32 = 1;
pub const GC_CIRCLE_NEG_INTERPOL: i32 = 2;
pub const GC_CIRCLE_POS_INTERPOL: i32 = 3;
pub const GC_COMMENT: i32 = 4;
pub const GC_TURN_ON_POLY_FILL: i32 = 36;
pub const GC_TURN_OFF_POLY_FILL: i32 = 37;
pub const GC_SELECT_TOOL: i32 = 54;
pub const GC_PHOTO_MODE: i32 = 55;
pub const GC_SPECIFY_INCHES: i32 = 70;
pub const GC_SPECIFY_MILLIMETERS: i32 = 71;
pub const GC_TURN_OFF_360_INTERPOL: i32 = 74;
pub const GC_TURN_ON_360_INTERPOL: i32 = 75;
pub const GC_SPECIFY_ABSOLUTE_COORD: i32 = 90;
pub const GC_SPECIFY_RELATIVE_COORD: i32 = 91;

const ARC_APPROX_ERROR_MAX: i32 = 500;
const MIN_SEGCOUNT_FOR_CIRCLE: f64 = 8.0;

pub fn code_number(text: &str, offset: usize) -> (i32, usize) {
    let bytes = text.as_bytes();
    let start = offset.saturating_add(1);
    let mut pos = start;

    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }

    if pos < bytes.len() && (bytes[pos] == b'+' || bytes[pos] == b'-') {
        pos += 1;
    }

    let digit_start = pos;

    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        pos += 1;
    }

    if digit_start == pos {
        return (0, start.min(text.len()));
    }

    let Ok(value) = text[start..pos].trim_start().parse::<i64>() else {
        return (0, start.min(text.len()));
    };

    if value >= i32::MAX as i64 || value < i32::MIN as i64 {
        (0, start.min(text.len()))
    } else {
        (value as i32, pos)
    }
}

pub fn execute_g_command(
    image: &mut GerberFileImage,
    text: &str,
    offset: usize,
    g_command: i32,
) -> CommandResult {
    let mut pos = offset;
    let mut ok = true;

    match g_command {
        GC_PHOTO_MODE => {}
        GC_LINEAR_INTERPOL_1X => image.interpolation = Interpolation::Linear1x,
        GC_CIRCLE_NEG_INTERPOL => image.interpolation = Interpolation::ArcNeg,
        GC_CIRCLE_POS_INTERPOL => image.interpolation = Interpolation::ArcPos,

        GC_COMMENT => {
            if text[pos..].starts_with(" #@! ") {
                pos += 5;
                let start = pos;

                while pos < text.len() && byte_at(text, pos) != Some(b'*') {
                    pos += 1;
                }

                let mut x2buf = text[start..pos].to_string();
                x2buf.push_str("*%");

                if let Some((code_command, after_id)) = read_x_command_id(&x2buf, 0) {
                    execute_rs274x_command(image, code_command, &x2buf, after_id);
                }
            }

            pos = skip_to_end_of_block(text, pos);
        }

        GC_SELECT_TOOL => {
            let (d_command, new_pos) = code_number(text, pos);
            pos = new_pos;

            if d_command < FIRST_DCODE {
                ok = false;
            } else {
                image.current_tool = d_command;

                if let Some(dcode) = image.aperture_list.get_mut(&d_command) {
                    dcode.in_use = true;
                }
            }
        }

        GC_SPECIFY_INCHES => image.gerb_metric = false,
        GC_SPECIFY_MILLIMETERS => image.gerb_metric = true,

        GC_TURN_OFF_360_INTERPOL => {
            image.arc_360_enabled = false;
            image.interpolation = Interpolation::Linear1x;
            image.as_arc_g74g75_cmd = true;
        }

        GC_TURN_ON_360_INTERPOL => {
            image.arc_360_enabled = true;
            image.as_arc_g74g75_cmd = true;
        }

        GC_SPECIFY_ABSOLUTE_COORD => image.relative = false,
        GC_SPECIFY_RELATIVE_COORD => image.relative = true,

        GC_TURN_ON_POLY_FILL => {
            image.polygon_fill_mode = true;
            image.exposure = false;
        }

        GC_TURN_OFF_POLY_FILL => {
            if image.exposure && !image.drawings.is_empty() {
                close_last_polygon(image);
                let item = image.drawings.last().cloned();
                if let Some(item) = item {
                    step_and_repeat_item(image, &item);
                }
            }

            image.exposure = false;
            image.polygon_fill_mode = false;
            image.polygon_fill_mode_state = 0;
            image.interpolation = Interpolation::Linear1x;
        }

        GC_MOVE | _ => {
            image
                .messages
                .push(format!("G{:02} command not handled", g_command));
            ok = false;
        }
    }

    CommandResult {
        ok,
        new_offset: pos,
    }
}

pub fn execute_dcode_command(image: &mut GerberFileImage, d_command: i32) -> bool {
    let mut size = Vec2I::new(15, 15);
    let mut aperture = ApertureType::Circle;
    let mut dcode_num = 0;

    if d_command >= FIRST_DCODE {
        image.current_tool = d_command;

        if let Some(dcode) = image.aperture_list.get_mut(&d_command) {
            dcode.in_use = true;
        } else {
            image.has_missing_dcode = true;
        }

        return true;
    }

    image.last_pen_command = d_command;

    if image.polygon_fill_mode {
        execute_dcode_polygon_mode(image, d_command)
    } else {
        if let Some(tool) = image.aperture_list.get(&image.current_tool) {
            size = tool.size;
            dcode_num = tool.num;
            aperture = tool.apert_type;
        }

        execute_dcode_normal_mode(image, d_command, size, aperture, dcode_num)
    }
}

pub fn fill_flashed_gbr_item(
    item: &mut DrawItem,
    aperture: ApertureType,
    dcode_index: i32,
    pos: Vec2I,
    mut size: Vec2I,
    layer_negative: bool,
    net_attributes: crate::netlist_metadata::NetlistMetadata,
) {
    item.size = size;
    item.start = pos;
    item.end = item.start;
    item.dcode = dcode_index;
    item.set_layer_polarity(layer_negative);
    item.flashed = true;
    item.net_attributes = net_attributes;

    match aperture {
        ApertureType::Polygon => item.shape_type = ShapeType::SpotPoly,
        ApertureType::Circle => {
            item.shape_type = ShapeType::SpotCircle;
            size.y = size.x;
            item.size = size;
        }
        ApertureType::Oval => item.shape_type = ShapeType::SpotOval,
        ApertureType::Rect => item.shape_type = ShapeType::SpotRect,
        ApertureType::Macro => item.shape_type = ShapeType::SpotMacro,
    }
}

pub fn fill_line_gbr_item(
    item: &mut DrawItem,
    dcode_index: i32,
    start: Vec2I,
    end: Vec2I,
    pen_size: Vec2I,
    layer_negative: bool,
    net_attributes: crate::netlist_metadata::NetlistMetadata,
) {
    item.flashed = false;
    item.size = pen_size;
    item.start = start;
    item.end = end;
    item.dcode = dcode_index;
    item.set_layer_polarity(layer_negative);
    item.net_attributes = net_attributes;
}

pub fn fill_arc_gbr_item(
    item: &mut DrawItem,
    dcode_index: i32,
    start: Vec2I,
    end: Vec2I,
    rel_center: Vec2I,
    pen_size: Vec2I,
    clockwise: bool,
    multiquadrant: bool,
    layer_negative: bool,
    net_attributes: crate::netlist_metadata::NetlistMetadata,
) {
    let center;
    item.shape_type = ShapeType::Arc;
    item.size = pen_size;
    item.flashed = false;
    item.net_attributes = net_attributes;

    if multiquadrant {
        center = add(start, rel_center);
    } else {
        let mut signed_center = rel_center;
        let delta = sub(end, start);

        if delta.x >= 0 && delta.y >= 0 {
            signed_center.x = -signed_center.x;
        } else if delta.x >= 0 && delta.y < 0 {
            // Quadrant 4: no change.
        } else if delta.x < 0 && delta.y >= 0 {
            signed_center.x = -signed_center.x;
            signed_center.y = -signed_center.y;
        } else {
            signed_center.y = -signed_center.y;
        }

        if !clockwise {
            signed_center = neg(signed_center);
        }

        center = add(signed_center, start);
    }

    if clockwise {
        item.start = start;
        item.end = end;
    } else {
        item.start = end;
        item.end = start;
    }

    item.arc_centre = center;
    item.dcode = dcode_index;
    item.set_layer_polarity(layer_negative);
}

fn execute_dcode_polygon_mode(image: &mut GerberFileImage, d_command: i32) -> bool {
    match d_command {
        1 => {
            if !image.exposure {
                image.exposure = true;
                let mut item = new_draw_item(image);
                item.shape_type = ShapeType::Polygon;
                item.flashed = false;
                item.dcode = 0;
                item.net_attributes = image.net_attribute_dict.clone();
                item.aper_function = image.aper_function.clone();
                image.drawings.push(item);
            }

            match image.interpolation {
                Interpolation::ArcNeg | Interpolation::ArcPos => {
                    if !image.as_arc_g74g75_cmd {
                        image.messages.push(
                            "Invalid Gerber file: missing G74 or G75 arc command".to_string(),
                        );
                        image.as_arc_g74g75_cmd = true;
                    }

                    let previous_pos = image.previous_pos;
                    let current_pos = image.current_pos;
                    let ij_pos = image.ij_pos;
                    let clockwise = image.interpolation != Interpolation::ArcNeg;
                    let arc_360_enabled = image.arc_360_enabled;
                    let layer_negative = image.layer_params.layer_negative;
                    let net_attributes = image.net_attribute_dict.clone();

                    if let Some(item) = image.drawings.last_mut() {
                        fill_arc_poly(
                            item,
                            previous_pos,
                            current_pos,
                            ij_pos,
                            clockwise,
                            arc_360_enabled,
                            layer_negative,
                            net_attributes,
                        );
                    }
                }
                _ => {
                    if let Some(item) = image.drawings.last_mut() {
                        item.start = image.previous_pos;

                        if item.shape_as_polygon.is_empty() {
                            item.shape_as_polygon.push(vec![item.start]);
                        }

                        item.end = image.current_pos;
                        if let Some(outline) = item.shape_as_polygon.last_mut() {
                            append_polygon_point(outline, item.end);
                        }
                    }
                }
            }

            image.previous_pos = image.current_pos;
            image.polygon_fill_mode_state = 1;
        }

        2 => {
            if image.exposure && !image.drawings.is_empty() {
                close_last_polygon(image);
                let item = image.drawings.last().cloned();
                if let Some(item) = item {
                    step_and_repeat_item(image, &item);
                }
            }

            image.exposure = false;
            image.previous_pos = image.current_pos;
            image.polygon_fill_mode_state = 0;
        }

        _ => return false,
    }

    true
}

fn execute_dcode_normal_mode(
    image: &mut GerberFileImage,
    d_command: i32,
    size: Vec2I,
    aperture: ApertureType,
    dcode_num: i32,
) -> bool {
    match d_command {
        1 => {
            image.exposure = true;

            match image.interpolation {
                Interpolation::Linear1x => {
                    let mut item = new_draw_item(image);
                    fill_line_gbr_item(
                        &mut item,
                        dcode_num,
                        image.previous_pos,
                        image.current_pos,
                        size,
                        image.layer_params.layer_negative,
                        image.net_attribute_dict.clone(),
                    );
                    image.drawings.push(item);
                    step_and_repeat_last_item(image);
                }

                Interpolation::ArcNeg | Interpolation::ArcPos => {
                    let mut item = new_draw_item(image);

                    if image.last_coord_is_ij_pos {
                        fill_arc_gbr_item(
                            &mut item,
                            dcode_num,
                            image.previous_pos,
                            image.current_pos,
                            image.ij_pos,
                            size,
                            image.interpolation != Interpolation::ArcNeg,
                            image.arc_360_enabled,
                            image.layer_params.layer_negative,
                            image.net_attribute_dict.clone(),
                        );
                        image.last_coord_is_ij_pos = false;
                    } else {
                        fill_line_gbr_item(
                            &mut item,
                            dcode_num,
                            image.previous_pos,
                            image.current_pos,
                            size,
                            image.layer_params.layer_negative,
                            image.net_attribute_dict.clone(),
                        );
                    }

                    image.drawings.push(item);
                    step_and_repeat_last_item(image);
                }
            }

            image.previous_pos = image.current_pos;
        }

        2 => {
            image.exposure = false;
            image.previous_pos = image.current_pos;
        }

        3 => {
            let mut item = new_draw_item(image);
            fill_flashed_gbr_item(
                &mut item,
                aperture,
                dcode_num,
                image.current_pos,
                size,
                image.layer_params.layer_negative,
                image.net_attribute_dict.clone(),
            );

            if aperture == ApertureType::Macro {
                let macro_info = image
                    .aperture_list
                    .get(&dcode_num)
                    .map(|dcode| (dcode.macro_name.clone(), dcode.am_params.clone()));

                if let Some((macro_name, params)) = macro_info {
                    if let Some(aperture_macro) = image.aperture_macros.get_mut(&macro_name) {
                        let item_transform = item.clone();
                        item.macro_shape_polygon =
                            aperture_macro.get_aperture_macro_shape(&params, item.start, |point| {
                                item_transform.get_ab_position(point)
                            });
                    }
                }
            }

            image.drawings.push(item);
            step_and_repeat_last_item(image);
            image.previous_pos = image.current_pos;
        }

        _ => return false,
    }

    true
}

fn fill_arc_poly(
    item: &mut DrawItem,
    start: Vec2I,
    end: Vec2I,
    rel_center: Vec2I,
    clockwise: bool,
    multiquadrant: bool,
    layer_negative: bool,
    net_attributes: crate::netlist_metadata::NetlistMetadata,
) {
    let mut dummy = DrawItem::new();

    item.set_layer_polarity(layer_negative);
    fill_arc_gbr_item(
        &mut dummy,
        0,
        start,
        end,
        rel_center,
        Vec2I::new(0, 0),
        clockwise,
        multiquadrant,
        layer_negative,
        net_attributes.clone(),
    );
    item.net_attributes = net_attributes;

    let center = dummy.arc_centre;
    let arc_start = sub(dummy.start, center);
    let arc_end = sub(dummy.end, center);

    let start_angle = angle_degrees(arc_start);
    let mut end_angle = angle_degrees(arc_end);

    if start_angle >= end_angle {
        end_angle += 360.0;
    }

    let arc_angle = start_angle - end_angle;
    let radius = euclidean_norm(sub(start, rel_center));
    let count = get_arc_to_segment_count(radius, ARC_APPROX_ERROR_MAX, arc_angle);
    let increment_angle = arc_angle.abs() / count as f64;

    if item.shape_as_polygon.is_empty() {
        item.shape_as_polygon.push(Vec::new());
    }

    if let Some(outline) = item.shape_as_polygon.last_mut() {
        for ii in 0..=count {
            let mut end_arc = arc_start;

            if ii < count {
                let rot = if clockwise {
                    increment_angle * ii as f64
                } else {
                    increment_angle * (count - ii) as f64
                };
                end_arc = rotate_point(end_arc, rot);
            } else {
                end_arc = if clockwise { arc_end } else { arc_start };
            }

            append_polygon_point(outline, add(end_arc, center));
        }
    }
}

fn new_draw_item(image: &GerberFileImage) -> DrawItem {
    let mut item = DrawItem::new();
    item.units_metric = image.gerb_metric;
    item.swap_axis = image.swap_axis;
    item.mirror_a = image.mirror_a;
    item.mirror_b = image.mirror_b;
    item.draw_scale = image.scale;
    item.layer_offset = image.offset;
    item.lyr_rotation = image.local_rotation;
    item.image_justify_offset = image.image_justify_offset;
    item.image_offset = image.image_offset;
    item.image_rotation = image.image_rotation;
    item.display_offset = image.display_offset;
    item.display_rotation = image.display_rotation;
    item.layer_negative = image.layer_params.layer_negative;
    item
}

fn step_and_repeat_last_item(image: &mut GerberFileImage) {
    let item = image.drawings.last().cloned();
    if let Some(item) = item {
        step_and_repeat_item(image, &item);
    }
}

fn step_and_repeat_item(image: &mut GerberFileImage, item: &DrawItem) {
    if image.layer_params.x_repeat_count < 2 && image.layer_params.y_repeat_count < 2 {
        return;
    }

    for ii in 0..image.layer_params.x_repeat_count {
        for jj in 0..image.layer_params.y_repeat_count {
            if jj == 0 && ii == 0 {
                continue;
            }

            let mut dup_item = item.clone();
            let move_vector = Vec2I::new(
                scale_to_iu(
                    ii as f64 * image.layer_params.step_for_repeat.0,
                    image.layer_params.step_for_repeat_metric,
                ),
                scale_to_iu(
                    jj as f64 * image.layer_params.step_for_repeat.1,
                    image.layer_params.step_for_repeat_metric,
                ),
            );
            move_item(&mut dup_item, move_vector);
            image.drawings.push(dup_item);
        }
    }
}

fn move_item(item: &mut DrawItem, move_vector: Vec2I) {
    item.start = add(item.start, move_vector);
    item.end = add(item.end, move_vector);
    item.arc_centre = add(item.arc_centre, move_vector);

    for outline in &mut item.shape_as_polygon {
        for point in outline {
            *point = add(*point, move_vector);
        }
    }

    for outline in &mut item.absolute_polygon {
        for point in outline {
            *point = add(*point, move_vector);
        }
    }

    item.macro_shape_polygon.move_by(move_vector);
}

fn close_last_polygon(image: &mut GerberFileImage) {
    if let Some(item) = image.drawings.last_mut() {
        let first = item
            .shape_as_polygon
            .first()
            .and_then(|outline| outline.first())
            .copied();

        if let (Some(first), Some(outline)) = (first, item.shape_as_polygon.last_mut()) {
            append_polygon_point(outline, first);
        }
    }
}

fn append_polygon_point(outline: &mut Vec<Vec2I>, point: Vec2I) {
    if outline.last().copied() != Some(point) {
        outline.push(point);
    }
}

fn get_arc_to_segment_count(radius: i32, error_max: i32, arc_angle: f64) -> i32 {
    let radius = radius.max(1);
    let error_max = error_max.max(1);
    let rel_error = error_max as f64 / radius as f64;
    let cos_arg = (1.0 - rel_error).clamp(-1.0, 1.0);
    let arc_increment =
        (180.0 / std::f64::consts::PI * cos_arg.acos() * 2.0).min(360.0 / MIN_SEGCOUNT_FOR_CIRCLE);
    let seg_count = (arc_angle.abs() / arc_increment).round() as i32;

    seg_count.max(2)
}

fn rotate_point(point: Vec2I, angle_degrees: f64) -> Vec2I {
    let angle = angle_degrees.to_radians();
    let sin = angle.sin();
    let cos = angle.cos();

    Vec2I::new(
        (point.x as f64 * cos - point.y as f64 * sin).round() as i32,
        (point.x as f64 * sin + point.y as f64 * cos).round() as i32,
    )
}

fn angle_degrees(point: Vec2I) -> f64 {
    (point.y as f64).atan2(point.x as f64).to_degrees()
}

fn euclidean_norm(point: Vec2I) -> i32 {
    ((point.x as f64).hypot(point.y as f64)).round() as i32
}

fn skip_to_end_of_block(text: &str, mut pos: usize) -> usize {
    while pos < text.len() && byte_at(text, pos) != Some(b'*') {
        pos += 1;
    }
    pos
}

fn byte_at(text: &str, pos: usize) -> Option<u8> {
    text.as_bytes().get(pos).copied()
}

fn add(a: Vec2I, b: Vec2I) -> Vec2I {
    Vec2I::new(a.x + b.x, a.y + b.y)
}

fn sub(a: Vec2I, b: Vec2I) -> Vec2I {
    Vec2I::new(a.x - b.x, a.y - b.y)
}

fn neg(a: Vec2I) -> Vec2I {
    Vec2I::new(-a.x, -a.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dcode::DCode;

    #[test]
    fn g_commands_update_parser_state() {
        let mut image = GerberFileImage::default();

        assert!(execute_g_command(&mut image, "", 0, GC_CIRCLE_NEG_INTERPOL).ok);
        assert_eq!(image.interpolation, Interpolation::ArcNeg);

        assert!(execute_g_command(&mut image, "", 0, GC_TURN_ON_360_INTERPOL).ok);
        assert!(image.arc_360_enabled);
        assert!(image.as_arc_g74g75_cmd);

        assert!(execute_g_command(&mut image, "", 0, GC_SPECIFY_MILLIMETERS).ok);
        assert!(image.gerb_metric);
    }

    #[test]
    fn code_number_advances_past_command_letter_when_digits_are_missing() {
        assert_eq!(code_number("D*", 0), (0, 1));
        assert_eq!(code_number("G X", 0), (0, 1));
    }

    #[test]
    fn code_number_matches_strtol_after_command_letter() {
        assert_eq!(code_number("D10*", 0), (10, 3));
        assert_eq!(code_number("G 54D10*", 0), (54, 4));
        assert_eq!(code_number("D-1*", 0), (-1, 3));
    }

    #[test]
    fn dcode_tool_selection_marks_existing_tool_in_use() {
        let mut image = GerberFileImage::default();
        image.aperture_list.insert(10, DCode::new(10));

        assert!(execute_dcode_command(&mut image, 10));
        assert_eq!(image.current_tool, 10);
        assert!(image.aperture_list.get(&10).unwrap().in_use);
    }

    #[test]
    fn d01_linear_creates_segment_item() {
        let mut image = GerberFileImage {
            current_tool: 10,
            previous_pos: Vec2I::new(0, 0),
            current_pos: Vec2I::new(10, 20),
            ..GerberFileImage::default()
        };
        let mut dcode = DCode::new(10);
        dcode.size = Vec2I::new(5, 5);
        image.aperture_list.insert(10, dcode);

        assert!(execute_dcode_command(&mut image, 1));
        assert_eq!(image.drawings.len(), 1);
        assert_eq!(image.drawings[0].start, Vec2I::new(0, 0));
        assert_eq!(image.drawings[0].end, Vec2I::new(10, 20));
        assert_eq!(image.drawings[0].dcode, 10);
        assert_eq!(image.previous_pos, Vec2I::new(10, 20));
    }

    #[test]
    fn d03_flash_creates_spot_item() {
        let mut image = GerberFileImage {
            current_tool: 10,
            current_pos: Vec2I::new(7, 8),
            ..GerberFileImage::default()
        };
        let mut dcode = DCode::new(10);
        dcode.apert_type = ApertureType::Rect;
        dcode.size = Vec2I::new(3, 4);
        image.aperture_list.insert(10, dcode);

        assert!(execute_dcode_command(&mut image, 3));
        assert_eq!(image.drawings[0].shape_type, ShapeType::SpotRect);
        assert!(image.drawings[0].flashed);
        assert_eq!(image.drawings[0].start, Vec2I::new(7, 8));
        assert_eq!(image.previous_pos, Vec2I::new(7, 8));
    }

    #[test]
    fn region_arc_runs_from_gerber_start_to_end() {
        let start = Vec2I::new(-741_329, 13_254_076);
        let end = Vec2I::new(-1_030_942, 13_152_113);
        let mut item = DrawItem::new();

        fill_arc_poly(
            &mut item,
            start,
            end,
            Vec2I::new(-289_613, 360_323),
            false,
            true,
            false,
            Default::default(),
        );

        let outline = &item.shape_as_polygon[0];
        assert_eq!(outline.first().copied(), Some(start));
        assert_eq!(outline.last().copied(), Some(end));
    }
}
