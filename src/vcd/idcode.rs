use std::error::Error;
use std::fmt::{self, Display};
use std::str::FromStr;

/// Parse error for invalid ID code.
#[derive(Debug, Clone)]
pub enum InvalidIdCode {
    /// ID is empty
    Empty,
    /// ID contains invalid characters
    InvalidChars,
    /// ID is too long
    TooLong,
}

impl Display for InvalidIdCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "ID cannot be empty"),
            Self::InvalidChars => write!(f, "invalid characters in ID"),
            Self::TooLong => write!(f, "ID too long"),
        }
    }
}

impl Error for InvalidIdCode { }

/// An ID used within the file to refer to a particular variable.
#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct IdCode(Vec<u8>);

const ID_CHAR_MIN: u8 = b'!';
const ID_CHAR_MAX: u8 = b'~';
const NUM_ID_CHARS: u64 = (ID_CHAR_MAX - ID_CHAR_MIN + 1) as u64;

impl IdCode {
    fn new(v: &[u8]) -> Result<IdCode, InvalidIdCode> {
        if v.is_empty() {
            return Err(InvalidIdCode::Empty);
        }
        for &i in v.iter() {
            if !(ID_CHAR_MIN..=ID_CHAR_MAX).contains(&i) {
                return Err(InvalidIdCode::InvalidChars);
            }
        }
        Ok(IdCode(v.to_vec()))
    }

    /// An arbitrary IdCode with a short representation.
    pub fn first() -> IdCode {
        IdCode(vec![ID_CHAR_MIN])
    }

    /// Returns the IdCode following this one in an arbitrary sequence.
    pub fn next(&self) -> IdCode {
        let mut next = self.0.clone();
        for c in &mut next {
            if *c < ID_CHAR_MAX {
                *c += 1;
                return IdCode(next);
            }
            *c = ID_CHAR_MIN;
        }
        next.push(ID_CHAR_MIN);
        IdCode(next)
    }
}

impl FromStr for IdCode {
    type Err = InvalidIdCode;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        IdCode::new(s.as_bytes())
    }
}

impl From<u32> for IdCode {
    fn from(i: u32) -> IdCode {
        IdCode::from(i as u64)
    }
}

impl From<u64> for IdCode {
    fn from(i: u64) -> IdCode {
        let mut i = i;
        let mut result = Vec::new();
        loop {
            let r = i % NUM_ID_CHARS;
            result.push(r as u8 + ID_CHAR_MIN);
            if i < NUM_ID_CHARS {
                break;
            }
            i = i / NUM_ID_CHARS - 1;
        }
        IdCode(result)
    }
}

impl From<IdCode> for u64 {
    fn from(i: IdCode) -> u64 {
        let mut result = 0u64;
        for &b in i.0.iter().rev() {
            let c = ((b - ID_CHAR_MIN) as u64) + 1;
            result = result * NUM_ID_CHARS + c;
        }
        result - 1
    }
}

impl Display for IdCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &c in &self.0 {
            write!(f, "{}", c as char)?;
        }
        Ok(())
    }
}

#[test]
fn test_id_code() {
    let mut id = IdCode::first();
    for i in 0..10000 {
        println!("{} {}", i, id);
        assert_eq!(id.to_string().parse::<IdCode>().unwrap(), id);
        id = id.next();
    }

    assert_eq!("!".parse::<IdCode>().unwrap().to_string(), "!");
    assert_eq!(
        "!!!!!!!!!!".parse::<IdCode>().unwrap().to_string(),
        "!!!!!!!!!!"
    );
    assert_eq!("~".parse::<IdCode>().unwrap().to_string(), "~");
    assert_eq!(
        "~~~~~~~~~".parse::<IdCode>().unwrap().to_string(),
        "~~~~~~~~~"
    );
    assert_eq!(
        "n999999999".parse::<IdCode>().unwrap().to_string(),
        "n999999999"
    );
    assert_eq!(
        "hbi_data__dec".parse::<IdCode>().unwrap().to_string(),
        "hbi_data__dec"
    );
}
