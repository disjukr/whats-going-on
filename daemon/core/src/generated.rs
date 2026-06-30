pub mod wire {
    #![allow(dead_code, unused_mut, unused_variables)]

    include!(concat!(env!("OUT_DIR"), "/wire.rs"));
}

pub mod rpc {
    #![allow(dead_code, unused_mut, unused_variables)]

    include!(concat!(env!("OUT_DIR"), "/rpc.rs"));
}

#[cfg(test)]
mod tests {
    use super::rpc;

    #[test]
    fn generated_rpc_model_roundtrips() {
        let value = rpc::DaemonInfo {
            supported_proc_ids: vec![1, 2, 3],
            version: "0.1.0".to_string(),
            os: "windows".to_string(),
            instance_id: "1234-5678".to_string(),
            started_at_ms: 1_234,
            server_time_ms: 5_678,
        };

        let encoded = value.encode();
        assert_eq!(rpc::DaemonInfo::decode(&encoded).unwrap(), value);
    }

    #[test]
    fn generated_u53_model_encoding_rejects_large_integer() {
        let value = rpc::DaemonInfo {
            supported_proc_ids: vec![u64::MAX],
            version: String::new(),
            os: String::new(),
            instance_id: String::new(),
            started_at_ms: 0,
            server_time_ms: 0,
        };

        assert!(matches!(
            value.try_encode(),
            Err(rpc::CodecError::IntegerOutOfRange("u53"))
        ));
    }

    #[test]
    fn generated_proc_metadata_decodes_request_payload() {
        let payload = rpc::StartPairingReq {
            confirmation_code: "42".to_string(),
            client_label: "test".to_string(),
            client_id: Some("client-1".to_string()),
        };

        assert_eq!(rpc::ProcId::from_u64(2), Some(rpc::ProcId::StartPairing));
        assert_eq!(rpc::PROC_DEFINITIONS.len(), rpc::ProcId::KNOWN.len());
        assert_eq!(rpc::PROC_DEFINITIONS[1].id, rpc::ProcId::StartPairing);
        assert_eq!(rpc::PROC_DEFINITIONS[1].wire_id, 2);
        assert_eq!(rpc::PROC_DEFINITIONS[1].name, "StartPairing");
        assert_eq!(rpc::ProcId::StartPairing.stream(), rpc::ProcStream::Unary);
        assert_eq!(
            rpc::RpcRequest::decode(2, Some(&payload.encode())).unwrap(),
            rpc::RpcRequest::StartPairing(payload)
        );
        assert!(matches!(
            rpc::RpcRequest::decode(2, None),
            Err(rpc::RpcRequestDecodeError::MissingPayload {
                proc: rpc::ProcId::StartPairing
            })
        ));
    }
}
