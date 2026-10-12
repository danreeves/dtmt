//! The engine's constant-buffer fields, mined from a shipped `shader43` section.
//!
//! A group's `mid` carries, after the material's channels, the engine's own
//! cbuffer fields. The engine's layout is fixed but its cbuffers differ per
//! group, so this table is keyed by a cbuffer's compiled **size** (the
//! reflection gives it) and stores each field's offset **relative to the
//! material's channels' padded prefix** - the "offsets by HLSL packing" rule.
//! Each entry is `(cbuffer size, prefix, fields)`, a field being
//! `(kind, name hash, offset, size)`.

/// The engine's cbuffer fields, by cbuffer size. See the module docs.
pub const ENGINE_CBUFFERS: &[(u32, u32, &[(u32, u32, u32, u32)])] = &[
    (16, 0, &[
        (5, 0xB6E42505, 0, 4),
    ]),
    (272, 32, &[
        (4, 0x57CBAEC2, 0, 64),
        (4, 0xA17AE7B4, 64, 64),
        (4, 0x4D46AE3B, 128, 64),
        (3, 0x795CF4A7, 192, 16),
        (3, 0x999C5B7A, 208, 16),
        (0, 0x429C277D, 224, 4),
    ]),
    (304, 64, &[
        (4, 0x57CBAEC2, 0, 64),
        (4, 0xA17AE7B4, 64, 64),
        (4, 0x4D46AE3B, 128, 64),
        (3, 0x795CF4A7, 192, 16),
        (3, 0x999C5B7A, 208, 16),
        (0, 0x429C277D, 224, 4),
    ]),
    (400, 0, &[
        (0, 0xAFD0A93D, 0, 4),
        (0, 0x5FD50519, 4, 4),
        (0, 0xEB37EDF3, 8, 4),
        (0, 0x02F6DC5B, 12, 4),
        (0, 0x8D53D4E6, 16, 4),
        (0, 0xD3E57C64, 20, 4),
        (0, 0x45546A88, 24, 4),
        (0, 0xBA249BA2, 28, 4),
        (0, 0xDA20446E, 32, 4),
        (0, 0x4F44E11F, 36, 4),
        (0, 0x06EE3E72, 40, 4),
        (2, 0xDFE45D62, 48, 12),
        (2, 0x5E990262, 64, 12),
        (0, 0x6D5FCFB1, 76, 4),
        (0, 0x65387547, 80, 4),
        (1, 0xD3BAE727, 84, 8),
        (0, 0xAB9526CB, 92, 4),
        (0, 0x004D7226, 96, 4),
        (2, 0x93B47324, 100, 12),
        (2, 0x40027A64, 112, 12),
        (0, 0xC303C9A2, 124, 4),
        (0, 0x377949F0, 128, 4),
        (2, 0x57BF5B84, 132, 12),
        (2, 0x397442D5, 144, 12),
        (2, 0x411D4A12, 160, 12),
        (0, 0x97C21E94, 172, 4),
        (0, 0x3EABDBD8, 176, 4),
        (0, 0x5B7818F5, 180, 4),
        (0, 0xBAF07DA4, 184, 4),
        (2, 0xD160E502, 192, 12),
        (2, 0xDEAE757B, 208, 12),
        (0, 0x64D48010, 220, 4),
        (0, 0x0F523FD6, 224, 4),
        (1, 0x9701F35E, 228, 8),
        (2, 0x186213B4, 240, 12),
        (0, 0xA4C1F2F8, 252, 4),
        (0, 0xE5880E06, 256, 4),
        (2, 0x67BE31EC, 260, 12),
        (2, 0xFF634AAB, 272, 12),
        (2, 0xBD60D8AA, 288, 12),
        (1, 0x9A5EB3C2, 304, 8),
        (0, 0xDFB34D33, 312, 4),
        (0, 0x9E45A569, 316, 4),
        (0, 0x699EDEBC, 320, 4),
        (0, 0xB3E28FD8, 324, 4),
        (0, 0x204D05BA, 328, 4),
        (0, 0x06983A67, 332, 4),
        (0, 0xC7B0D8EB, 336, 4),
        (0, 0x51695278, 340, 4),
        (0, 0x4484B38F, 344, 4),
        (0, 0x6E219052, 348, 4),
        (0, 0x15A04BC9, 352, 4),
        (0, 0x5894B54A, 356, 4),
        (0, 0x3E50E950, 360, 4),
        (0, 0xC7638567, 364, 4),
        (0, 0xB19D1B03, 368, 4),
        (0, 0x21EC5EF1, 372, 4),
        (3, 0xE3896B8A, 384, 16),
    ]),
    (416, 64, &[
        (4, 0x57CBAEC2, 0, 64),
        (4, 0xA17AE7B4, 64, 64),
        (4, 0x4D46AE3B, 128, 64),
        (4, 0xB8DADD64, 192, 64),
        (4, 0x711B4ADC, 256, 64),
        (5, 0xED9D8188, 320, 4),
        (3, 0xB9D685CB, 336, 16),
    ]),
];


/// The engine cbuffer whose compiled `size` this is: its prefix (the material's
/// channels' padded extent) and its fields, each `(kind, name hash, offset,
/// size)` relative to that prefix. `None` when the size is not one of the
/// engine's.
pub fn engine_cbuffer(size: u32) -> Option<(u32, &'static [(u32, u32, u32, u32)])> {
    ENGINE_CBUFFERS
        .iter()
        .find(|entry| entry.0 == size)
        .map(|entry| (entry.1, entry.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table is indexed by the compiled cbuffer size, and its offsets are
    /// relative to the channels' prefix.
    #[test]
    fn the_engine_cbuffers_are_indexed_by_size() {
        let (prefix, fields) = engine_cbuffer(416).expect("c_per_object");
        assert_eq!(prefix, 64);
        assert_eq!(fields.len(), 7);
        assert_eq!(fields.first(), Some(&(4, 0x57CB_AEC2, 0, 64)));
        assert!(engine_cbuffer(4096).is_none());
    }
}
