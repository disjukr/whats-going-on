use crate::cbor::{CborError, Value};
use crate::generated::wire::CodecError;
pub use crate::generated::wire::{
    DatagramMessage, PairedSecretCredential, ReqResMessage, RpcErrorCode, RpcErrorKind,
    RpcErrorPayload, SessionAuthErrorCode,
};

pub const MAX_MESSAGE_SEQUENCE_SIZE: usize = 64 * 1024 * 1024;
pub const PAIRED_SECRET_AUTH_MECHANISM: &str = "wgo.paired-secret.v1";

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("cbor error: {0}")]
    Cbor(#[from] CborError),
    #[error("codec error: {0:?}")]
    Codec(CodecError),
    #[error("message sequence is empty")]
    EmptySequence,
    #[error("reqres message sequence ended with an incomplete kind/map pair")]
    IncompleteMessagePair,
    #[error("message sequence exceeds implementation limit")]
    SequenceTooLarge,
    #[error("expected reqres message union tuple")]
    ExpectedReqResMessage,
}

impl From<CodecError> for WireError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}

impl ReqResMessage {
    pub fn encode(&self) -> Vec<u8> {
        let (kind, fields) = self
            .to_flattened_parts()
            .expect("generated reqres message failed to encode");
        let mut out = kind.encode();
        out.extend_from_slice(&fields.encode());
        out
    }

