#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum ParserState {
    Start,
    Word,
    SingleQuote,
    DoubleQuote,
}

pub struct ShellParser<'a> {
    bytes: &'a [u8],
    offset: usize,
    pub errored: bool,
}

impl<'a> ShellParser<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
            errored: false,
        }
    }

    fn parse_word(&mut self) -> Option<&'a [u8]> {
        // The start of the current word. Certain leading characters should be ignored,
        // so this might change.
        let mut start = self.offset;
        let mut state = ParserState::Start;

        while self.offset < self.bytes.len() {
            let c = self.bytes[self.offset];
            self.offset += 1;

            match state {
                ParserState::Start => match c {
                    // Ignore leading whitespace
                    b' ' | b'\t' | b'\n' => start += 1,
                    b'\'' => {
                        state = ParserState::SingleQuote;
                        start += 1;
                    }
                    b'"' => {
                        state = ParserState::DoubleQuote;
                        start += 1;
                    }
                    _ => {
                        state = ParserState::Word;
                    }
                },
                ParserState::Word => match c {
                    // Unquoted whitespace ends the current word
                    b' ' | b'\t' | b'\n' => {
                        return Some(&self.bytes[start..self.offset - 1]);
                    }
                    _ => {}
                },
                ParserState::SingleQuote => match c {
                    b'\'' => {
                        return Some(&self.bytes[start..(self.offset - 1)]);
                    }
                    _ => {}
                },
                ParserState::DoubleQuote => match c {
                    b'"' => {
                        return Some(&self.bytes[start..(self.offset - 1)]);
                    }
                    _ => {}
                },
            }
        }

        match state {
            ParserState::Start => None,
            ParserState::Word => Some(&self.bytes[start..self.offset]),
            ParserState::SingleQuote | ParserState::DoubleQuote => {
                self.errored = true;
                None
            }
        }
    }
}

impl<'a> Iterator for ShellParser<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.parse_word()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_one_word() {
        let mut it = ShellParser::new(b"hello");
        assert_eq!(it.next(), Some("hello".as_bytes()));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn test_one_single() {
        let mut it = ShellParser::new(b"'hello'");
        assert_eq!(it.next(), Some("hello".as_bytes()));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn test_open_quote() {
        let mut it = ShellParser::new(b"'hello");
        assert_eq!(it.next(), None);
        assert!(it.errored)
    }

    #[test]
    fn test_ww2ogg() {
        let mut it = ShellParser::new(
            b"ww2ogg.exe --pcb \"/usr/share/ww2ogg/packed_cookbook_aoTuV_603.bin\"",
        );
        assert_eq!(it.next(), Some("ww2ogg.exe".as_bytes()));
        assert_eq!(it.next(), Some("--pcb".as_bytes()));
        assert_eq!(
            it.next(),
            Some("/usr/share/ww2ogg/packed_cookbook_aoTuV_603.bin".as_bytes())
        );
        assert_eq!(it.next(), None);
    }
}

#[cfg(test)]
mod bench {
    extern crate test;

    use super::*;
    #[cfg(feature = "shlex-bench")]
    use shlex::bytes::Shlex;
    use test::Bencher;

    mod ww2ogg {
        use super::*;

        #[bench]
        fn custom(b: &mut Bencher) {
            let val = test::black_box(
                b"ww2ogg.exe --pcb \"/usr/share/ww2ogg/packed_cookbook_aoTuV_603.bin\"",
            );
            b.iter(|| {
                let it = ShellParser::new(val);
                let _: Vec<_> = test::black_box(it.collect());
            })
        }

        #[cfg(feature = "shlex-bench")]
        #[bench]
        fn shlex(b: &mut Bencher) {
            let val = test::black_box(
                b"ww2ogg.exe --pcb \"/usr/share/ww2ogg/packed_cookbook_aoTuV_603.bin\"",
            );
            b.iter(|| {
                let it = Shlex::new(val);
                let _: Vec<_> = test::black_box(it.collect());
            })
        }
    }

    mod one_single {
        use super::*;

        #[bench]
        fn custom(b: &mut Bencher) {
            let val = test::black_box(b"'hello'");
            b.iter(|| {
                let it = ShellParser::new(val);
                let _: Vec<_> = test::black_box(it.collect());
            })
        }

        #[cfg(feature = "shlex-bench")]
        #[bench]
        fn shlex(b: &mut Bencher) {
            let val = test::black_box(b"'hello'");
            b.iter(|| {
                let it = Shlex::new(val);
                let _: Vec<_> = test::black_box(it.collect());
            })
        }
    }
}
