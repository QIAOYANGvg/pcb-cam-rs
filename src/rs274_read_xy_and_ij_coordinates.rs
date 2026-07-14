pub const GERB_IU_PER_MM: f64 = 1e5;
const SCALE_LIST_SIZE: usize = 9;
const SCALE_LIST: [f64; SCALE_LIST_SIZE] = [
    1000.0 * GERB_IU_PER_MM * 0.0254,
    100.0 * GERB_IU_PER_MM * 0.0254,
    10.0 * GERB_IU_PER_MM * 0.0254,
    1.0 * GERB_IU_PER_MM * 0.0254,
    0.1 * GERB_IU_PER_MM * 0.0254,
    0.01 * GERB_IU_PER_MM * 0.0254,
    0.001 * GERB_IU_PER_MM * 0.0254,
    0.0001 * GERB_IU_PER_MM * 0.0254,
    0.00001 * GERB_IU_PER_MM * 0.0254,
];

use crate::geometry::Vec2I;
use crate::gerber_file_image::{GerberFileImage, LastExtraArcDataType};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadCoordResult {
    pub pos: Vec2I,
    pub new_offset: usize,
}

fn is_number(x: u8) -> bool {
    (x >= b'0' && x <= b'9') || x == b'-' || x == b'+' || x == b'.'
}

fn ki_round_i32(value: f64) -> i32 {
    if value.is_nan() {
        return 0;
    }

    let rounded = value.round();

    if rounded < i32::MIN as f64 {
        i32::MIN
    } else if rounded > i32::MAX as f64 {
        i32::MAX
    } else {
        rounded as i32
    }
}

pub fn scale_to_iu(coord: f64, is_metric: bool) -> i32 {
    let ret;

    if is_metric {
        ret = ki_round_i32(coord * GERB_IU_PER_MM);
    } else {
        ret = ki_round_i32(coord * GERB_IU_PER_MM * 25.4);
    }

    ret
}

pub fn read_int(text: &str, offset: usize, skip_separator: bool) -> (i32, usize) {
    let bytes = text.as_bytes();
    let mut index = offset;
    let ret;

    if text
        .get(index..)
        .is_some_and(|tail| tail.len() >= 2 && tail[..2].eq_ignore_ascii_case("0X"))
    {
        index += 1;
        ret = 0;
    } else {
        let start = index;

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
            index += 1;
        }

        let digit_start = index;

        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }

        if digit_start == index {
            index = start;
            ret = 0;
        } else {
            ret = text[start..index].trim_start().parse::<i32>().unwrap_or(0);
        }
    }

    if index < bytes.len() && (bytes[index] == b',' || bytes[index].is_ascii_whitespace()) {
        if skip_separator {
            index += 1;
        }
    }

    (ret, index)
}

pub fn read_double(text: &str, offset: usize, skip_separator: bool) -> (f64, usize) {
    let bytes = text.as_bytes();
    let mut index = offset;
    let ret;

    if text
        .get(index..)
        .is_some_and(|tail| tail.len() >= 2 && tail[..2].eq_ignore_ascii_case("0X"))
    {
        index += 1;
        ret = 0.0;
    } else {
        let mut line = text[index..].trim_start().to_string();
        line = line.replacen(',', " ", 1);
        ret = parse_c_double_prefix(&line);

        let mut scan = line.into_bytes();

        if (scan.first() == Some(&b'+') || scan.first() == Some(&b'-'))
            && scan.len() > 1
            && scan[1] != b'$'
        {
            scan[0] = b'0';
        }

        let endpos = scan
            .iter()
            .position(|ch| !ch.is_ascii_digit() && *ch != b'.')
            .unwrap_or(scan.len());

        index += endpos;
    }

    if index < bytes.len() && (bytes[index] == b',' || bytes[index].is_ascii_whitespace()) {
        if skip_separator {
            index += 1;
        }
    }

    (ret, index)
}

fn parse_c_double_prefix(line: &str) -> f64 {
    let bytes = line.as_bytes();
    let mut index = 0;

    if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
        index += 1;
    }

    let mut has_digit = false;

    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
        has_digit = true;
    }

    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;

        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
            has_digit = true;
        }
    }

    if !has_digit {
        0.0
    } else {
        line[..index].parse::<f64>().unwrap_or(0.0)
    }
}

