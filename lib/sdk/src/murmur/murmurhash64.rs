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
const M: u64 = 0xc6a4a7935bd1e995;
// Multiplicative inverse of `M` under % 2^64
const M_INVERSE: u64 = 0x5f7a0ea7e59b19bd;
const R: u8 = 47;

pub fn hash(key: &[u8], seed: u64) -> u64 {
    let len = key.len();
    let mut h: u64 = seed ^ ((len as u64).wrapping_mul(M));

    let endpos = len - (len & 7);
    let mut i = 0;
    while i != endpos {
        let mut k: u64;

        k = key[i] as u64;
        k |= (key[i + 1] as u64) << 8;
        k |= (key[i + 2] as u64) << 16;
        k |= (key[i + 3] as u64) << 24;
        k |= (key[i + 4] as u64) << 32;
        k |= (key[i + 5] as u64) << 40;
        k |= (key[i + 6] as u64) << 48;
        k |= (key[i + 7] as u64) << 56;

        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);

        h ^= k;
        h = h.wrapping_mul(M);

        i += 8;
    }

    let overflow = len & 7;
    if overflow == 7 {
        h ^= (key[i + 6] as u64) << 48;
    }
    if overflow >= 6 {
        h ^= (key[i + 5] as u64) << 40;
    }
    if overflow >= 5 {
        h ^= (key[i + 4] as u64) << 32;
    }
    if overflow >= 4 {
        h ^= (key[i + 3] as u64) << 24;
    }
    if overflow >= 3 {
        h ^= (key[i + 2] as u64) << 16;
    }
    if overflow >= 2 {
        h ^= (key[i + 1] as u64) << 8;
    }
    if overflow >= 1 {
        h ^= key[i] as u64;
    }
    if overflow > 0 {
        h = h.wrapping_mul(M);
    }

    h ^= h >> R;
    h = h.wrapping_mul(M);
    h ^= h >> R;
    h
}

pub fn hash_inverse(hash: u64, seed: u64) -> u64 {
    let mut h = hash;
    h ^= h >> R;
    h = h.wrapping_mul(M_INVERSE);
    h ^= h >> R;
    h = h.wrapping_mul(M_INVERSE);

    let h_forward: u64 = seed ^ (M.wrapping_mul(8));
    let mut k: u64 = h ^ h_forward;

    k = k.wrapping_mul(M_INVERSE);
    k ^= k >> R;
    k = k.wrapping_mul(M_INVERSE);

    // let mut k = k.to_ne_bytes();
    // k.reverse();
    // u64::from_ne_bytes(k)
    k
}

pub fn hash32(key: &[u8], seed: u32) -> u32 {
    let h = hash(key, seed as u64);
    (h >> 32) as u32
}

#[test]
fn test_hash() {
    assert_eq!(0, hash("".as_bytes(), 0));
    assert_eq!(0xc26e8bc196329b0f, hash("".as_bytes(), 10));
    assert_eq!(0xa14e8dfa2cd117e2, hash("lua".as_bytes(), 0));
    assert_eq!(
        0x069A33456AAD3042,
        hash("twitch_intervention".as_bytes(), 0)
    );
}

#[test]
fn test_inverse() {
    let h = hash("lua".as_bytes(), crate::murmur::SEED as u64);
    let inv = hash_inverse(h, crate::murmur::SEED as u64);
    assert_eq!(h, hash(&inv.to_le_bytes(), crate::murmur::SEED as u64));
    assert_ne!(h, hash(&inv.to_be_bytes(), crate::murmur::SEED as u64));
}