    pub fn encode_sequence(messages: &[Self]) -> Vec<u8> {
        let mut out = Vec::new();
        for message in messages {
            out.extend_from_slice(&message.encode());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.len() > MAX_MESSAGE_SEQUENCE_SIZE {
            return Err(WireError::SequenceTooLarge);
        }
        let mut values = Value::decode_sequence(bytes)?;
        if values.len() != 2 {
            return Err(WireError::IncompleteMessagePair);
        }
        let fields = values.pop().ok_or(WireError::IncompleteMessagePair)?;
        let kind = values.pop().ok_or(WireError::IncompleteMessagePair)?;
        Self::from_flattened_parts(kind, fields)
    }

    pub fn decode_prefix(bytes: &[u8]) -> Result<Option<(Self, usize)>, WireError> {
        if bytes.len() > MAX_MESSAGE_SEQUENCE_SIZE {
            return Err(WireError::SequenceTooLarge);
        }
        let Some((kind, kind_len)) = Value::decode_prefix(bytes)? else {
            return Ok(None);
        };
        let Some((fields, fields_len)) = Value::decode_prefix(&bytes[kind_len..])? else {
            return Ok(None);
        };
        let message = Self::from_flattened_parts(kind, fields)?;
        Ok(Some((message, kind_len + fields_len)))
    }

    pub fn decode_sequence(bytes: &[u8]) -> Result<Vec<Self>, WireError> {
        if bytes.len() > MAX_MESSAGE_SEQUENCE_SIZE {
            return Err(WireError::SequenceTooLarge);
        }
        let values = Value::decode_sequence(bytes)?;
        if values.is_empty() {
            return Err(WireError::EmptySequence);
        }
        if values.len() % 2 != 0 {
            return Err(WireError::IncompleteMessagePair);
        }

        let mut messages = Vec::with_capacity(values.len() / 2);
        let mut values = values.into_iter();
        while let Some(kind) = values.next() {
            let fields = values.next().ok_or(WireError::IncompleteMessagePair)?;
            messages.push(Self::from_flattened_parts(kind, fields)?);
        }
        Ok(messages)
    }

    pub fn proc_id(&self) -> Option<u64> {
        match self {
            Self::RequestUnary { proc_id, .. } | Self::RequestStreamStart { proc_id, .. } => {
                Some(*proc_id)
            }
            Self::RequestStreamChunk { .. }
            | Self::ResponseUnaryOk { .. }
            | Self::ResponseUnaryError { .. }
            | Self::ResponseStreamStart { .. }
            | Self::ResponseStreamChunk { .. }
            | Self::ResponseStreamErrorEnd { .. }
            | Self::SessionAuthenticate { .. }
            | Self::SessionAuthenticated
            | Self::SessionAuthError { .. } => None,
        }
    }

    pub fn payload(&self) -> Option<&[u8]> {
        match self {
            Self::RequestUnary { payload, .. }
            | Self::RequestStreamStart { payload, .. }
            | Self::ResponseUnaryOk { payload }
            | Self::ResponseStreamStart { payload } => payload.as_deref(),
            Self::RequestStreamChunk { payload }
            | Self::ResponseStreamChunk { payload }
            | Self::SessionAuthenticate { payload, .. } => Some(payload),
            Self::ResponseUnaryError { .. }
            | Self::ResponseStreamErrorEnd { .. }
            | Self::SessionAuthenticated
            | Self::SessionAuthError { .. } => None,
        }
    }

    pub fn error(&self) -> Option<&[u8]> {
        match self {
            Self::ResponseUnaryError { error, .. } | Self::ResponseStreamErrorEnd { error, .. } => {
                Some(error)
            }
            Self::RequestUnary { .. }
            | Self::RequestStreamStart { .. }
            | Self::RequestStreamChunk { .. }
            | Self::ResponseUnaryOk { .. }
            | Self::ResponseStreamStart { .. }
            | Self::ResponseStreamChunk { .. }
            | Self::SessionAuthenticate { .. }
            | Self::SessionAuthenticated
            | Self::SessionAuthError { .. } => None,
        }
    }

    pub fn error_kind(&self) -> Option<RpcErrorKind> {
        match self {
            Self::ResponseUnaryError { error_kind, .. }
            | Self::ResponseStreamErrorEnd { error_kind, .. } => Some(error_kind.clone()),
            Self::RequestUnary { .. }
            | Self::RequestStreamStart { .. }
            | Self::RequestStreamChunk { .. }
            | Self::ResponseUnaryOk { .. }
            | Self::ResponseStreamStart { .. }
            | Self::ResponseStreamChunk { .. }
            | Self::SessionAuthenticate { .. }
            | Self::SessionAuthenticated
            | Self::SessionAuthError { .. } => None,
        }
    }

    pub fn is_rpc_request(&self) -> bool {
        matches!(
            self,
            Self::RequestUnary { .. }
                | Self::RequestStreamStart { .. }
                | Self::RequestStreamChunk { .. }
        )
    }

    pub fn is_rpc_response(&self) -> bool {
        matches!(
            self,
            Self::ResponseUnaryOk { .. }
                | Self::ResponseUnaryError { .. }
                | Self::ResponseStreamStart { .. }
                | Self::ResponseStreamChunk { .. }
                | Self::ResponseStreamErrorEnd { .. }
        )
    }

    pub fn is_session_control(&self) -> bool {
        matches!(
            self,
            Self::SessionAuthenticate { .. }
                | Self::SessionAuthenticated
                | Self::SessionAuthError { .. }
        )
    }

    fn to_flattened_parts(&self) -> Result<(Value, Value), CodecError> {
        let Value::Array(mut items) = self.encode_value()? else {
            return Err(CodecError::ExpectedArray);
        };
        if items.len() != 2 {
            return Err(CodecError::ExpectedArray);
        }
        let fields = items.pop().ok_or(CodecError::ExpectedArray)?;
        let kind = items.pop().ok_or(CodecError::ExpectedArray)?;
        Ok((kind, fields))
    }

    fn from_flattened_parts(kind: Value, fields: Value) -> Result<Self, WireError> {
        Ok(Self::decode_value(&Value::Array(vec![kind, fields]))?)
    }
}

impl DatagramMessage {
    pub fn encode(&self) -> Vec<u8> {
        self.encode_value()
            .expect("generated datagram message failed to encode")
            .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        Ok(Self::decode_value(&Value::decode(bytes)?)?)
    }
}

impl PairedSecretCredential {
    pub fn encode(&self) -> Vec<u8> {
        self.encode_value()
            .expect("generated paired secret credential failed to encode")
            .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        Ok(Self::decode_value(&Value::decode(bytes)?)?)
    }
}

impl RpcErrorPayload {
    pub fn encode(&self) -> Vec<u8> {
        self.encode_value()
            .expect("generated rpc error payload failed to encode")
            .encode()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        Ok(Self::decode_value(&Value::decode(bytes)?)?)
    }
}

impl RpcErrorCode {
    pub fn as_u64(&self) -> u64 {
        match self {
            Self::BadMessage => 1,
            Self::Unauthorized => 2,
            Self::MissingPayload => 3,
            Self::NotImplemented => 4,
            Self::PermissionDenied => 6,
            Self::NotFound => 7,
            Self::OperationFailed => 8,
            Self::MalformedPayload => 9,
        }
    }
}

impl RpcErrorKind {
    pub fn as_u64(&self) -> u64 {
        match self {
            Self::System => 1,
            Self::Method => 2,
        }
    }
}

impl SessionAuthErrorCode {
    pub fn as_u64(&self) -> u64 {
        match self {
            Self::UnsupportedMechanism => 1,
            Self::InvalidCredentials => 2,
            Self::MalformedPayload => 3,
            Self::AlreadyAuthenticated => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::ProcId;

    #[test]
    fn encodes_and_decodes_reqres_message() {
        let message = ReqResMessage::RequestUnary {
            proc_id: ProcId::StartPairing.as_u64(),
            payload: Some(b"hello".to_vec()),
        };
        assert_eq!(ReqResMessage::decode(&message.encode()).unwrap(), message);
    }

    #[test]
    fn decodes_cbor_message_sequence() {
        let first = ReqResMessage::RequestStreamStart {
            proc_id: ProcId::StartPairing.as_u64(),
            payload: None,
        };
        let second = ReqResMessage::RequestStreamChunk {
            payload: b"done".to_vec(),
        };
        let bytes = ReqResMessage::encode_sequence(&[first.clone(), second.clone()]);
        assert_eq!(
            ReqResMessage::decode_sequence(&bytes).unwrap(),
            vec![first, second]
        );
    }

    #[test]
    fn request_unary_vector_is_stable() {
        let message = ReqResMessage::RequestUnary {
            proc_id: ProcId::StartPairing.as_u64(),
            payload: None,
        };
        assert_eq!(
            ReqResMessage::encode_sequence(&[message]),
            vec![0x00, 0xa1, 0x01, 0x02]
        );
    }

    #[test]
    fn session_authenticate_roundtrip() {
        let credential = PairedSecretCredential {
            credential_id: "client".to_string(),
            credential_secret: "secret".to_string(),
        };
        let message = ReqResMessage::SessionAuthenticate {
            mechanism: PAIRED_SECRET_AUTH_MECHANISM.to_string(),
            payload: credential.encode(),
        };
        assert_eq!(ReqResMessage::decode(&message.encode()).unwrap(), message);
    }

    #[test]
    fn datagram_ping_roundtrip() {
        let message = DatagramMessage::Ping { ping_id: 42 };
        assert_eq!(DatagramMessage::decode(&message.encode()).unwrap(), message);
        assert_eq!(message.encode(), vec![0x82, 0x01, 0xa1, 0x01, 0x18, 0x2a]);
    }
}
