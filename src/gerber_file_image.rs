use std::collections::BTreeMap;

use crate::aperture_macro::ApertureMacroSet;
use crate::dcode::DCode;
use crate::gerber_draw_item::DrawItem;
use crate::geometry::Vec2I;
use crate::gerber_layer::GerberLayer;
use crate::netlist_metadata::NetlistMetadata;
use crate::types::{CommandState, Interpolation};
use crate::x2_gerber_attributes::X2AttributeFileFunction;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LastExtraArcDataType {
    #[default]
    None,
    Center,
    Radius,
}

/// Hold the image data and parameters for one gerber file and layer parameters.
/// Ported from KiCad GERBER_FILE_IMAGE (gerber_file_image.h / gerber_file_image.cpp).
///
/// In Gerber world:
/// - An "image" is the entire gerber file and its global parameters
/// - A "layer" is a sub-set of a file that has specific parameters
#[derive(Clone, Debug)]
pub struct GerberFileImage {
    // === Coordinate format state ===
    pub current_pos: Vec2I,
    pub ij_pos: Vec2I,
    pub gerb_metric: bool,
    pub relative: bool,
    pub no_trailing_zeros: bool,
    pub fmt_scale: Vec2I,
    pub fmt_len: Vec2I,
    pub arc_radius: i32,
    pub last_arc_data_type: LastExtraArcDataType,
    pub last_coord_is_ij_pos: bool,

    // === File-level parameters ===
    /// true if this image is currently in use (a file is loaded in it)
    pub in_use: bool,
    /// Full file name for this layer
    pub file_name: String,
    /// Image name, from IN <name>* command
    pub image_name: String,
    /// Graphic layer number
    pub graphic_layer: i32,
    /// true = Negative image
    pub image_negative: bool,

    // === Coordinate format (expanded) ===
    /// Coord Offset, from IO command
    pub image_offset: Vec2I,
    /// Coord Offset, from OF command
    pub offset: Vec2I,
    /// Scale (X and Y) of layer
    pub scale: (f64, f64),
    /// Image rotation (0, 90, 180, 270 only) in degrees
    pub image_rotation: i32,
    /// Local rotation added to image_rotation (stored in 0.1 degrees)
    pub local_rotation: f64,
    /// false if A = X and B = Y (default); true if A = Y, B = X
    pub swap_axis: bool,
    /// true: mirror axis A (X)
    pub mirror_a: bool,
    /// true: mirror axis B (Y)
    pub mirror_b: bool,

    // === Image justify (%IJ) ===
    pub image_justify_x_center: bool,
    pub image_justify_y_center: bool,
    pub image_justify_offset: Vec2I,

    // === Parse state ===
    /// Linear, 90 arc, Circ.
    pub interpolation: Interpolation,
    /// Current Tool (Dcode) number selected
    pub current_tool: i32,
    /// Current or last pen state (0..9, set by Dn option with n < 10)
    pub last_pen_command: i32,
    /// State of gerber analysis command
    pub command_state: CommandState,
    /// Line number of the gerber file while reading
    pub line_num: i32,
    /// Previous specified coord for plot
    pub previous_pos: Vec2I,
    /// True if has DCodes in file
    pub has_dcode: bool,
    /// True if some DCodes in file are not defined
    pub has_missing_dcode: bool,
    /// Enable 360 deg circular interpolation
    pub arc_360_enabled: bool,
    /// Set to true when a circular interpolation command type is found
    pub as_arc_g74g75_cmd: bool,
    /// Enable polygon mode (read coord as a polygon descr)
    pub polygon_fill_mode: bool,
    /// In polygon mode: 0 = first segm, 1 = next segm
    pub polygon_fill_mode_state: i32,

    // === Data collections ===
    /// D-code (Aperture) list for this layer
    pub aperture_list: BTreeMap<i32, DCode>,
    /// Aperture macro collection, sorted by name
    pub aperture_macros: ApertureMacroSet,
    /// List of draw items parsed from the file
    pub drawings: Vec<DrawItem>,

    // === X2 attributes ===
    /// True if a X2 gerber attribute was found in file
    pub is_x2_file: bool,
    /// File function parameters from %TF command
    pub file_function: Option<X2AttributeFileFunction>,
    /// MD5 value from %TF.MD5 command
    pub md5_value: String,
    /// Part string from %TF.Part command
    pub part_string: String,
    /// Net attributes from %TO commands
    pub net_attribute_dict: NetlistMetadata,
    /// Aperture function from %TA.AperFunction
    pub aper_function: String,

    // === Exposure ===
    /// Whether an aperture macro tool is flashed on or off
    pub exposure: bool,

    // === Layer parameters ===
    pub layer_params: GerberLayer,

    // === Display (not affecting coordinates) ===
    pub display_offset: Vec2I,
    pub display_rotation: f64,

    // === Messages ===
    pub messages: Vec<String>,
}

impl Default for GerberFileImage {
    fn default() -> Self {
        Self {
            current_pos: Vec2I::new(0, 0),
            ij_pos: Vec2I::new(0, 0),
            gerb_metric: false,
            relative: false,
            no_trailing_zeros: false,
            fmt_scale: Vec2I::new(4, 4),
            fmt_len: Vec2I::new(7, 7),
            arc_radius: 0,
            last_arc_data_type: LastExtraArcDataType::None,
            last_coord_is_ij_pos: false,
            in_use: false,
            file_name: String::new(),
            image_name: String::new(),
            graphic_layer: 0,
            image_negative: false,
            image_offset: Vec2I::new(0, 0),
            offset: Vec2I::new(0, 0),
            scale: (1.0, 1.0),
            image_rotation: 0,
            local_rotation: 0.0,
            swap_axis: false,
            mirror_a: false,
            mirror_b: false,
            image_justify_x_center: false,
            image_justify_y_center: false,
            image_justify_offset: Vec2I::new(0, 0),
            interpolation: Interpolation::Linear1x,
            current_tool: 0,
            last_pen_command: 0,
            command_state: CommandState::Idle,
            line_num: 0,
            previous_pos: Vec2I::new(0, 0),
            has_dcode: false,
            has_missing_dcode: false,
            arc_360_enabled: false,
            as_arc_g74g75_cmd: false,
            polygon_fill_mode: false,
            polygon_fill_mode_state: 0,
            aperture_list: BTreeMap::new(),
            aperture_macros: ApertureMacroSet::new(),
            drawings: Vec::new(),
            is_x2_file: false,
            file_function: None,
            md5_value: String::new(),
            part_string: String::new(),
            net_attribute_dict: NetlistMetadata::default(),
            aper_function: String::new(),
            exposure: true,
            layer_params: GerberLayer::default(),
            display_offset: Vec2I::new(0, 0),
            display_rotation: 0.0,
            messages: Vec::new(),
        }
    }
}
