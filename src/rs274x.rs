//! RS-274X extended command parser.
//! Ported from KiCad gerbview/rs274x.cpp, adapted to the Rust data model.

use crate::am_param::AmParam;
use crate::am_primitive::{AmPrimitive, AmPrimitiveId};
use crate::aperture_macro::ApertureMacro;
use crate::coord::{GerberFileImage, Vec2I, read_double, read_int, scale_to_iu};
use crate::dcode::DCode;
use crate::netlist_metadata::{GBR_NETINFO_CMP, GBR_NETINFO_NET, GBR_NETINFO_PAD};
use crate::types::{ApertureHoleType, ApertureType, CommandState, Interpolation};
use crate::x2_attribute::{X2Attribute, X2AttributeFileFunction};

pub const fn code(x: u8, y: u8) -> i32 {
    ((x as i32) << 8) + y as i32
}

pub const AXIS_SELECT: i32 = code(b'A', b'S');
pub const FORMAT_STATEMENT: i32 = code(b'F', b'S');
pub const MIRROR_IMAGE: i32 = code(b'M', b'I');
pub const MODE_OF_UNITS: i32 = code(b'M', b'O');
pub const INCH: i32 = code(b'I', b'N');
pub const MILLIMETER: i32 = code(b'M', b'M');
pub const OFFSET: i32 = code(b'O', b'F');
pub const SCALE_FACTOR: i32 = code(b'S', b'F');
pub const IMAGE_JUSTIFY: i32 = code(b'I', b'J');
pub const IMAGE_NAME: i32 = code(b'I', b'N');
pub const IMAGE_OFFSET: i32 = code(b'I', b'O');
pub const IMAGE_POLARITY: i32 = code(b'I', b'P');
pub const IMAGE_ROTATION: i32 = code(b'I', b'R');
pub const AP_DEFINITION: i32 = code(b'A', b'D');
pub const AP_MACRO: i32 = code(b'A', b'M');
pub const FILE_ATTRIBUTE: i32 = code(b'T', b'F');
pub const NET_ATTRIBUTE: i32 = code(b'T', b'O');
pub const APERTURE_ATTRIBUTE: i32 = code(b'T', b'A');
pub const REMOVE_APERTURE_ATTRIBUTE: i32 = code(b'T', b'D');
pub const KNOCKOUT: i32 = code(b'K', b'O');
pub const STEP_AND_REPEAT: i32 = code(b'S', b'R');
pub const ROTATE: i32 = code(b'R', b'O');
pub const LOAD_POLARITY: i32 = code(b'L', b'P');
pub const LOAD_NAME: i32 = code(b'L', b'N');

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandResult {
    pub ok: bool,
    pub new_offset: usize,
}

pub fn read_x_command_id(text: &str, offset: usize) -> Option<(i32, usize)> {
    let bytes = text.as_bytes();

    if offset + 1 >= bytes.len() {
        return None;
    }

    Some((code(bytes[offset], bytes[offset + 1]), offset + 2))
}

pub fn read_rs274x_command(
    image: &mut GerberFileImage,
    text: &str,
    offset: usize,
) -> CommandResult {
    let bytes = text.as_bytes();
    let mut pos = offset;

    if pos < bytes.len() && bytes[pos] == b'%' {
        pos += 1;
    }

    loop {
        while pos < bytes.len() {
            match bytes[pos] {
                b'%' => {
                    image.command_state = CommandState::Idle;
                    return CommandResult {
                        ok: true,
                        new_offset: pos + 1,
                    };
                }
                b' ' | b'\r' | b'*' => pos += 1,
                b'\n' => {
                    image.line_num += 1;
                    pos += 1;
                }
                _ => {
                    let Some((command, after_id)) = read_x_command_id(text, pos) else {
                        return CommandResult {
                            ok: false,
                            new_offset: pos,
                        };
                    };

                    let result = execute_rs274x_command(image, command, text, after_id);

                    if !result.ok {
                        return result;
                    }

                    pos = result.new_offset;
                }
            }
        }

        return CommandResult {
            ok: false,
            new_offset: pos,
        };
    }
}

