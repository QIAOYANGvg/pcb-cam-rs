/// Core type definitions for Gerber parsing.
/// Ported from KiCad gerbview.h

/// Interpolation type (G01/G02/G03)
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Interpolation {
    #[default]
    Linear1x = 0,
    ArcNeg,
    ArcPos,
}

/// G-code command types
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GCommand {
    #[default]
    Move = 0,
    LinearInterpol1x = 1,
    CircleNegInterpol = 2,
    CirclePosInterpol = 3,
    Comment = 4,
    TurnOnPolyFill = 36,
    TurnOffPolyFill = 37,
    SelectTool = 54,
    PhotoMode = 55,
    SpecifyInches = 70,
    SpecifyMillimeters = 71,
    TurnOff360Interpol = 74,
    TurnOn360Interpol = 75,
    SpecifyAbsoluteCoord = 90,
    SpecifyRelativeCoord = 91,
}

impl GCommand {
    pub fn from_i32(val: i32) -> Option<Self> {
        match val {
            0 => Some(Self::Move),
            1 => Some(Self::LinearInterpol1x),
            2 => Some(Self::CircleNegInterpol),
            3 => Some(Self::CirclePosInterpol),
            4 => Some(Self::Comment),
            36 => Some(Self::TurnOnPolyFill),
            37 => Some(Self::TurnOffPolyFill),
            54 => Some(Self::SelectTool),
            55 => Some(Self::PhotoMode),
            70 => Some(Self::SpecifyInches),
            71 => Some(Self::SpecifyMillimeters),
            74 => Some(Self::TurnOff360Interpol),
            75 => Some(Self::TurnOn360Interpol),
            90 => Some(Self::SpecifyAbsoluteCoord),
            91 => Some(Self::SpecifyRelativeCoord),
            _ => None,
        }
    }
}

/// Command analysis state
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CommandState {
    #[default]
    Idle = 0,
    EndBlock,
    EnterRs274xCmd,
}

/// Aperture types from ADD command
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ApertureType {
    #[default]
    Circle = b'C' as isize,
    Rect = b'R' as isize,
    Oval = b'0' as isize,
    Polygon = b'P' as isize,
    Macro = b'M' as isize,
}

impl ApertureType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            b'C' => Some(Self::Circle),
            b'R' => Some(Self::Rect),
            b'0' => Some(Self::Oval),
            b'P' => Some(Self::Polygon),
            b'M' => Some(Self::Macro),
            _ => None,
        }
    }
}

/// Aperture hole types
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ApertureHoleType {
    #[default]
    NoHole = 0,
    RoundHole,
    RectHole,
}

/// Shape types for draw items
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShapeType {
    #[default]
    Segment = 0,
    Arc,
    Circle,
    Polygon,
    SpotCircle,
    SpotRect,
    SpotOval,
    SpotPoly,
    SpotMacro,
}

/// Minimum valid D-code number
pub const FIRST_DCODE: i32 = 10;

/// Maximum valid D-code number
pub const LAST_DCODE: i32 = 0x7FFF_FFFF;
