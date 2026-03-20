use getrandom::fill as fill_random;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::sync::{Mutex, OnceLock};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PREFIX: &str = "kf_";
const BODY_LEN: usize = 26;
const ENCODED_LEN: usize = PREFIX.len() + BODY_LEN;
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactId {
    bytes: [u8; 16],
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactIdParseError {
    InvalidLength { found: usize },
    MissingPrefix,
    InvalidCharacter { ch: char, index: usize },
    Overflow,
}

impl fmt::Display for FactIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FactIdParseError::InvalidLength { found } => {
                write!(
                    f,
                    "invalid fact id length: expected {ENCODED_LEN} characters, got {found}"
                )
            }
            FactIdParseError::MissingPrefix => write!(f, "invalid fact id prefix: expected `kf_`"),
            FactIdParseError::InvalidCharacter { ch, index } => {
                write!(f, "invalid fact id character `{ch}` at index {index}")
            }
            FactIdParseError::Overflow => write!(f, "invalid fact id payload overflow"),
        }
    }
}

impl std::error::Error for FactIdParseError {}

#[derive(Debug)]
struct GeneratorState {
    last_ms: u64,
    sequence: u16,
}

impl GeneratorState {
    const fn new() -> Self {
        Self {
            last_ms: 0,
            sequence: 0,
        }
    }

    fn next_parts(&mut self) -> (u64, u16) {
        let mut now_ms = now_ms();
        loop {
            match now_ms.cmp(&self.last_ms) {
                std::cmp::Ordering::Greater => {
                    self.last_ms = now_ms;
                    self.sequence = 0;
                    return (now_ms, 0);
                }
                std::cmp::Ordering::Equal => {
                    if self.sequence < u16::MAX {
                        self.sequence += 1;
                        return (now_ms, self.sequence);
                    }
                    now_ms = wait_until_next_millisecond(self.last_ms);
                }
                std::cmp::Ordering::Less => {
                    if self.sequence < u16::MAX {
                        self.sequence += 1;
                        return (self.last_ms, self.sequence);
                    }
                    now_ms = wait_until_next_millisecond(self.last_ms);
                }
            }
        }
    }
}

static GENERATOR: OnceLock<Mutex<GeneratorState>> = OnceLock::new();

impl FactId {
    pub fn new() -> Self {
        Self::try_new().expect("OS randomness unavailable for Kronroe Fact ID generation")
    }

    pub fn try_new() -> Result<Self, getrandom::Error> {
        let generator = GENERATOR.get_or_init(|| Mutex::new(GeneratorState::new()));
        let mut state = generator
            .lock()
            .expect("Kronroe Fact ID generator mutex poisoned");
        let (timestamp_ms, sequence) = state.next_parts();
        drop(state);

        let mut entropy_bytes = [0u8; 8];
        fill_random(&mut entropy_bytes)?;
        let entropy = u64::from_be_bytes(entropy_bytes);

        Ok(Self::from_parts(timestamp_ms, sequence, entropy))
    }

    pub fn from_parts(timestamp_ms: u64, sequence: u16, entropy: u64) -> Self {
        let mut bytes = [0u8; 16];
        bytes[0] = ((timestamp_ms >> 40) & 0xFF) as u8;
        bytes[1] = ((timestamp_ms >> 32) & 0xFF) as u8;
        bytes[2] = ((timestamp_ms >> 24) & 0xFF) as u8;
        bytes[3] = ((timestamp_ms >> 16) & 0xFF) as u8;
        bytes[4] = ((timestamp_ms >> 8) & 0xFF) as u8;
        bytes[5] = (timestamp_ms & 0xFF) as u8;
        bytes[6] = ((sequence >> 8) & 0xFF) as u8;
        bytes[7] = (sequence & 0xFF) as u8;
        bytes[8..].copy_from_slice(&entropy.to_be_bytes());
        Self {
            text: encode_text(bytes),
            bytes,
        }
    }