impl GerberFileImage {
    pub fn read_xy_coord(
        &mut self,
        text: Option<&str>,
        offset: usize,
        excellon_mode: bool,
    ) -> ReadCoordResult {
        let mut pos = Vec2I::new(0, 0);
        let mut is_float = false;

        if !self.relative {
            pos = self.current_pos;
        }

        let Some(text) = text else {
            return ReadCoordResult {
                pos,
                new_offset: offset,
            };
        };

        let bytes = text.as_bytes();
        let mut index = offset;

        while index < bytes.len()
            && (bytes[index] == b'X' || bytes[index] == b'Y' || bytes[index] == b'A')
        {
            let mut decimal_scale = 1.0;
            let mut nbdigits = 0;
            let current_coord;
            let type_coord = bytes[index];
            index += 1;

            let line_start = index;

            while index < bytes.len() && is_number(bytes[index]) {
                if bytes[index] == b'.' {
                    is_float = true;
                }

                if bytes[index] >= b'0' && bytes[index] <= b'9' {
                    nbdigits += 1;
                }

                index += 1;
            }

            let val = text[line_start..index].parse::<f64>().unwrap_or(0.0);

            if is_float {
                current_coord = scale_to_iu(val, self.gerb_metric);
            } else {
                let fmt_scale = if type_coord == b'X' {
                    self.fmt_scale.x
                } else {
                    self.fmt_scale.y
                };

                if self.no_trailing_zeros {
                    let digit_count = if type_coord == b'X' {
                        self.fmt_len.x
                    } else {
                        self.fmt_len.y
                    };

                    if nbdigits < digit_count || (excellon_mode && nbdigits > digit_count) {
                        decimal_scale = 10_f64.powi(digit_count - nbdigits);
                    }
                }

                let mut real_scale = SCALE_LIST[fmt_scale as usize];

                if self.gerb_metric {
                    real_scale = real_scale / 25.4;
                }

                current_coord = ki_round_i32(val * real_scale * decimal_scale);
            }

            if type_coord == b'X' {
                pos.x = current_coord;
            } else if type_coord == b'Y' {
                pos.y = current_coord;
            } else if type_coord == b'A' {
                self.arc_radius = current_coord;
                self.last_arc_data_type = LastExtraArcDataType::Radius;
            }
        }

        if self.relative {
            pos.x += self.current_pos.x;
            pos.y += self.current_pos.y;
        }

        self.current_pos = pos;

        ReadCoordResult {
            pos,
            new_offset: index,
        }
    }