pub fn execute_rs274x_command(
    image: &mut GerberFileImage,
    command: i32,
    text: &str,
    offset: usize,
) -> CommandResult {
    let mut ok = true;
    let mut pos = offset;

    match command {
        FORMAT_STATEMENT => {
            let mut x_fmt_known = false;
            let mut y_fmt_known = false;

            while pos < text.len() && byte_at(text, pos) != Some(b'*') {
                match byte_at(text, pos) {
                    Some(b' ') => pos += 1,
                    Some(b'D') => {
                        image.messages.push(format!(
                            "RS274X: Invalid GERBER format command 'D' at line {}",
                            image.line_num
                        ));
                        image.no_trailing_zeros = false;
                        pos += 1;
                    }
                    Some(b'L') => {
                        image.no_trailing_zeros = false;
                        pos += 1;
                    }
                    Some(b'T') => {
                        image.no_trailing_zeros = true;
                        pos += 1;
                    }
                    Some(b'A') => {
                        image.relative = false;
                        pos += 1;
                    }
                    Some(b'I') => {
                        image.relative = true;
                        pos += 1;
                    }
                    Some(b'G' | b'N') => {
                        pos += 1;
                        if pos < text.len() {
                            pos += 1;
                        }
                    }
                    Some(b'M') => {
                        pos += 1;
                        if matches!(byte_at(text, pos), Some(b'0'..=b'9')) {
                            pos += 1;
                        }
                    }
                    Some(b'X' | b'Y') => {
                        let axis = byte_at(text, pos).unwrap();
                        pos += 1;

                        if pos + 1 < text.len() {
                            let int_digits = byte_at(text, pos).unwrap_or(0) as i32 - b'0' as i32;
                            pos += 1;
                            let mut dec_digits =
                                byte_at(text, pos).unwrap_or(0) as i32 - b'0' as i32;
                            let fmt_len = int_digits + dec_digits;
                            pos += 1;

                            dec_digits = dec_digits.clamp(0, 7);

                            if axis == b'X' {
                                x_fmt_known = true;
                                image.fmt_scale.x = dec_digits;
                                image.fmt_len.x = fmt_len;
                            } else {
                                y_fmt_known = true;
                                image.fmt_scale.y = dec_digits;
                                image.fmt_len.y = fmt_len;
                            }
                        }
                    }
                    Some(other) => {
                        image
                            .messages
                            .push(format!("Unknown id ({}) in FS command", other as char));
                        ok = false;
                        pos = skip_to_end_of_block(text, pos);
                        break;
                    }
                    None => break,
                }
            }

            if !x_fmt_known || !y_fmt_known {
                image
                    .messages
                    .push("RS274X: Format Statement (FS) without X or Y format".to_string());
            }
        }

        AXIS_SELECT => {
            image.swap_axis = text[pos..]
                .get(..4)
                .is_some_and(|s| s.eq_ignore_ascii_case("AYBX"));
        }

        MIRROR_IMAGE => {
            image.mirror_a = false;
            image.mirror_b = false;

            while pos < text.len() && byte_at(text, pos) != Some(b'*') {
                match byte_at(text, pos) {
                    Some(b'A') => {
                        pos += 1;
                        image.mirror_a = byte_at(text, pos) == Some(b'1');
                    }
                    Some(b'B') => {
                        pos += 1;
                        image.mirror_b = byte_at(text, pos) == Some(b'1');
                    }
                    _ => pos += 1,
                }
            }
        }

        MODE_OF_UNITS => {
            if let Some((unit, after_unit)) = read_x_command_id(text, pos) {
                if unit == INCH {
                    image.gerb_metric = false;
                } else if unit == MILLIMETER {
                    image.gerb_metric = true;
                }
                pos = after_unit;
            }
        }

        FILE_ATTRIBUTE => {
            let (attr, consumed) = parse_attribute(text, pos);
            pos += consumed;

            if attr.is_file_function() {
                image.file_function = Some(X2AttributeFileFunction::new(&attr));
                image.is_x2_file = true;
            } else if attr.is_file_md5() {
                image.md5_value = attr.get_prm(1).to_string();
            } else if attr.is_file_part() {
                image.part_string = attr.get_prm(1).to_string();
            }
        }

        APERTURE_ATTRIBUTE => {
            let (attr, consumed) = parse_attribute(text, pos);
            pos += consumed;

            if attr.get_attribute().eq_ignore_ascii_case(".AperFunction") {
                image.aper_function = attr.get_prm(1).to_string();

                for idx in 2..attr.get_prm_count() {
                    image.aper_function.push(',');
                    image.aper_function.push_str(attr.get_prm(idx));
                }
            }
        }

        NET_ATTRIBUTE => {
            let (attr, consumed) = parse_attribute(text, pos);
            pos += consumed;

            if attr.get_attribute().eq_ignore_ascii_case(".N") {
                image.net_attribute_dict.net_attrib_type |= GBR_NETINFO_NET;
                image.net_attribute_dict.netname = format_string_from_gerber(attr.get_prm(1));
            } else if attr.get_attribute().eq_ignore_ascii_case(".C") {
                image.net_attribute_dict.net_attrib_type |= GBR_NETINFO_CMP;
                image.net_attribute_dict.cmpref = format_string_from_gerber(attr.get_prm(1));
            } else if attr.get_attribute().eq_ignore_ascii_case(".P") {
                image.net_attribute_dict.net_attrib_type |= GBR_NETINFO_PAD;
                image.net_attribute_dict.cmpref = format_string_from_gerber(attr.get_prm(1));
                image.net_attribute_dict.padname = format_string_from_gerber(attr.get_prm(2));

                if attr.get_prm_count() > 3 {
                    image.net_attribute_dict.pad_pin_function =
                        format_string_from_gerber(attr.get_prm(3));
                } else {
                    image.net_attribute_dict.pad_pin_function.clear();
                }
            }
        }

        REMOVE_APERTURE_ATTRIBUTE => {
            let (attr, consumed) = parse_attribute(text, pos);
            pos += consumed;
            remove_attribute(image, attr.get_prm(0));
        }

        OFFSET => {
            image.offset = Vec2I::new(0, 0);

            while pos < text.len() && byte_at(text, pos) != Some(b'*') {
                match byte_at(text, pos) {
                    Some(b'A') => {
                        pos += 1;
                        let (value, new_pos) = read_double(text, pos, true);
                        pos = new_pos;
                        image.offset.x = scale_to_iu(value, image.gerb_metric);
                    }
                    Some(b'B') => {
                        pos += 1;
                        let (value, new_pos) = read_double(text, pos, true);
                        pos = new_pos;
                        image.offset.y = scale_to_iu(value, image.gerb_metric);
                    }
                    _ => pos += 1,
                }
            }
        }

        SCALE_FACTOR => {
            image.scale = (1.0, 1.0);

            while pos < text.len() && byte_at(text, pos) != Some(b'*') {
                match byte_at(text, pos) {
                    Some(b'A') => {
                        pos += 1;
                        let (value, new_pos) = read_double(text, pos, true);
                        pos = new_pos;
                        image.scale.0 = value;
                    }
                    Some(b'B') => {
                        pos += 1;
                        let (value, new_pos) = read_double(text, pos, true);
                        pos = new_pos;
                        image.scale.1 = value;
                    }
                    _ => pos += 1,
                }
            }
        }

        IMAGE_OFFSET => {
            image.image_offset = Vec2I::new(0, 0);

            while pos < text.len() && byte_at(text, pos) != Some(b'*') {
                match byte_at(text, pos) {
                    Some(b'A') => {
                        pos += 1;
                        let (value, new_pos) = read_double(text, pos, true);
                        pos = new_pos;
                        image.image_offset.x = scale_to_iu(value, image.gerb_metric);
                    }
                    Some(b'B') => {
                        pos += 1;
                        let (value, new_pos) = read_double(text, pos, true);
                        pos = new_pos;
                        image.image_offset.y = scale_to_iu(value, image.gerb_metric);
                    }
                    _ => pos += 1,
                }
            }
        }

        IMAGE_ROTATION => {
            if text[pos..].starts_with("0*") {
                image.image_rotation = 0;
            } else if text[pos..].starts_with("90*") {
                image.image_rotation = 90;
            } else if text[pos..].starts_with("180*") {
                image.image_rotation = 180;
            } else if text[pos..].starts_with("270*") {
                image.image_rotation = 270;
            } else {
                image
                    .messages
                    .push("RS274X: Command \"IR\" rotation value not allowed".to_string());
            }
        }

        STEP_AND_REPEAT => {
            image.interpolation = Interpolation::Linear1x;
            image.layer_params.step_for_repeat.0 = 0.0;
            image.layer_params.step_for_repeat.0 = 0.0;
            image.layer_params.x_repeat_count = 1;
            image.layer_params.y_repeat_count = 1;
            image.layer_params.step_for_repeat_metric = image.gerb_metric;

            while pos < text.len() && byte_at(text, pos) != Some(b'*') {
                match byte_at(text, pos) {
                    Some(b'I') => {
                        pos += 1;
                        let (value, new_pos) = read_double(text, pos, true);
                        pos = new_pos;
                        image.layer_params.step_for_repeat.0 = value;
                    }
                    Some(b'J') => {
                        pos += 1;
                        let (value, new_pos) = read_double(text, pos, true);
                        pos = new_pos;
                        image.layer_params.step_for_repeat.1 = value;
                    }
                    Some(b'X') => {
                        pos += 1;
                        let (value, new_pos) = read_int(text, pos, true);
                        pos = new_pos;
                        image.layer_params.x_repeat_count = value;
                    }
                    Some(b'Y') => {
                        pos += 1;
                        let (value, new_pos) = read_int(text, pos, true);
                        pos = new_pos;
                        image.layer_params.y_repeat_count = value;
                    }
                    _ => pos += 1,
                }
            }
        }

        IMAGE_JUSTIFY => {
            image.image_justify_x_center = false;
            image.image_justify_y_center = false;
            image.image_justify_offset = Vec2I::new(0, 0);

            while pos < text.len() && byte_at(text, pos) != Some(b'*') {
                match byte_at(text, pos) {
                    Some(b'A') => {
                        pos += 1;
                        if matches!(byte_at(text, pos), Some(b'C' | b'L')) {
                            image.image_justify_x_center = true;
                            pos += 1;
                        } else {
                            let (value, new_pos) = read_double(text, pos, true);
                            pos = new_pos;
                            image.image_justify_offset.x = scale_to_iu(value, image.gerb_metric);
                        }
                    }
                    Some(b'B') => {
                        pos += 1;
                        if matches!(byte_at(text, pos), Some(b'C' | b'L')) {
                            image.image_justify_y_center = true;
                            pos += 1;
                        } else {
                            let (value, new_pos) = read_double(text, pos, true);
                            pos = new_pos;
                            image.image_justify_offset.y = scale_to_iu(value, image.gerb_metric);
                        }
                    }
                    _ => pos += 1,
                }
            }

            if image.image_justify_x_center {
                image.image_justify_offset.x = 0;
            }
            if image.image_justify_y_center {
                image.image_justify_offset.y = 0;
            }
        }

        KNOCKOUT => {
            image.interpolation = Interpolation::Linear1x;
            image
                .messages
                .push("RS274X: Command KNOCKOUT ignored by GerbView".to_string());
        }

        ROTATE => {
            image.interpolation = Interpolation::Linear1x;
            let (value, new_pos) = read_double(text, pos, true);
            image.local_rotation = value;
            pos = new_pos;
        }

        IMAGE_NAME => {
            let start = pos;
            pos = skip_to_end_of_block(text, pos);
            image.image_name = text[start..pos].to_string();
        }

        LOAD_NAME => {
            pos = skip_to_end_of_block(text, pos);
        }

        IMAGE_POLARITY => {
            if text[pos..]
                .get(..3)
                .is_some_and(|s| s.eq_ignore_ascii_case("NEG"))
            {
                image.image_negative = true;
                image
                    .messages
                    .push("IPNEG Gerber command is deprecated since 2012. Skip it".to_string());
            } else {
                image.image_negative = false;
            }
        }

        LOAD_POLARITY => {
            image.layer_params.layer_negative = byte_at(text, pos) == Some(b'C');
        }

        AP_MACRO => {
            let (aperture_macro, new_pos) = read_aperture_macro(image, text, pos);
            pos = new_pos;

            if let Some(aperture_macro) = aperture_macro {
                image
                    .aperture_macros
                    .insert(aperture_macro.am_name.clone(), aperture_macro);
            } else {
                ok = false;
            }
        }

        AP_DEFINITION => {
            let result = read_aperture_definition(image, text, pos);
            ok = result.ok;
            pos = result.new_offset;
        }

        _ => {
            ok = false;
        }
    }

    pos = skip_to_end_of_block(text, pos);

    CommandResult {
        ok,
        new_offset: pos,
    }
}

