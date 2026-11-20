//! Stable product profile metadata shared by runtime and installation.
use windows_core::GUID;

/// Stable text-service class identifier.
pub const SERVICE_ID: GUID = GUID::from_u128(0x1a746212_84db_4acf_88b3_caaa26352774);

/// Native language profile metadata.
pub struct Profile {
    /// Native profile id.
    pub id: GUID,
    /// Native profile language.
    pub language: u16,
    /// Native profile layout.
    pub layout: &'static str,
    /// Native profile description.
    pub description: &'static str,
    /// Native profile mode.
    pub mode: u32,
}

/// Authoritative English and Russian profiles.
pub const PROFILES: [Profile; 2] = [
    Profile {
        id: GUID::from_u128(0x0d52a994_9eb5_44ab_b18f_603dc5f69671),
        language: 0x0409,
        layout: "00000409",
        description: "Keyrill English",
        mode: 1,
    },
    Profile {
        id: GUID::from_u128(0x4c1e583c_f8e5_4b7d_b66c_b7e7319aed51),
        language: 0x0419,
        layout: "00000419",
        description: "Keyrill Русский",
        mode: 2,
    },
];
