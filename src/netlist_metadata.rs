/// Netlist metadata from X2 attributes.
/// Ported from KiCad gbr_netlist_metadata.h

/// Net attribute type flags
pub const GBR_NETINFO_NET: u32 = 1;
pub const GBR_NETINFO_CMP: u32 = 2;
pub const GBR_NETINFO_PAD: u32 = 4;

/// Net attribute metadata from %TO commands
#[derive(Clone, Debug, Default)]
pub struct NetlistMetadata {
    /// Bitfield of GBR_NETINFO_* flags
    pub net_attrib_type: u32,

    /// Net name from %TO.N
    pub netname: String,

    /// Component reference from %TO.C or %TO.P
    pub cmpref: String,

    /// Pad name from %TO.P
    pub padname: String,

    /// Pad/pin function from %TO.P
    pub pad_pin_function: String,
}

impl NetlistMetadata {
    pub fn clear(&mut self) {
        self.net_attrib_type = 0;
        self.netname.clear();
        self.cmpref.clear();
        self.padname.clear();
        self.pad_pin_function.clear();
    }
}
