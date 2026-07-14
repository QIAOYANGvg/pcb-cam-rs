/// Gerber layer parameters.
/// Ported from KiCad GERBER_LAYER (gerber_file_image.h)

/// Layer-specific parameters that can change within a single Gerber file.
/// Each GERBER_FILE_IMAGE must include one GERBER_LAYER to define all parameters to plot a file.
#[derive(Clone, Debug)]
pub struct GerberLayer {
    /// Layer name, from LN <name>* command
    pub layer_name: String,

    /// true = Negative Layer: command LP
    pub layer_negative: bool,

    /// X and Y offsets for Step and Repeat command
    pub step_for_repeat: (f64, f64),

    /// The repeat count on X axis
    pub x_repeat_count: i32,

    /// The repeat count on Y axis
    pub y_repeat_count: i32,

    /// false = Inches, true = metric
    /// needed here because repeated gerber items can have coordinates in different units
    /// than step parameters and the actual coordinates calculation must handle this
    pub step_for_repeat_metric: bool,
}

impl Default for GerberLayer {
    fn default() -> Self {
        Self {
            layer_name: String::new(),
            layer_negative: false,
            step_for_repeat: (0.0, 0.0),
            x_repeat_count: 1,
            y_repeat_count: 1,
            step_for_repeat_metric: false,
        }
    }
}

impl GerberLayer {
    pub fn reset_default_values(&mut self) {
        self.layer_name.clear();
        self.layer_negative = false;
        self.step_for_repeat = (0.0, 0.0);
        self.x_repeat_count = 1;
        self.y_repeat_count = 1;
        self.step_for_repeat_metric = false;
    }
}
