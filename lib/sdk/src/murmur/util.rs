use color_eyre::eyre::bail;
use color_eyre::Result;

// Generates tables similar to these:
// https://github.com/zbjornson/fast-hex/blob/a3487bca95127634a61bfeae8f8bfc8f0e5baa3f/src/hex.cc#L20-L89
// `upper` determines upper vs. lower bits (first character is `upper`).
const fn generate_byte_map(upper: bool) -> [u8; 256] {
    let mut out = [0u8; 256];
    let factor = if upper { 16 } else { 1 };

    let mut i = 0;

    while i < 256 {
        match i {
            0x30..=0x39 => out[i] = factor * (i as u8 - 0x30),
            0x41..=0x46 => out[i] = factor * (9 + i as u8 - 0x40),
            0x61..=0x66 => out[i] = factor * (9 + i as u8 - 0x60),
            _ => out[i] = u8::MAX,
        }
        i += 1;
    }

    out
}

const BYTE_MAP_UPPER: [u8; 256] = generate_byte_map(true);
const BYTE_MAP_LOWER: [u8; 256] = generate_byte_map(false);

macro_rules! make_parse_hex {
    ($name:ident, $ty:ty, $len:expr) => {
        #[inline]
        pub fn $name(s: impl AsRef<str>) -> Result<$ty> {
            // For the string to be valid hex characters, it needs to be ASCII.
            // So we can simply treat it as a byte stream.
            let s = s.as_ref().as_bytes();

            if s.len() != $len {
                bail!(
                    "String length doesn't match. Expected {}, got {}",
                    $len,
                    s.len()
                );
            }

            let n = $len / 2;
            let mut out: $ty = 0;
            let mut i = 0;

            while i < n {
                let j = i * 2;

                let c1 = BYTE_MAP_UPPER[s[j] as usize];
                if c1 == u8::MAX {
                    bail!("Invalid character '{:?}' ({})", char::from(c1), c1);
                }

                let c2 = BYTE_MAP_LOWER[s[j + 1] as usize];
                if c2 == u8::MAX {
                    bail!("Invalid character '{:?}' ({})", char::from(c2), c2);
                }

                out |= ((c1 + c2) as $ty) << (n - i - 1) * 8;

                i += 1;
            }

            Ok(out)
        }
    };
}

make_parse_hex!(parse_hex64, u64, 16);
make_parse_hex!(parse_hex32, u32, 8);

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_32() {
        let hash = "A14E8DFA";
        assert_eq!(parse_hex32(hash).unwrap(), 0xA14E8DFA);
    }

    #[test]
    fn parse_64() {
        let hash = "A14E8DFA2CD117E2";
        assert_eq!(parse_hex64(hash).unwrap(), 0xA14E8DFA2CD117E2);
    }

    #[test]
    fn std_from_radix_32() {
        let hash = "A14E8DFA";
        assert_eq!(u32::from_str_radix(hash, 16).unwrap(), 0xA14E8DFA);
    }

    #[test]
    fn std_from_radix_64() {
        let hash = "A14E8DFA2CD117E2";
        assert_eq!(u64::from_str_radix(hash, 16).unwrap(), 0xA14E8DFA2CD117E2);
    }
}

#[cfg(test)]
mod bench {
    use super::{parse_hex32, parse_hex64};

    extern crate test;

    const HASH32: &str = "A14E8DFA";
    const HASH64: &str = "A14E8DFA2CD117E2";

    #[bench]
    fn custom_32(b: &mut test::Bencher) {
        b.iter(|| test::black_box(parse_hex32(test::black_box(HASH32))))
    }

    #[bench]
    fn std_32(b: &mut test::Bencher) {
        b.iter(|| test::black_box(u32::from_str_radix(test::black_box(HASH32), 16)))
    }

    #[bench]
    fn custom_64(b: &mut test::Bencher) {
        b.iter(|| test::black_box(parse_hex64(test::black_box(HASH64))))
    }

    #[bench]
    fn std_64(b: &mut test::Bencher) {
        b.iter(|| test::black_box(u64::from_str_radix(test::black_box(HASH64), 16)))
    }
}