pub fn read_aperture_macro(
    image: &mut GerberFileImage,
    text: &str,
    offset: usize,
) -> (Option<ApertureMacro>, usize) {
    let mut pos = offset;
    let mut aperture_macro = ApertureMacro::new();

    let name_start = pos;
    while pos < text.len() && byte_at(text, pos) != Some(b'*') {
        pos += 1;
    }
    aperture_macro.am_name = text[name_start..pos].to_string();

    if byte_at(text, pos) == Some(b'*') {
        pos += 1;
    }

    loop {
        pos = skip_space_and_line_end(text, pos);

        if byte_at(text, pos) == Some(b'*') {
            pos += 1;
            pos = skip_space_and_line_end(text, pos);
        }

        match byte_at(text, pos) {
            Some(b'%') | None => break,
            Some(b'$') => {
                aperture_macro.add_local_param_def_to_stack();
                let mut slice = &text[pos..];

                if let Some(param) = AmParam::read_param_from_am_def(&mut slice) {
                    if let Some(last_param) = aperture_macro.get_last_local_param_def_from_stack() {
                        *last_param = param;
                    }

                    pos = text.len() - slice.len();
                } else {
                    pos = skip_to_end_of_block(text, pos);
                }
            }
            Some(b'0') => {
                pos = skip_to_end_of_block(text, pos);
            }
            Some(b'1'..=b'9') => {
                let (primitive_id, new_pos) = read_int(text, pos, true);
                pos = new_pos;

                let mut param_count = match primitive_id {
                    1 => 4,
                    2 | 20 => 7,
                    21 | 22 => 6,
                    4 => 4,
                    5 => 6,
                    6 => 9,
                    7 => 6,
                    _ => {
                        image.messages.push(format!(
                            "RS274X: Aperture Macro \"{}\": Invalid primitive id code {}",
                            aperture_macro.am_name, primitive_id
                        ));
                        return (None, skip_to_end_of_block(text, pos));
                    }
                };

                let mut primitive =
                    AmPrimitive::new(image.gerb_metric, AmPrimitiveId::from_i32(primitive_id));

                let mut ii = 0;
                while ii < param_count
                    && pos < text.len()
                    && !matches!(byte_at(text, pos), Some(b'*' | b'%'))
                {
                    let mut slice = &text[pos..];
                    primitive
                        .params
                        .push(AmParam::read_param_from_am_def(&mut slice).unwrap_or_default());
                    pos = text.len() - slice.len();
                    ii += 1;
                }

                if ii < param_count {
                    image.messages.push(format!(
                        "RS274X: read macro descr type {:?}: read {} parameters, insufficient parameters\n",
                        primitive.primitive_id, ii
                    ));
                }

                if primitive.primitive_id == AmPrimitiveId::Outline && primitive.params.len() > 1 {
                    param_count = (primitive.params[1].get_value_from_macro(&[]) as i32) * 2 + 1;

                    for _ in 0..param_count {
                        if matches!(byte_at(text, pos), Some(b'*' | b'%') | None) {
                            break;
                        }

                        let mut slice = &text[pos..];
                        primitive
                            .params
                            .push(AmParam::read_param_from_am_def(&mut slice).unwrap_or_default());
                        pos = text.len() - slice.len();
                    }
                }

                if primitive.primitive_id == AmPrimitiveId::Circle
                    && !matches!(byte_at(text, pos), Some(b'*' | b'%') | None)
                {
                    let mut slice = &text[pos..];
                    primitive
                        .params
                        .push(AmParam::read_param_from_am_def(&mut slice).unwrap_or_default());
                    pos = text.len() - slice.len();
                }

                aperture_macro.add_primitive_to_list(primitive);
            }
            Some(_) => {
                image.messages.push(format!(
                    "RS274X: Aperture Macro \"{}\": ill. symbol",
                    aperture_macro.am_name
                ));
                pos = skip_to_end_of_block(text, pos);
            }
        }

        if byte_at(text, pos) == Some(b'*') {
            pos += 1;
        }
    }

    (Some(aperture_macro), pos)
}

