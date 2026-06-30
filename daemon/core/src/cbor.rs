use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum CborError {
    #[error("unexpected end of input")]
    UnexpectedEnd,
    #[error("unsupported CBOR type")]
    UnsupportedType,
    #[error("non-canonical integer encoding")]
    NonCanonicalInteger,
    #[error("duplicate map key")]
    DuplicateMapKey,
    #[error("expected unsigned integer map key")]
    ExpectedUnsignedMapKey,
    #[error("trailing bytes after value")]
    TrailingBytes,
    #[error("invalid utf-8 string")]
    InvalidUtf8,
    #[error("length exceeds implementation limit")]
    LengthTooLarge,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    Map(BTreeMap<u64, Value>),
}

impl Value {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        encode_value(self, &mut out);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CborError> {
        let mut cursor = Cursor::new(bytes);
        let value = decode_value(&mut cursor)?;
        if cursor.position() != bytes.len() as u64 {
            return Err(CborError::TrailingBytes);
        }
        Ok(value)
    }

    pub fn decode_sequence(bytes: &[u8]) -> Result<Vec<Self>, CborError> {
        let mut cursor = Cursor::new(bytes);
        let mut values = Vec::new();
        while cursor.position() != bytes.len() as u64 {
            values.push(decode_value(&mut cursor)?);
        }
        Ok(values)
    }

