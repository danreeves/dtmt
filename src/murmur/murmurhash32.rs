// Copyright (C) 2022 Lucas Schwiderski
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Adapted from https://github.com/badboy/murmurhash64-rs

// 'M' and 'R' are mixing constants generated offline.
// They're not really 'magic', they just happen to work well.
const M: u32 = 0x5bd1e995;
const R: u8 = 24;

pub fn hash(key: &[u8], seed: u32) -> u32 {
    let len = key.len();

    // Initialize hash to a "random" value
    let mut h: u32 = seed ^ ((len as u32).wrapping_mul(M));

    let mut i = 0;
    while i != len - (len & 3) {
        let mut k: u32 = key[i] as u32;
        k |= (key[i + 1] as u32) << 8;
        k |= (key[i + 2] as u32) << 16;
        k |= (key[i + 3] as u32) << 24;

        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);

        h ^= k;
        h = h.wrapping_mul(M);

        i += 4;
    }

    let overflow = len & 3;
    if overflow >= 3 {
        h ^= (key[i + 2] as u32) << 16;
    }
    if overflow >= 2 {
        h ^= (key[i + 1] as u32) << 8;
    }
    if overflow >= 1 {
        h ^= key[i] as u32;
    }
    if overflow > 0 {
        h = h.wrapping_mul(M);
    }

    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}