fn read_aperture_definition(
    image: &mut GerberFileImage,
    text: &str,
    offset: usize,
) -> CommandResult {
    let mut pos = offset;

    if byte_at(text, pos) != Some(b'D') {
        return CommandResult {
            ok: false,
            new_offset: pos,
        };
    }

    pos += 1;
    image.has_dcode = true;

    let (dcode_num, new_pos) = read_int(text, pos, true);
    pos = new_pos;

    let aper_function = image.aper_function.clone();
    let gerb_metric = image.gerb_metric;
    let macros = image.aperture_macros.clone();
    let messages = &mut image.messages;

    let dcode = image
        .aperture_list
        .entry(dcode_num)
        .or_insert_with(|| DCode::new(dcode_num));
    dcode.clear();
    dcode.aper_function = aper_function;

    if pos + 1 < text.len() && byte_at(text, pos + 1) == Some(b',') {
        let std_aperture = byte_at(text, pos).unwrap_or_default().to_ascii_uppercase();
        pos += 2;

        let (size_x, new_pos) = read_double(text, pos, true);
        pos = new_pos;
        dcode.size.x = scale_to_iu(size_x, gerb_metric);
        dcode.size.y = dcode.size.x;

        match std_aperture {
            b'C' => {
                dcode.apert_type = ApertureType::Circle;
                pos = read_optional_drill(dcode, text, pos, gerb_metric);
                dcode.defined = true;
            }
            b'O' | b'R' => {
                dcode.apert_type = if std_aperture == b'O' {
                    ApertureType::Oval
                } else {
                    ApertureType::Rect
                };

                pos = skip_spaces(text, pos);
                if byte_at(text, pos) == Some(b'X') {
                    pos += 1;
                    let (size_y, new_pos) = read_double(text, pos, true);
                    pos = new_pos;
                    dcode.size.y = scale_to_iu(size_y, gerb_metric);
                }

                pos = read_optional_drill(dcode, text, pos, gerb_metric);
                dcode.defined = true;
            }
            b'P' => {
                dcode.apert_type = ApertureType::Polygon;

                pos = skip_spaces(text, pos);
                if byte_at(text, pos) == Some(b'X') {
                    pos += 1;
                    let (edges, new_pos) = read_int(text, pos, true);
                    pos = new_pos;
                    dcode.edges_count = edges;
                }

                pos = skip_spaces(text, pos);
                if byte_at(text, pos) == Some(b'X') {
                    pos += 1;
                    let (rotation, new_pos) = read_double(text, pos, true);
                    pos = new_pos;
                    dcode.rotation = rotation;
                }

                pos = read_optional_drill(dcode, text, pos, gerb_metric);
                dcode.defined = true;
                dcode.convert_shape_to_polygon();
            }
            _ => {}
        }

        if dcode.defined && dcode.polyset.outline_count() == 0 {
            dcode.convert_shape_to_polygon();
        }
    } else {
        let name_start = pos;
        while pos < text.len() && !matches!(byte_at(text, pos), Some(b'*' | b',' | b'%')) {
            pos += 1;
        }
        let macro_name = text[name_start..pos].to_string();

        if byte_at(text, pos) == Some(b',') {
            pos += 1;

            while pos < text.len() && !matches!(byte_at(text, pos), Some(b'*' | b'%')) {
                let before = pos;
                let (value, new_pos) = read_double(text, pos, true);
                pos = new_pos;
                dcode.append_param(value);

                if pos == before {
                    messages.push(format!(
                        "RS274X: aperture macro {} has invalid template parameters",
                        macro_name
                    ));
                    return CommandResult {
                        ok: false,
                        new_offset: skip_to_end_of_block(text, pos),
                    };
                }

                pos = skip_c_isspace(text, pos);
                if matches!(byte_at(text, pos), Some(b'X' | b'x')) {
                    pos += 1;
                }
            }
        }

        if macros.contains_key(&macro_name) {
            dcode.apert_type = ApertureType::Macro;
            dcode.macro_name = macro_name.clone();
            dcode.defined = true;
        } else {
            messages.push(format!("RS274X: aperture macro {} not found", macro_name));
            return CommandResult {
                ok: false,
                new_offset: skip_to_end_of_block(text, pos),
            };
        }
    }

    CommandResult {
        ok: true,
        new_offset: pos,
    }
}