    pub fn decode_prefix(bytes: &[u8]) -> Result<Option<(Self, usize)>, CborError> {
        let mut cursor = Cursor::new(bytes);
        match decode_value(&mut cursor) {
            Ok(value) => Ok(Some((value, cursor.position() as usize))),
            Err(CborError::UnexpectedEnd) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

fn encode_value(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null => out.push(0xf6),
        Value::Bool(false) => out.push(0xf4),
        Value::Bool(true) => out.push(0xf5),
        Value::U64(value) => encode_type_value(0, *value, out),
        Value::I64(value) if *value >= 0 => encode_type_value(0, *value as u64, out),
        Value::I64(value) => encode_type_value(1, (-1 - *value) as u64, out),
        Value::F64(value) => {
            out.push(0xfb);
            out.extend_from_slice(&value.to_bits().to_be_bytes());
        }
        Value::Bytes(bytes) => {
            encode_type_value(2, bytes.len() as u64, out);
            out.extend_from_slice(bytes);
        }
        Value::Text(text) => {
            encode_type_value(3, text.len() as u64, out);
            out.extend_from_slice(text.as_bytes());
        }
        Value::Array(items) => {
            encode_type_value(4, items.len() as u64, out);
            for item in items {
                encode_value(item, out);
            }
        }
        Value::Map(map) => {
            encode_type_value(5, map.len() as u64, out);
            for (key, value) in map {
                encode_type_value(0, *key, out);
                encode_value(value, out);
            }
        }
    }
}

fn encode_type_value(major: u8, value: u64, out: &mut Vec<u8>) {
    let prefix = major << 5;
    if value < 24 {
        out.push(prefix | value as u8);
    } else if value <= u8::MAX as u64 {
        out.push(prefix | 24);
        out.push(value as u8);
    } else if value <= u16::MAX as u64 {
        out.push(prefix | 25);
        out.extend_from_slice(&(value as u16).to_be_bytes());
    } else if value <= u32::MAX as u64 {
        out.push(prefix | 26);
        out.extend_from_slice(&(value as u32).to_be_bytes());
    } else {
        out.push(prefix | 27);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

fn decode_value(cursor: &mut Cursor<&[u8]>) -> Result<Value, CborError> {
    let initial = read_u8(cursor)?;
    let major = initial >> 5;
    let additional = initial & 0x1f;
    match major {
        0 => Ok(Value::U64(decode_argument(additional, cursor)?)),
        1 => {
            let encoded = decode_argument(additional, cursor)?;
            if encoded > i64::MAX as u64 {
                return Err(CborError::LengthTooLarge);
            }
            Ok(Value::I64(-1 - encoded as i64))
        }
        2 => {
            let len = checked_len(decode_argument(additional, cursor)?)?;
            let mut bytes = vec![0u8; len];
            cursor
                .read_exact(&mut bytes)
                .map_err(|_| CborError::UnexpectedEnd)?;
            Ok(Value::Bytes(bytes))
        }
        3 => {
            let len = checked_len(decode_argument(additional, cursor)?)?;
            let mut bytes = vec![0u8; len];
            cursor
                .read_exact(&mut bytes)
                .map_err(|_| CborError::UnexpectedEnd)?;
            String::from_utf8(bytes)
                .map(Value::Text)
                .map_err(|_| CborError::InvalidUtf8)
        }
        4 => {
            let len = checked_len(decode_argument(additional, cursor)?)?;
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(decode_value(cursor)?);
            }
            Ok(Value::Array(items))
        }
        5 => {
            let len = checked_len(decode_argument(additional, cursor)?)?;
            let mut map = BTreeMap::new();
            for _ in 0..len {
                let key = decode_value(cursor)?;
                let Value::U64(key) = key else {
                    return Err(CborError::ExpectedUnsignedMapKey);
                };
                let value = decode_value(cursor)?;
                if map.insert(key, value).is_some() {
                    return Err(CborError::DuplicateMapKey);
                }
            }
            Ok(Value::Map(map))
        }
        7 => match additional {
            20 => Ok(Value::Bool(false)),
            21 => Ok(Value::Bool(true)),
            22 => Ok(Value::Null),
            27 => {
                let mut bytes = [0u8; 8];
                cursor
                    .read_exact(&mut bytes)
                    .map_err(|_| CborError::UnexpectedEnd)?;
                Ok(Value::F64(f64::from_bits(u64::from_be_bytes(bytes))))
            }
            _ => Err(CborError::UnsupportedType),
        },
        _ => Err(CborError::UnsupportedType),
    }
}

fn read_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8, CborError> {
    let mut byte = [0u8; 1];
    cursor
        .read_exact(&mut byte)
        .map_err(|_| CborError::UnexpectedEnd)?;
    Ok(byte[0])
}

fn decode_argument(additional: u8, cursor: &mut Cursor<&[u8]>) -> Result<u64, CborError> {
    match additional {
        value @ 0..=23 => Ok(value as u64),
        24 => {
            let value = read_u8(cursor)? as u64;
            if value < 24 {
                return Err(CborError::NonCanonicalInteger);
            }
            Ok(value)
        }
        25 => {
            let mut bytes = [0u8; 2];
            cursor
                .read_exact(&mut bytes)
                .map_err(|_| CborError::UnexpectedEnd)?;
            let value = u16::from_be_bytes(bytes) as u64;
            if value <= u8::MAX as u64 {
                return Err(CborError::NonCanonicalInteger);
            }
            Ok(value)
        }
        26 => {
            let mut bytes = [0u8; 4];
            cursor
                .read_exact(&mut bytes)
                .map_err(|_| CborError::UnexpectedEnd)?;
            let value = u32::from_be_bytes(bytes) as u64;
            if value <= u16::MAX as u64 {
                return Err(CborError::NonCanonicalInteger);
            }
            Ok(value)
        }
        27 => {
            let mut bytes = [0u8; 8];
            cursor
                .read_exact(&mut bytes)
                .map_err(|_| CborError::UnexpectedEnd)?;
            let value = u64::from_be_bytes(bytes);
            if value <= u32::MAX as u64 {
                return Err(CborError::NonCanonicalInteger);
            }
            Ok(value)
        }
        _ => Err(CborError::UnsupportedType),
    }
}

fn checked_len(len: u64) -> Result<usize, CborError> {
    usize::try_from(len).map_err(|_| CborError::LengthTooLarge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_struct_like_map_with_field_ids() {
        let value = Value::Map(BTreeMap::from([
            (1, Value::Text("C:\\Users".to_string())),
            (2, Value::Bool(false)),
        ]));
        let encoded = value.encode();
        assert_eq!(
            encoded,
            vec![0xa2, 0x01, 0x68, b'C', b':', b'\\', b'U', b's', b'e', b'r', b's', 0x02, 0xf4]
        );
        assert_eq!(Value::decode(&encoded).unwrap(), value);
    }

    #[test]
    fn rejects_duplicate_map_keys() {
        let bytes = [0xa2, 0x01, 0x01, 0x01, 0x02];
        assert_eq!(Value::decode(&bytes), Err(CborError::DuplicateMapKey));
    }
}