    pub fn read_ij_coord(&mut self, text: Option<&str>, offset: usize) -> ReadCoordResult {
        let mut pos = Vec2I::new(0, 0);
        let mut is_float = false;

        let Some(text) = text else {
            return ReadCoordResult {
                pos,
                new_offset: offset,
            };
        };

        let bytes = text.as_bytes();
        let mut index = offset;

        while index < bytes.len() && (bytes[index] == b'I' || bytes[index] == b'J') {
            let mut decimal_scale = 1.0;
            let mut nbdigits = 0;
            let current_coord;
            let type_coord = bytes[index];
            index += 1;

            let line_start = index;

            while index < bytes.len() && is_number(bytes[index]) {
                if bytes[index] == b'.' {
                    is_float = true;
                }

                if bytes[index] >= b'0' && bytes[index] <= b'9' {
                    nbdigits += 1;
                }

                index += 1;
            }

            let val = text[line_start..index].trim().parse::<f64>().unwrap_or(0.0);

            if is_float {
                current_coord = scale_to_iu(val, self.gerb_metric);
            } else {
                let fmt_scale = if type_coord == b'I' {
                    self.fmt_scale.x
                } else {
                    self.fmt_scale.y
                };

                if self.no_trailing_zeros {
                    let digit_count = if type_coord == b'I' {
                        self.fmt_len.x
                    } else {
                        self.fmt_len.y
                    };

                    if nbdigits < digit_count {
                        decimal_scale = 10_f64.powi(digit_count - nbdigits);
                    }
                }

                let mut real_scale = SCALE_LIST[fmt_scale as usize];

                if self.gerb_metric {
                    real_scale = real_scale / 25.4;
                }

                current_coord = ki_round_i32(val * real_scale * decimal_scale);
            }

            if type_coord == b'I' {
                pos.x = current_coord;
            } else if type_coord == b'J' {
                pos.y = current_coord;
            }
        }

        self.ij_pos = pos;
        self.last_arc_data_type = LastExtraArcDataType::Center;
        self.last_coord_is_ij_pos = true;

        ReadCoordResult {
            pos,
            new_offset: index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_to_iu_converts_metric_coordinates() {
        assert_eq!(scale_to_iu(1.0, true), 100_000);
        assert_eq!(scale_to_iu(0.5, true), 50_000);
        assert_eq!(scale_to_iu(0.001, true), 100);
    }

    #[test]
    fn scale_to_iu_converts_imperial_coordinates() {
        assert_eq!(scale_to_iu(1.0, false), 2_540_000);
        assert_eq!(scale_to_iu(0.1, false), 254_000);
        assert_eq!(scale_to_iu(0.001, false), 2_540);
    }

    #[test]
    fn scale_to_iu_rounds_halfway_cases_away_from_zero() {
        assert_eq!(scale_to_iu(0.000005, true), 1);
        assert_eq!(scale_to_iu(-0.000005, true), -1);
        assert_eq!(scale_to_iu(0.000004999, true), 0);
        assert_eq!(scale_to_iu(-0.000004999, true), 0);
    }

    #[test]
    fn scale_to_iu_handles_zero_and_small_coordinates() {
        assert_eq!(scale_to_iu(0.0, true), 0);
        assert_eq!(scale_to_iu(0.000000001, true), 0);
        assert_eq!(scale_to_iu(-0.000000001, true), 0);
    }

    #[test]
    fn scale_to_iu_clamps_extreme_values_like_kiround_int() {
        assert_eq!(scale_to_iu(30_000.0, false), i32::MAX);
        assert_eq!(scale_to_iu(-30_000.0, false), i32::MIN);
    }

    #[test]
    fn scale_to_iu_returns_zero_for_nan_like_kiround() {
        assert_eq!(scale_to_iu(f64::NAN, true), 0);
    }

    #[test]
    fn read_xy_coord_returns_current_position_for_none_text_in_absolute_mode() {
        let mut image = GerberFileImage {
            current_pos: Vec2I::new(10, 20),
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(None, 3, false);

        assert_eq!(result.pos, Vec2I::new(10, 20));
        assert_eq!(result.new_offset, 3);
        assert_eq!(image.current_pos, Vec2I::new(10, 20));
    }

    #[test]
    fn read_xy_coord_returns_origin_for_none_text_in_relative_mode() {
        let mut image = GerberFileImage {
            current_pos: Vec2I::new(10, 20),
            relative: true,
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(None, 3, false);

        assert_eq!(result.pos, Vec2I::new(0, 0));
        assert_eq!(result.new_offset, 3);
        assert_eq!(image.current_pos, Vec2I::new(10, 20));
    }

    #[test]
    fn read_xy_coord_parses_metric_floating_point_xy() {
        let mut image = GerberFileImage {
            gerb_metric: true,
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(Some("X1.5Y2.5D01*"), 0, false);

        assert_eq!(result.pos, Vec2I::new(150_000, 250_000));
        assert_eq!(result.new_offset, 8);
        assert_eq!(image.current_pos, Vec2I::new(150_000, 250_000));
    }

    #[test]
    fn read_xy_coord_parses_imperial_floating_point_xy() {
        let mut image = GerberFileImage::default();

        let result = image.read_xy_coord(Some("X1.0Y2.0*"), 0, false);

        assert_eq!(result.pos, Vec2I::new(2_540_000, 5_080_000));
        assert_eq!(result.new_offset, 8);
    }

    #[test]
    fn read_xy_coord_parses_integer_coordinates_with_leading_zero_suppression() {
        let mut image = GerberFileImage {
            fmt_scale: Vec2I::new(4, 4),
            fmt_len: Vec2I::new(6, 6),
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(Some("X150000Y100000*"), 0, false);

        assert_eq!(result.pos, Vec2I::new(38_100_000, 25_400_000));
        assert_eq!(result.new_offset, 14);
    }

    #[test]
    fn read_xy_coord_parses_integer_coordinates_with_trailing_zero_suppression() {
        let mut image = GerberFileImage {
            no_trailing_zeros: true,
            fmt_scale: Vec2I::new(4, 4),
            fmt_len: Vec2I::new(6, 6),
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(Some("X15Y10*"), 0, false);

        assert_eq!(result.pos, Vec2I::new(38_100_000, 25_400_000));
        assert_eq!(result.new_offset, 6);
    }

    #[test]
    fn read_xy_coord_applies_excellon_extra_digit_decimal_scale() {
        let mut image = GerberFileImage {
            no_trailing_zeros: true,
            fmt_scale: Vec2I::new(4, 4),
            fmt_len: Vec2I::new(4, 4),
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(Some("X123456*"), 0, true);

        assert_eq!(result.pos, Vec2I::new(313_578, 0));
        assert_eq!(result.new_offset, 7);
    }

    #[test]
    fn read_xy_coord_parses_metric_integer_coordinates() {
        let mut image = GerberFileImage {
            gerb_metric: true,
            fmt_scale: Vec2I::new(3, 3),
            fmt_len: Vec2I::new(6, 6),
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(Some("X1500Y2000*"), 0, false);

        assert_eq!(result.pos, Vec2I::new(150_000, 200_000));
        assert_eq!(result.new_offset, 10);
    }

    #[test]
    fn read_xy_coord_handles_relative_coordinates() {
        let mut image = GerberFileImage {
            gerb_metric: true,
            relative: true,
            current_pos: Vec2I::new(100_000, 200_000),
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(Some("X500.0Y-300.0*"), 0, false);

        assert_eq!(result.pos, Vec2I::new(50_100_000, -29_800_000));
        assert_eq!(image.current_pos, Vec2I::new(50_100_000, -29_800_000));
    }

    #[test]
    fn read_xy_coord_reads_arc_radius_a_coordinate() {
        let mut image = GerberFileImage {
            gerb_metric: true,
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(Some("A1.25X2.0*"), 0, false);

        assert_eq!(image.arc_radius, 125_000);
        assert_eq!(image.last_arc_data_type, LastExtraArcDataType::Radius);
        assert_eq!(result.pos, Vec2I::new(200_000, 0));
    }

    #[test]
    fn read_xy_coord_keeps_cpp_is_float_state_across_axes() {
        let mut image = GerberFileImage {
            gerb_metric: true,
            fmt_scale: Vec2I::new(4, 4),
            fmt_len: Vec2I::new(6, 6),
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(Some("X1.5Y2500*"), 0, false);

        assert_eq!(result.pos, Vec2I::new(150_000, 250_000_000));
    }

    #[test]
    fn read_xy_coord_empty_line_leaves_position_unchanged() {
        let mut image = GerberFileImage {
            current_pos: Vec2I::new(7, 8),
            ..GerberFileImage::default()
        };

        let result = image.read_xy_coord(Some("D01*"), 0, false);

        assert_eq!(result.pos, Vec2I::new(7, 8));
        assert_eq!(result.new_offset, 0);
        assert_eq!(image.current_pos, Vec2I::new(7, 8));
    }

    #[test]
    fn read_xy_coord_empty_d_code_axis_uses_zero_value() {
        let mut image = GerberFileImage::default();

        let result = image.read_xy_coord(Some("XD01*"), 0, false);

        assert_eq!(result.pos, Vec2I::new(0, 0));
        assert_eq!(result.new_offset, 1);
    }

    #[test]
    fn read_ij_coord_returns_origin_for_none_text_without_state_updates() {
        let mut image = GerberFileImage {
            ij_pos: Vec2I::new(7, 8),
            last_arc_data_type: LastExtraArcDataType::Radius,
            last_coord_is_ij_pos: false,
            ..GerberFileImage::default()
        };

        let result = image.read_ij_coord(None, 4);

        assert_eq!(result.pos, Vec2I::new(0, 0));
        assert_eq!(result.new_offset, 4);
        assert_eq!(image.ij_pos, Vec2I::new(7, 8));
        assert_eq!(image.last_arc_data_type, LastExtraArcDataType::Radius);
        assert!(!image.last_coord_is_ij_pos);
    }

    #[test]
    fn read_ij_coord_parses_metric_floating_point_offsets() {
        let mut image = GerberFileImage {
            gerb_metric: true,
            ..GerberFileImage::default()
        };

        let result = image.read_ij_coord(Some("I1.5J-2.5D01*"), 0);

        assert_eq!(result.pos, Vec2I::new(150_000, -250_000));
        assert_eq!(result.new_offset, 9);
        assert_eq!(image.ij_pos, Vec2I::new(150_000, -250_000));
        assert_eq!(image.last_arc_data_type, LastExtraArcDataType::Center);
        assert!(image.last_coord_is_ij_pos);
    }

    #[test]
    fn read_ij_coord_parses_imperial_integer_offsets() {
        let mut image = GerberFileImage {
            fmt_scale: Vec2I::new(4, 4),
            fmt_len: Vec2I::new(6, 6),
            ..GerberFileImage::default()
        };

        let result = image.read_ij_coord(Some("I150000J100000*"), 0);

        assert_eq!(result.pos, Vec2I::new(38_100_000, 25_400_000));
        assert_eq!(result.new_offset, 14);
    }

    #[test]
    fn read_ij_coord_parses_trailing_zero_suppression_without_excellon_extra_digit_rule() {
        let mut image = GerberFileImage {
            no_trailing_zeros: true,
            fmt_scale: Vec2I::new(4, 4),
            fmt_len: Vec2I::new(4, 4),
            ..GerberFileImage::default()
        };

        let result = image.read_ij_coord(Some("I123456*"), 0);

        assert_eq!(result.pos, Vec2I::new(31_357_824, 0));
        assert_eq!(result.new_offset, 7);
    }

    #[test]
    fn read_ij_coord_keeps_cpp_is_float_state_across_axes() {
        let mut image = GerberFileImage {
            gerb_metric: true,
            fmt_scale: Vec2I::new(4, 4),
            fmt_len: Vec2I::new(6, 6),
            ..GerberFileImage::default()
        };

        let result = image.read_ij_coord(Some("I1.5J2500*"), 0);

        assert_eq!(result.pos, Vec2I::new(150_000, 250_000_000));
    }

    #[test]
    fn read_ij_coord_empty_line_still_sets_center_arc_state() {
        let mut image = GerberFileImage {
            last_arc_data_type: LastExtraArcDataType::Radius,
            ..GerberFileImage::default()
        };

        let result = image.read_ij_coord(Some("D01*"), 0);

        assert_eq!(result.pos, Vec2I::new(0, 0));
        assert_eq!(result.new_offset, 0);
        assert_eq!(image.ij_pos, Vec2I::new(0, 0));
        assert_eq!(image.last_arc_data_type, LastExtraArcDataType::Center);
        assert!(image.last_coord_is_ij_pos);
    }

    #[test]
    fn read_ij_coord_empty_axis_uses_zero_value() {
        let mut image = GerberFileImage::default();

        let result = image.read_ij_coord(Some("ID01*"), 0);

        assert_eq!(result.pos, Vec2I::new(0, 0));
        assert_eq!(result.new_offset, 1);
        assert_eq!(image.ij_pos, Vec2I::new(0, 0));
        assert_eq!(image.last_arc_data_type, LastExtraArcDataType::Center);
    }

    #[test]
    fn read_int_parses_decimal_and_skips_one_separator() {
        assert_eq!(read_int("123,456", 0, true), (123, 4));
        assert_eq!(read_int("-42 X", 0, true), (-42, 4));
        assert_eq!(read_int("+17\tA", 0, true), (17, 4));
    }

    #[test]
    fn read_int_can_leave_separator_unconsumed() {
        assert_eq!(read_int("123,456", 0, false), (123, 3));
        assert_eq!(read_int("123 456", 0, false), (123, 3));
    }

    #[test]
    fn read_int_handles_kicad_zero_x_separator_special_case() {
        assert_eq!(read_int("0X123", 0, true), (0, 1));
        assert_eq!(read_int("0x123", 0, true), (0, 1));
    }

    #[test]
    fn read_int_returns_zero_without_advancing_when_no_digits_are_found() {
        assert_eq!(read_int("ABC", 0, true), (0, 0));
        assert_eq!(read_int("+ABC", 0, true), (0, 0));
    }

    #[test]
    fn read_double_parses_decimal_and_skips_one_separator() {
        assert_eq!(read_double("1.25,2", 0, true), (1.25, 5));
        assert_eq!(read_double("-3.5 X", 0, true), (-3.5, 5));
        assert_eq!(read_double("+4.0\tA", 0, true), (4.0, 5));
    }

    #[test]
    fn read_double_can_leave_separator_unconsumed() {
        assert_eq!(read_double("1.25,2", 0, false), (1.25, 4));
        assert_eq!(read_double("1.25 2", 0, false), (1.25, 4));
    }

    #[test]
    fn read_double_handles_kicad_zero_x_separator_special_case() {
        assert_eq!(read_double("0X1.25", 0, true), (0.0, 1));
        assert_eq!(read_double("0x1.25", 0, true), (0.0, 1));
    }

    #[test]
    fn read_double_treats_first_comma_as_operand_separator() {
        assert_eq!(read_double("1,25", 0, true), (1.0, 2));
    }

    #[test]
    fn read_double_stops_before_operator_sign_after_number() {
        assert_eq!(read_double("1.5-2.0", 0, true), (1.5, 3));
        assert_eq!(read_double("1.5+2.0", 0, true), (1.5, 3));
    }
}
