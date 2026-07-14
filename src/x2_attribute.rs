/// X2 Gerber extension attributes.
/// Ported from KiCad X2_gerber_attributes.h / X2_gerber_attributes.cpp
///
/// Gerber X2 attributes look like:
/// %TF.FileFunction,Copper,L1,Top*%
///
/// Supports:
/// .FileFunction - identifies the file's function in the PCB
/// .Part - identifies the part the file represents
/// .MD5 - sets the MD5 file signature or checksum

/// X2 attribute parsed from %TF, %TO, %TA, %TD commands.
///
/// The attribute value consists of a number of substrings separated by commas.
#[derive(Clone, Debug, Default)]
pub struct X2Attribute {
    /// The list of parameters (after TF/TO/TA/TD).
    /// The first one is the attribute name if starting with '.'
    pub prms: Vec<String>,
}

impl X2Attribute {
    pub fn new() -> Self {
        Self { prms: Vec::new() }
    }

    /// Get the parameters list
    pub fn get_prms(&self) -> &Vec<String> {
        &self.prms
    }

    /// Get a parameter by index.
    /// idx = 0 is the parameter read after the TF function (same as get_attribute())
    pub fn get_prm(&self, idx: usize) -> &str {
        if idx < self.prms.len() {
            &self.prms[idx]
        } else {
            ""
        }
    }

    /// Get the attribute name (e.g., ".FileFunction")
    pub fn get_attribute(&self) -> &str {
        if !self.prms.is_empty() {
            &self.prms[0]
        } else {
            ""
        }
    }

    /// Get the number of parameters
    pub fn get_prm_count(&self) -> usize {
        self.prms.len()
    }

    /// Parse an attribute command terminated with * and fill prms with the parameters found.
    ///
    /// text points to the first char after the 2-letter command code (e.g., after "TF").
    /// On return, text points to the '*' terminator.
    pub fn parse_attrib_cmd(text: &mut &str) -> Self {
        let mut attr = Self::new();
        let bytes = text.as_bytes();
        let mut idx = 0;
        let mut field_start = 0;

        while idx < bytes.len() {
            let ch = bytes[idx];

            if ch == b'*' || ch == b'%' {
                // End of command - capture last field
                if idx > field_start {
                    attr.prms.push(text[field_start..idx].to_string());
                }
                break;
            }

            if ch == b',' {
                // Field separator
                attr.prms.push(text[field_start..idx].to_string());
                idx += 1;
                field_start = idx;
                continue;
            }

            idx += 1;
        }

        *text = &text[idx..];
        attr
    }

    /// Return true if the attribute is .FileFunction
    pub fn is_file_function(&self) -> bool {
        self.get_attribute().eq_ignore_ascii_case(".FileFunction")
    }

    /// Return true if the attribute is .MD5
    pub fn is_file_md5(&self) -> bool {
        self.get_attribute().eq_ignore_ascii_case(".MD5")
    }

    /// Return true if the attribute is .Part
    pub fn is_file_part(&self) -> bool {
        self.get_attribute().eq_ignore_ascii_case(".Part")
    }
}

/// File function information parsed from %TF.FileFunction.
///
/// Example: %TF.FileFunction,Copper,L1,Top*%
/// - Type: Copper, SolderMask, etc.
/// - Position: L1, L2, Top, Bot
#[derive(Clone, Debug, Default)]
pub struct X2AttributeFileFunction {
    /// The base attribute
    pub attribute: X2Attribute,

    /// Z-order of the layer for a board (front to back)
    pub z_order: i32,

    /// Z sub-order of the copper layer
    pub z_sub_order: i32,
}

impl X2AttributeFileFunction {
    pub fn new(attr: &X2Attribute) -> Self {
        let mut ff = Self {
            attribute: attr.clone(),
            z_order: 0,
            z_sub_order: 0,
        };
        ff.set_z_order();
        ff
    }

    /// Return true if the file function type is "Copper"
    pub fn is_copper(&self) -> bool {
        self.get_file_type().eq_ignore_ascii_case("Copper")
    }

    /// Return true if the file function type is "Plated" or "NotPlated" (drill file)
    pub fn is_drill_file(&self) -> bool {
        let ft = self.get_file_type();
        ft.eq_ignore_ascii_case("Plated") || ft.eq_ignore_ascii_case("NotPlated")
    }

    /// Get the type of layer (Copper, SolderMask, etc.)
    pub fn get_file_type(&self) -> &str {
        self.attribute.get_prm(1)
    }

    /// Get the board layer identifier: Ln (for Copper) or Top/Bot for other types
    pub fn get_brd_layer_id(&self) -> &str {
        self.attribute.get_prm(2)
    }

    /// Get the board layer side: Top, Bot, Inr
    pub fn get_brd_layer_side(&self) -> &str {
        if self.is_copper() {
            self.attribute.get_prm(3)
        } else {
            self.attribute.get_prm(2)
        }
    }

    /// Get the drill layer pair: n,m for drill files
    pub fn get_drill_layer_pair(&self) -> &str {
        if self.is_drill_file() {
            self.attribute.get_prm(2)
        } else {
            ""
        }
    }

    /// Get the Layer Pair type for drill files (PTH, NPTH, Blind, Buried)
    pub fn get_lp_type(&self) -> &str {
        if self.is_drill_file() {
            self.attribute.get_prm(3)
        } else {
            ""
        }
    }

    /// Get the drill/routing type (Drill, Route, Mixed)
    pub fn get_route_type(&self) -> &str {
        if self.is_drill_file() {
            self.attribute.get_prm(4)
        } else {
            ""
        }
    }

    /// Get the label, if any
    pub fn get_label(&self) -> &str {
        let last = self.attribute.get_prm_count();
        if last > 0 {
            self.attribute.get_prm(last - 1)
        } else {
            ""
        }
    }

    /// Get the z-order
    pub fn get_z_order(&self) -> i32 {
        self.z_order
    }

    /// Get the z sub-order
    pub fn get_z_sub_order(&self) -> i32 {
        self.z_sub_order
    }

    /// Initialize the z order priority from attributes
    fn set_z_order(&mut self) {
        let file_type = self.get_file_type().to_lowercase();

        // Assign z-order based on file type
        self.z_order = match file_type.as_str() {
            "copper" => 10,
            "soldermask" | "solder mask" | "solderresist" => 20,
            "silkscreen" | "legend" => 30,
            "paste" | "solderpaste" => 40,
            "assembly" | "drawing" => 50,
            "plated" | "notplated" => 60,
            "profile" | "outline" => 70,
            _ => 100,
        };

        // For copper layers, set sub-order based on layer number
        if self.is_copper() {
            let layer_id = self.get_brd_layer_id();
            if let Some(num_str) = layer_id.strip_prefix('L') {
                if let Ok(num) = num_str.parse::<i32>() {
                    self.z_sub_order = num;
                }
            } else {
                let side = self.get_brd_layer_side().to_lowercase();
                self.z_sub_order = match side.as_str() {
                    "top" => 1,
                    "bot" => 999,
                    "inr" | "inner" => 500,
                    _ => 0,
                };
            }
        }
    }
}