fn read_optional_drill(dcode: &mut DCode, text: &str, mut pos: usize, gerb_metric: bool) -> usize {
    pos = skip_spaces(text, pos);

    if byte_at(text, pos) == Some(b'X') {
        pos += 1;
        let (drill, new_pos) = read_double(text, pos, true);
        pos = new_pos;
        dcode.drill.x = scale_to_iu(drill, gerb_metric);
        dcode.drill.y = dcode.drill.x;
        dcode.drill_shape = ApertureHoleType::RoundHole;
    }

    pos = skip_spaces(text, pos);

    if byte_at(text, pos) == Some(b'X') {
        pos += 1;
        let (drill_y, new_pos) = read_double(text, pos, true);
        pos = new_pos;
        dcode.drill.y = scale_to_iu(drill_y, gerb_metric);
        dcode.drill_shape = ApertureHoleType::RectHole;
    }

    pos
}

fn parse_attribute(text: &str, pos: usize) -> (X2Attribute, usize) {
    let original = &text[pos..];
    let mut slice = original;
    let attr = X2Attribute::parse_attrib_cmd(&mut slice);
    (attr, original.len() - slice.len())
}

fn remove_attribute(image: &mut GerberFileImage, command: &str) {
    if command.is_empty() {
        image.net_attribute_dict.clear();
        image.aper_function.clear();
        return;
    }

    if command.eq_ignore_ascii_case(".N") {
        image.net_attribute_dict.net_attrib_type &= !GBR_NETINFO_NET;
        image.net_attribute_dict.netname.clear();
    } else if command.eq_ignore_ascii_case(".C") {
        image.net_attribute_dict.net_attrib_type &= !GBR_NETINFO_CMP;
        image.net_attribute_dict.cmpref.clear();
    } else if command.eq_ignore_ascii_case(".P") {
        image.net_attribute_dict.net_attrib_type &= !GBR_NETINFO_PAD;
        image.net_attribute_dict.padname.clear();
        image.net_attribute_dict.pad_pin_function.clear();
    }

    if command.eq_ignore_ascii_case(".AperFunction") {
        image.aper_function.clear();
    }
}

