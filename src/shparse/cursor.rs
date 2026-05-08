//! Byte cursor over UTF-8 source. Shell syntax is byte-oriented — operators,
//! quotes, and whitespace are all ASCII — so we iterate bytes and only decode
//! when emitting a `Word` string.

pub struct Cursor<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(src: &'a str) -> Self {
        Self { src: src.as_bytes(), pos: 0 }
    }

    #[cfg(test)]
    pub fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    pub fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    pub fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    pub fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    pub fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn eat_str(&mut self, s: &[u8]) -> bool {
        if self.src[self.pos..].starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    pub fn starts_with(&self, s: &[u8]) -> bool {
        self.src[self.pos..].starts_with(s)
    }

    pub fn eat_while(&mut self, mut f: impl FnMut(u8) -> bool) -> &'a [u8] {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if !f(b) {
                break;
            }
            self.pos += 1;
        }
        &self.src[start..self.pos]
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_bump_eof() {
        let mut c = Cursor::new("ab");
        assert_eq!(c.peek(), Some(b'a'));
        assert_eq!(c.bump(), Some(b'a'));
        assert_eq!(c.peek(), Some(b'b'));
        assert_eq!(c.bump(), Some(b'b'));
        assert!(c.is_eof());
        assert_eq!(c.bump(), None);
    }

    #[test]
    fn eat_and_eat_str() {
        let mut c = Cursor::new("&&||");
        assert!(c.eat_str(b"&&"));
        assert!(!c.eat_str(b"&&"));
        assert!(c.eat_str(b"||"));
    }

    #[test]
    fn eat_while_stops_on_predicate() {
        let mut c = Cursor::new("abc def");
        let run = c.eat_while(|b| b != b' ');
        assert_eq!(run, b"abc");
        assert_eq!(c.peek(), Some(b' '));
    }

    #[test]
    fn peek_at_offset() {
        let c = Cursor::new("&&");
        assert_eq!(c.peek_at(0), Some(b'&'));
        assert_eq!(c.peek_at(1), Some(b'&'));
        assert_eq!(c.peek_at(2), None);
    }
}
