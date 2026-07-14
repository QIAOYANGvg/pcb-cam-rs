//! Main Gerber parser entry point.
//! Ported from KiCad gerbview/readgerb.cpp.

use std::fs;
use std::path::Path;

use crate::gerber_file_image::GerberFileImage;
use crate::rs274d::{code_number, execute_dcode_command, execute_g_command};
use crate::rs274x::read_rs274x_command;
use crate::types::CommandState;

/// Load a Gerber RS-274D/X file and return the parsed image.
///
/// Mirrors `GERBER_FILE_IMAGE::LoadGerberFile` from KiCad: reset parser state,
/// scan commands, and keep parser warnings in `GerberFileImage::messages`.
pub fn load_gerber_file(filename: &str) -> Result<GerberFileImage, Vec<String>> {
    let content = fs::read_to_string(filename)
        .map_err(|err| vec![format!("File '{}' not found: {}", filename, err)])?;

    let mut image = GerberFileImage {
        file_name: filename.to_string(),
        ..GerberFileImage::default()
    };

    parse_gerber_str(&mut image, &content);
    image.in_use = true;

    Ok(image)
}

/// Heuristic Gerber detector, ported from `GERBER_FILE_IMAGE::TestFileIsRS274`.
pub fn test_file_is_rs274(filename: &str) -> bool {
    let Ok(content) = fs::read_to_string(filename) else {
        return false;
    };

    test_str_is_rs274(&content)
}

pub fn test_str_is_rs274(content: &str) -> bool {
    let mut found_add = false;
    let mut found_d0 = false;
    let mut found_d2 = false;
    let mut found_m0 = false;
    let mut found_m2 = false;
    let mut found_star = false;
    let mut found_x = false;
    let mut found_y = false;

    for raw_line in content.lines() {
        let line = str_purge(raw_line);

        if line.is_empty() {
            continue;
        }

        if !line.is_ascii() {
            return false;
        }

        if line.contains("%ADD") {
            found_add = true;
        }

        if line.contains("D00") || line.contains("D0") {
            found_d0 = true;
        }

        if line.contains("D02") || line.contains("D2") {
            found_d2 = true;
        }

        if line.contains("M00") || line.contains("M0") {
            found_m0 = true;
        }

        if line.contains("M02") || line.contains("M2") {
            found_m2 = true;
        }

        if line.contains('*') {
            found_star = true;
        }

        if has_axis_number(line, b'X') {
            found_x = true;
        }

        if has_axis_number(line, b'Y') {
            found_y = true;
        }
    }

    (found_d0 || found_d2 || found_m0 || found_m2)
        && found_star
        && (found_x || found_y)
        && (found_add || !found_add)
}

pub fn parse_gerber_str(image: &mut GerberFileImage, content: &str) {
    let bytes = content.as_bytes();
    let mut pos = 0;
    image.line_num = 1;

    while pos < bytes.len() {
        match bytes[pos] {
            b' ' | b'\r' => pos += 1,
            b'\n' => {
                image.line_num += 1;
                pos += 1;
            }

            b'*' => {
                image.command_state = CommandState::EndBlock;
                pos += 1;
            }

            b'M' => {
                image.command_state = CommandState::Idle;
                break;
            }

            b'G' => {
                let (g_command, new_pos) = code_number(content, pos);
                pos = new_pos;
                let result = execute_g_command(image, content, pos, g_command);
                pos = result.new_offset;
            }

            b'D' => {
                let (d_command, new_pos) = code_number(content, pos);
                pos = new_pos;
                execute_dcode_command(image, d_command);
            }

            b'X' | b'Y' => {
                let result = image.read_xy_coord(Some(content), pos, false);
                pos = result.new_offset;

                if byte_at(content, pos) == Some(b'*') {
                    execute_dcode_command(image, image.last_pen_command);
                }
            }

            b'I' | b'J' => {
                let result = image.read_ij_coord(Some(content), pos);
                pos = result.new_offset;

                if byte_at(content, pos) == Some(b'*') {
                    execute_dcode_command(image, image.last_pen_command);
                }
            }

            b'%' => {
                if image.command_state != CommandState::EnterRs274xCmd {
                    image.command_state = CommandState::EnterRs274xCmd;
                    let result = read_rs274x_command(image, content, pos);
                    pos = result.new_offset;
                    image.line_num +=
                        count_newlines(&content[pos.min(content.len())..pos.min(content.len())]);
                } else {
                    image.messages.push("Expected RS274X Command".to_string());
                    image.command_state = CommandState::Idle;
                    pos += 1;
                }
            }

            unexpected => {
                image.messages.push(format!(
                    "Unexpected char 0x{:02X} ({})",
                    unexpected, unexpected as char
                ));
                pos += 1;
            }
        }
    }
}

fn str_purge(text: &str) -> &str {
    text.trim_matches(|ch: char| ch.is_ascii_whitespace())
}

fn has_axis_number(line: &str, axis: u8) -> bool {
    let bytes = line.as_bytes();

    for window in bytes.windows(2) {
        if window[0] == axis && window[1].is_ascii_digit() {
            return true;
        }
    }

    false
}

fn byte_at(text: &str, pos: usize) -> Option<u8> {
    text.as_bytes().get(pos).copied()
}

fn count_newlines(text: &str) -> i32 {
    text.as_bytes().iter().filter(|&&b| b == b'\n').count() as i32
}

#[allow(dead_code)]
fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dcode::DCode;
    use crate::types::ShapeType;

    #[test]
    fn test_str_is_rs274_detects_rs274x() {
        let content = "%FSLAX24Y24*%\n%ADD10C,0.1*%\nD10*\nX10Y10D02*\nX20Y20D01*\nM02*\n";

        assert!(test_str_is_rs274(content));
    }

    #[test]
    fn test_str_is_rs274_rejects_non_ascii() {
        assert!(!test_str_is_rs274("%ADD10C,0.1*%\nµ\n"));
    }

    #[test]
    fn parse_gerber_str_reads_format_units_aperture_and_draws_line() {
        let mut image = GerberFileImage::default();
        let content = "%FSLAX24Y24*MOMM*%\n%ADD10C,0.10*%\nD10*\nX000000Y000000D02*\nX010000Y000000D01*\nM02*\n";

        parse_gerber_str(&mut image, content);

        assert!(image.gerb_metric);
        assert!(image.aperture_list.contains_key(&10));
        assert_eq!(image.drawings.len(), 1);
        assert_eq!(image.drawings[0].shape_type, ShapeType::Segment);
        assert_eq!(image.drawings[0].dcode, 10);
    }

    #[test]
    fn parse_gerber_str_uses_last_pen_command_when_xy_block_has_no_dcode() {
        let mut image = GerberFileImage::default();
        let mut dcode = DCode::new(10);
        dcode.defined = true;
        image.aperture_list.insert(10, dcode);

        parse_gerber_str(&mut image, "D10*D02*X0Y0*D01*X1Y1*M02*");

        // Matches KiCad's main loop: `D01*` executes immediately using the
        // current position, and the following coordinate-only block reuses D01.
        assert_eq!(image.drawings.len(), 2);
    }

    #[test]
    fn parse_gerber_str_reports_embedded_tab_like_kicad_main_loop() {
        let mut image = GerberFileImage::default();

        parse_gerber_str(&mut image, "X0\tY0*\nM02*");

        assert!(
            image
                .messages
                .iter()
                .any(|message| message == "Unexpected char 0x09 (\t)")
        );
    }
}