fn skip_to_end_of_block(text: &str, mut pos: usize) -> usize {
    while pos < text.len() && !matches!(byte_at(text, pos), Some(b'*' | b'%')) {
        pos += 1;
    }
    pos
}

fn skip_space_and_line_end(text: &str, mut pos: usize) -> usize {
    while matches!(byte_at(text, pos), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        pos += 1;
    }
    pos
}

fn skip_spaces(text: &str, mut pos: usize) -> usize {
    while matches!(byte_at(text, pos), Some(b' ')) {
        pos += 1;
    }
    pos
}

fn skip_c_isspace(text: &str, mut pos: usize) -> usize {
    while byte_at(text, pos).is_some_and(|byte| byte.is_ascii_whitespace()) {
        pos += 1;
    }
    pos
}

fn byte_at(text: &str, pos: usize) -> Option<u8> {
    text.as_bytes().get(pos).copied()
}

fn format_string_from_gerber(value: &str) -> String {
    value.replace("\\,", ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_x_command_id_uses_high_byte_first() {
        assert_eq!(
            read_x_command_id("FSLAX24Y24*", 0),
            Some((FORMAT_STATEMENT, 2))
        );
    }

    #[test]
    fn fs_updates_coordinate_format() {
        let mut image = GerberFileImage::default();
        let result = execute_rs274x_command(&mut image, FORMAT_STATEMENT, "LAX24Y35*", 0);

        assert!(result.ok);
        assert_eq!(image.no_trailing_zeros, false);
        assert_eq!(image.relative, false);
        assert_eq!(image.fmt_scale, Vec2I::new(4, 5));
        assert_eq!(image.fmt_len, Vec2I::new(6, 8));
    }

    #[test]
    fn fs_rejects_tab_like_kicad_format_statement_loop() {
        let mut image = GerberFileImage::default();
        let result = execute_rs274x_command(&mut image, FORMAT_STATEMENT, "\tLAX24Y24*", 0);

        assert!(!result.ok);
        assert!(
            image
                .messages
                .iter()
                .any(|message| message == "Unknown id (\t) in FS command")
        );
    }

    #[test]
    fn fs_sequence_code_consumes_next_character_even_when_not_digit_like_kicad() {
        let mut image = GerberFileImage::default();
        let result = execute_rs274x_command(&mut image, FORMAT_STATEMENT, "NQLAX24Y24*", 0);

        assert!(result.ok);
        assert_eq!(image.fmt_scale, Vec2I::new(4, 4));
        assert_eq!(image.fmt_len, Vec2I::new(6, 6));
    }

    #[test]
    fn fs_clamps_fmt_scale_after_fmt_len_calculation_like_kicad() {
        let mut image = GerberFileImage::default();
        let result = execute_rs274x_command(&mut image, FORMAT_STATEMENT, "LAX29Y24*", 0);

        assert!(result.ok);
        assert_eq!(image.fmt_scale.x, 7);
        assert_eq!(image.fmt_len.x, 11);
    }

    #[test]
    fn mo_updates_units() {
        let mut image = GerberFileImage::default();
        execute_rs274x_command(&mut image, MODE_OF_UNITS, "MM*", 0);
        assert!(image.gerb_metric);

        execute_rs274x_command(&mut image, MODE_OF_UNITS, "IN*", 0);
        assert!(!image.gerb_metric);
    }

    #[test]
    fn sf_preserves_fractional_scale_like_kicad() {
        let mut image = GerberFileImage::default();
        let result = execute_rs274x_command(&mut image, SCALE_FACTOR, "A1.5B0.25*", 0);

        assert!(result.ok);
        assert_eq!(image.scale, (1.5, 0.25));
    }

    #[test]
    fn sr_preserves_previous_y_step_when_j_is_missing_like_kicad() {
        let mut image = GerberFileImage::default();
        image.layer_params.step_for_repeat = (4.0, 9.0);

        let result = execute_rs274x_command(&mut image, STEP_AND_REPEAT, "Y2*", 0);

        assert!(result.ok);
        assert_eq!(image.layer_params.step_for_repeat, (0.0, 9.0));
        assert_eq!(image.layer_params.x_repeat_count, 1);
        assert_eq!(image.layer_params.y_repeat_count, 2);
    }

    #[test]
    fn ad_circle_creates_defined_dcode() {
        let mut image = GerberFileImage {
            gerb_metric: true,
            ..GerberFileImage::default()
        };
        let result = execute_rs274x_command(&mut image, AP_DEFINITION, "D10C,1.5X0.3*", 0);

        assert!(result.ok);
        let dcode = image.aperture_list.get(&10).unwrap();
        assert_eq!(dcode.apert_type, ApertureType::Circle);
        assert_eq!(dcode.size, Vec2I::new(150_000, 150_000));
        assert_eq!(dcode.drill, Vec2I::new(30_000, 30_000));
        assert_eq!(dcode.drill_shape, ApertureHoleType::RoundHole);
        assert!(dcode.defined);
    }

    #[test]
    fn ad_dcode_below_first_dcode_is_created_like_kicad_get_or_create_path() {
        let mut image = GerberFileImage::default();
        let result = execute_rs274x_command(&mut image, AP_DEFINITION, "D9C,0.1*", 0);

        assert!(result.ok);
        assert!(image.has_dcode);
        assert!(
            image
                .aperture_list
                .get(&9)
                .is_some_and(|dcode| dcode.defined)
        );
        assert!(image.messages.is_empty());
    }

    #[test]
    fn ad_standard_aperture_does_not_skip_second_tab_before_optional_x_like_kicad() {
        let mut image = GerberFileImage::default();
        let result = execute_rs274x_command(&mut image, AP_DEFINITION, "D10C,0.1\t\tX0.02*", 0);

        assert!(result.ok);
        let dcode = image.aperture_list.get(&10).unwrap();
        assert_eq!(dcode.drill, Vec2I::new(0, 0));
        assert_eq!(dcode.drill_shape, ApertureHoleType::NoHole);
    }

    #[test]
    fn ad_macro_parameters_skip_c_isspace_like_kicad() {
        let mut image = GerberFileImage::default();
        image.aperture_macros.insert(
            "MAC".to_string(),
            crate::aperture_macro::ApertureMacro::new(),
        );

        let result = execute_rs274x_command(&mut image, AP_DEFINITION, "D10MAC,0.1\tX0.2*", 0);

        assert!(result.ok);
        let dcode = image.aperture_list.get(&10).unwrap();
        assert_eq!(dcode.am_params, vec![0.1, 0.2]);
    }

    #[test]
    fn read_rs274x_command_runs_multiple_commands_until_percent() {
        let mut image = GerberFileImage::default();
        let result = read_rs274x_command(&mut image, "%FSLAX24Y24*MOMM*%", 0);

        assert!(result.ok);
        assert_eq!(result.new_offset, 18);
        assert!(image.gerb_metric);
        assert_eq!(image.fmt_scale, Vec2I::new(4, 4));
    }

    #[test]
    fn read_rs274x_command_does_not_skip_tab_like_kicad_outer_loop() {
        let mut image = GerberFileImage::default();
        let result = read_rs274x_command(&mut image, "%\tFSLAX24Y24*%", 0);

        assert!(!result.ok);
        assert_eq!(result.new_offset, 12);
        assert_eq!(image.fmt_scale, GerberFileImage::default().fmt_scale);
    }

    #[test]
    fn read_rs274x_command_counts_multiline_command_lines_like_kicad() {
        let mut image = GerberFileImage {
            line_num: 1,
            ..GerberFileImage::default()
        };

        let result = read_rs274x_command(&mut image, "%\nFSDAX24Y24*%", 0);

        assert!(result.ok);
        assert_eq!(image.line_num, 2);
        assert!(
            image
                .messages
                .iter()
                .any(|message| message.contains("line 2"))
        );
    }

    #[test]
    fn unknown_rs274x_command_fails_after_scanning_to_end_of_block_like_kicad() {
        let mut image = GerberFileImage::default();
        let result = execute_rs274x_command(&mut image, code(b'Z', b'Z'), "payload*next", 0);

        assert!(!result.ok);
        assert_eq!(result.new_offset, 7);
    }

    #[test]
    fn aperture_macro_keeps_x_as_multiplication_inside_parameter() {
        let mut image = GerberFileImage {
            gerb_metric: true,
            ..GerberFileImage::default()
        };
        let (aperture_macro, _) = read_aperture_macro(&mut image, "M*1,1,0.5X2,0,0*%", 0);
        let aperture_macro = aperture_macro.unwrap();
        let primitive = &aperture_macro.primitives_list[0];

        assert_eq!(primitive.params.len(), 4);
        assert_eq!(primitive.params[1].get_value_from_macro(&[]), 1.0);
    }

    #[test]
    fn aperture_macro_circle_reads_optional_rotation_after_minimum_params() {
        let mut image = GerberFileImage::default();
        let (aperture_macro, _) = read_aperture_macro(&mut image, "M*1,1,1,0,0,45*%", 0);
        let aperture_macro = aperture_macro.unwrap();

        assert_eq!(aperture_macro.primitives_list[0].params.len(), 5);
        assert_eq!(
            aperture_macro.primitives_list[0].params[4].get_value_from_macro(&[]),
            45.0
        );
    }

    #[test]
    fn aperture_macro_outline_reads_extra_points_and_rotation_from_corner_count() {
        let mut image = GerberFileImage::default();
        let (aperture_macro, _) = read_aperture_macro(&mut image, "M*4,1,2,0,0,1,0,1,1,30*%", 0);
        let aperture_macro = aperture_macro.unwrap();

        assert_eq!(aperture_macro.primitives_list[0].params.len(), 9);
        assert_eq!(
            aperture_macro.primitives_list[0].params[8].get_value_from_macro(&[]),
            30.0
        );
    }

    #[test]
    fn aperture_macro_invalid_primitive_id_fails_like_kicad() {
        let mut image = GerberFileImage::default();
        let (aperture_macro, _) = read_aperture_macro(&mut image, "M*9,1,2,3*%", 0);

        assert!(aperture_macro.is_none());
        assert!(image.messages[0].contains("Invalid primitive id code 9"));
    }
}
