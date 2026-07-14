pub mod am_param;
pub mod am_primitive;
pub mod aperture_macro;
pub mod dcode;
pub mod evaluate;
pub mod export;
pub mod geometry;
pub mod gerber_draw_item;
pub mod gerber_file_image;
pub mod gerber_layer;
pub mod netlist_metadata;
pub mod readgerb;
pub mod rs274_read_xy_and_ij_coordinates;
pub mod rs274d;
pub mod rs274x;
pub mod types;
pub mod x2_gerber_attributes;

pub use export::golden as golden_export;
pub use gerber_draw_item as draw_item;
pub use readgerb as gerber_parser;
pub use x2_gerber_attributes as x2_attribute;

pub mod coord {
    pub use crate::geometry::Vec2I;
    pub use crate::gerber_file_image::{GerberFileImage, LastExtraArcDataType};
    pub use crate::rs274_read_xy_and_ij_coordinates::{
        read_double, read_int, scale_to_iu, ReadCoordResult, GERB_IU_PER_MM,
    };
}