    pub fn parse(text: &str) -> Result<Self, FactIdParseError> {
        let body = text
            .strip_prefix(PREFIX)
            .ok_or(FactIdParseError::MissingPrefix)?;
        if body.len() != BODY_LEN {
            return Err(FactIdParseError::InvalidLength { found: text.len() });
        }

        let mut value = 0u128;
        for (idx, ch) in body.char_indices() {
            let decoded = decode_char(ch).ok_or(FactIdParseError::InvalidCharacter {
                ch,
                index: PREFIX.len() + idx,
            })?;
            if idx == 0 && decoded > 0b111 {
                return Err(FactIdParseError::Overflow);
            }
            value = (value << 5) | u128::from(decoded);
        }

        let bytes = value.to_be_bytes();
        Ok(Self {
            bytes,
            text: encode_text(bytes),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.bytes
    }

    pub fn timestamp_ms(&self) -> u64 {
        ((self.bytes[0] as u64) << 40)
            | ((self.bytes[1] as u64) << 32)
            | ((self.bytes[2] as u64) << 24)
            | ((self.bytes[3] as u64) << 16)
            | ((self.bytes[4] as u64) << 8)
            | (self.bytes[5] as u64)
    }

    pub fn sequence(&self) -> u16 {
        ((self.bytes[6] as u16) << 8) | self.bytes[7] as u16
    }
}

impl Default for FactId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for FactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for FactId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Serialize for FactId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FactId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(de::Error::custom)
    }
}

/// Current wall-clock time in milliseconds since UNIX epoch.
///
/// On native targets this uses `SystemTime::now()`. On `wasm32-unknown-unknown`
/// it delegates to JavaScript's `Date.now()` because `SystemTime` has no clock
/// source on bare WASM.
#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

fn wait_until_next_millisecond(last_ms: u64) -> u64 {
    loop {
        let current = now_ms();
        if current > last_ms {
            return current;
        }
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(std::time::Duration::from_millis(1));
        #[cfg(target_arch = "wasm32")]
        std::hint::spin_loop();
    }
}

fn encode_text(bytes: [u8; 16]) -> String {
    let mut out = String::with_capacity(ENCODED_LEN);
    out.push_str(PREFIX);

    let mut value = u128::from_be_bytes(bytes);
    let mut body = [b'0'; BODY_LEN];
    for idx in (0..BODY_LEN).rev() {
        body[idx] = ALPHABET[(value & 0x1F) as usize];
        value >>= 5;
    }
    out.push_str(std::str::from_utf8(&body).expect("base32 alphabet is ASCII"));
    out
}

fn decode_char(ch: char) -> Option<u8> {
    let upper = ch.to_ascii_uppercase();
    ALPHABET
        .iter()
        .position(|candidate| *candidate as char == upper)
        .map(|idx| idx as u8)
}

pub(crate) fn deterministic_entropy(seed: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in seed.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fact_id_roundtrips() {
        let id = FactId::from_parts(1_742_355_200_123, 42, 0x0123_4567_89ab_cdef);
        let parsed = FactId::parse(id.as_str()).expect("parse");
        assert_eq!(parsed, id);
    }

    #[test]
    fn fact_id_rejects_invalid_prefix() {
        let err = FactId::parse("uf_01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap_err();
        assert!(matches!(err, FactIdParseError::MissingPrefix));
    }

    #[test]
    fn fact_id_rejects_invalid_character() {
        let err = FactId::parse("kf_01ARZ3NDEKTSV4RRFFQ69G5FAI").unwrap_err();
        assert!(matches!(err, FactIdParseError::InvalidCharacter { .. }));
    }

    #[test]
    fn text_order_matches_binary_order() {
        let low = FactId::from_parts(100, 0, 1);
        let high = FactId::from_parts(100, 1, 0);
        assert!(low < high);
        assert!(low.as_str() < high.as_str());
    }

    #[test]
    fn sequence_is_embedded_in_id() {
        let first = FactId::from_parts(500, 0, 9);
        let second = FactId::from_parts(500, 1, 1);
        assert_eq!(first.timestamp_ms(), second.timestamp_ms());
        assert!(first < second);
    }
}
