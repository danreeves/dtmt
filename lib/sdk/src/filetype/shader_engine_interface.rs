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


/// The engine cbuffer whose compiled `size` this is: whether it carries the
/// material's channels (and so has a non-zero prefix), and its fields - each
/// `(kind, name hash, offset, size)` relative to that prefix. `None` when the
/// size is not one of the engine's.
pub fn engine_cbuffer(size: u32) -> Option<(bool, &'static [(u32, u32, u32, u32)])> {
    ENGINE_CBUFFERS
        .iter()
        .find(|entry| entry.0 == size)
        .map(|entry| (entry.1 > 0, entry.2))
}

/// A group's `mid`: the sections that carry its cbuffers' fields, interleaved
/// with the headers, ending with the packed copies.
///
/// `cbuffers` are the group's cbuffers' compiled sizes in the head's order
/// (minus `global_viewport`); `channels` the material's channel records - the
/// 20-byte rows the engine's channel table gives, at their absolute offsets in
/// the cbuffer that carries them; `copies` the packed-copy records; `x_end` the
/// head's per-draw total. The header before section `j` is `{the j-th cbuffer's
/// size, W2, the section's record count}`, and the fields belong to the *next*
/// cbuffer - so the first cbuffer's fields are never emitted, and the last
/// section is the copies. The channel-bearing cbuffer's fields sit at
/// `align16(the channels' extent) + the engine's relative offset`.
pub fn derived_mid(cbuffers: &[(u32, u32)], channels: &[u8], copies: &[u8], x_end: u32) -> Vec<u8> {
    // The material's channels occupy a 16-aligned prefix of their cbuffer.
    let extent = channels
        .chunks_exact(20)
        .map(|row| {
            let offset = u32::from_le_bytes(row[12..16].try_into().unwrap());
            let size = u32::from_le_bytes(row[16..20].try_into().unwrap());
            offset + size
        })
        .max()
        .unwrap_or(0);
    let prefix = extent.div_ceil(16) * 16;

    let records = |size: u32| -> Vec<u8> {
        let mut out = Vec::new();
        let Some((has_channels, fields)) = engine_cbuffer(size) else {
            return out;
        };
        if has_channels {
            out.extend_from_slice(channels);
        }
        // The channels' prefix applies only to the cbuffer that carries them.
        let base = if has_channels { prefix } else { 0 };
        for (kind, hash, offset, len) in fields {
            for word in [*kind, 0, *hash, base + offset, *len] {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        out
    };

    let n = cbuffers.len();
    // The j-th section is the (j+1)-th cbuffer's fields; the last is the copies,
    // whose records are 28 bytes rather than 20.
    let sections: Vec<Vec<u8>> = (0..n)
        .map(|j| {
            if j + 1 < n {
                records(cbuffers[j + 1].0)
            } else {
                copies.to_vec()
            }
        })
        .collect();

    let mut out = Vec::new();
    // The running total advances by each cbuffer's size, but a global cbuffer
    // contributes nothing *and* prints a zero `W2`.
    let mut accumulator = x_end;
    for (j, section) in sections.iter().enumerate() {
        let step = if j + 1 < n { 20 } else { 28 };
        let count = (section.len() / step) as u32;
        let w2 = if is_global(cbuffers[j].0) {
            0
        } else {
            accumulator
        };
        for word in [cbuffers[j].0, w2, count] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(section);
        if !is_global(cbuffers[j].0) {
            accumulator += cbuffers[j].0;
        }
    }
    out
}

/// Whether a cbuffer is one of the engine's globals, which carry no packed
/// copies and so have a zero `W2` - measured on the standard, whose
/// `global_environment_settings` (400 bytes) is the only one in group 0.
fn is_global(size: u32) -> bool {
    size == 400
}

/// The engine's channel shapes, by name hash: each channel's `(kind, offset,
/// size)` rows within its own extent. `bca` keeps a leading kind-5 row; the
/// two-float2 shape is the common one, so [`channel_rows`] falls back to it for a
/// channel the table does not name.
pub const CHANNELS: &[(u32, &[(u32, u32, u32)])] = &[
    (0x2FAD_CC8C, &[(5, 0, 4), (1, 4, 8), (1, 16, 8)]), // bca
];

/// A channel's rows: its entry in [`CHANNELS`], or the common two-float2 shape.
pub fn channel_rows(hash: u32) -> &'static [(u32, u32, u32)] {
    CHANNELS
        .iter()
        .find(|entry| entry.0 == hash)
        .map_or(&[(1, 0, 8), (1, 8, 8)], |entry| entry.1)
}

/// The channel records a `mid` carries for `channels` (their hashes, in
/// declaration order): each channel's rows, packed one channel after another.
pub fn channel_records(channels: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut base = 0u32;
    for channel in channels {
        let rows = channel_rows(*channel);
        for (kind, offset, size) in rows {
            for word in [*kind, 0, *channel, base + offset, *size] {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        base += rows
            .iter()
            .map(|(_, offset, size)| offset + size)
            .max()
            .unwrap_or(0);
    }
    out
}

/// The packed-copy rows for a group: every channel's rows re-emitted once per
/// cbuffer that carries the channels, with `b` the copy set index and `x` the
/// cbuffer's packed index. Each row is `{channel, a, b, offset, kind, cbuffer,
/// x}` where `a` is the kind for a kind-5 row and the component's byte offset
/// (0, 8, ...) for the float2 rows. `cbuffers` are `(compiled size, name hash)`.
pub fn packed_copies(cbuffers: &[(u32, u32)], channels: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut set = 0u32;
    for (size, name) in cbuffers {
        // Only a cbuffer that carries the channels gets copies.
        let Some((true, _)) = engine_cbuffer(*size) else {
            continue;
        };
        let Some((hash, x)) = PACKED
            .iter()
            .find(|(packed, cbuffer, _)| packed == size && cbuffer == name)
            .map(|(_, hash, x)| (hash, x))
        else {
            continue;
        };
        let mut base = 0u32;
        for channel in channels {
            let rows = channel_rows(*channel);
            let mut component = 0u32;
            for (kind, offset, _) in rows {
                let a = if *kind == 5 {
                    5
                } else {
                    let a = component * 8;
                    component += 1;
                    a
                };
                for word in [*channel, a, set, base + offset, *kind, *hash, *x] {
                    out.extend_from_slice(&word.to_le_bytes());
                }
            }
            base += rows.iter().map(|(_, offset, size)| offset + size).max().unwrap_or(0);
        }
        set += 1;
    }
    out
}

/// The cbuffers that carry copies: `(compiled size, name hash, packed index)` -
/// the last word of every copy row. Measured on the standard.
pub const PACKED: &[(u32, u32, u32)] = &[
    (416, 0xB563_9618, 2), // c_per_object
    (304, 0xCFB4_40F0, 4), // c_depth_only
    (272, 0xB563_9618, 1), // c_per_object, the smaller variant
    (272, 0xCFB4_40F0, 1), // c_depth_only, the smaller variant
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The table is indexed by the compiled cbuffer size, and its offsets are
    /// relative to the channels' prefix.
    #[test]
    fn the_engine_cbuffers_are_indexed_by_size() {
        let (has_channels, fields) = engine_cbuffer(416).expect("c_per_object");
        assert!(has_channels);
        assert_eq!(fields.len(), 7);
        assert_eq!(fields.first(), Some(&(4, 0x57CB_AEC2, 0, 64)));
        assert_eq!(engine_cbuffer(400).map(|(channels, _)| channels), Some(false));
        assert!(engine_cbuffer(4096).is_none());
    }

    /// `derived_mid`'s first section, against the standard's group 0: the header
    /// is `{16, x_end, 14}` and the engine rows follow the seven channel rows.
    #[test]
    fn the_first_mid_section_matches_the_standard() {
        let row = |kind: u32, hash: u32, offset: u32, size: u32| {
            let mut out = Vec::new();
            for word in [kind, 0, hash, offset, size] {
                out.extend_from_slice(&word.to_le_bytes());
            }
            out
        };
        // bca keeps a kind-5 row, orm and nm only their two float2 rows.
        let mut channels = Vec::new();
        for (kind, hash, offset, size) in [
            (5u32, 0x2FAD_CC8C, 0u32, 4u32),
            (1, 0x2FAD_CC8C, 4, 8),
            (1, 0x2FAD_CC8C, 16, 8),
            (1, 0x8655_079C, 24, 8),
            (1, 0x8655_079C, 32, 8),
            (1, 0xB45E_95F3, 40, 8),
            (1, 0xB45E_95F3, 48, 8),
        ] {
            channels.extend_from_slice(&row(kind, hash, offset, size));
        }

        let mid = derived_mid(&[(16, 0), (416, 0xB563_9618)], &channels, &[], 176);
        assert_eq!(u32::from_le_bytes(mid[0..4].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(mid[4..8].try_into().unwrap()), 176);
        assert_eq!(u32::from_le_bytes(mid[8..12].try_into().unwrap()), 14);
        // The first engine row: `view_proj` at the channels' 16-aligned extent
        // (56 -> 64).
        let at = 12 + 7 * 20;
        let engine: [u32; 5] = std::array::from_fn(|w| {
            u32::from_le_bytes(mid[at + w * 4..at + w * 4 + 4].try_into().unwrap())
        });
        assert_eq!(engine, [4, 0, 0x57CB_AEC2, 64, 64]);
    }

    /// The material's channel records pack one channel after another: `bca` 24
    /// bytes, then `orm` at 24 and `nm` at 40.
    #[test]
    fn the_channel_records_pack_sequentially() {
        let records = channel_records(&[0x2FAD_CC8C, 0x8655_079C, 0xB45E_95F3]);
        assert_eq!(records.len(), 7 * 20);
        let row = |i: usize| -> [u32; 5] {
            std::array::from_fn(|w| {
                u32::from_le_bytes(records[i * 20 + w * 4..i * 20 + w * 4 + 4].try_into().unwrap())
            })
        };
        assert_eq!(row(0), [5, 0, 0x2FAD_CC8C, 0, 4]);
        assert_eq!(row(3), [1, 0, 0x8655_079C, 24, 8]);
        assert_eq!(row(6), [1, 0, 0xB45E_95F3, 48, 8]);
    }

    /// The packed copies: the channel rows once per channel-bearing cbuffer.
    #[test]
    fn the_copies_repeat_the_channels_per_cbuffer() {
        let copies = packed_copies(
            &[
                (16, 0),
                (416, 0xB563_9618),
                (400, 0),
                (304, 0xCFB4_40F0),
            ],
            &[0x2FAD_CC8C, 0x8655_079C, 0xB45E_95F3],
        );
        assert_eq!(copies.len(), 14 * 28);
        let row = |i: usize| -> [u32; 7] {
            std::array::from_fn(|w| {
                u32::from_le_bytes(copies[i * 28 + w * 4..i * 28 + w * 4 + 4].try_into().unwrap())
            })
        };
        assert_eq!(row(0), [0x2FAD_CC8C, 5, 0, 0, 5, 0xB563_9618, 2]);
        assert_eq!(row(6), [0xB45E_95F3, 8, 0, 48, 1, 0xB563_9618, 2]);
        assert_eq!(row(7), [0x2FAD_CC8C, 5, 1, 0, 5, 0xCFB4_40F0, 4]);
    }
}
