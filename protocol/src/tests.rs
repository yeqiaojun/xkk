use std::collections::HashSet;

use super::*;
use xmongo::BsonPathGetter;

#[test]
fn msg_ids_are_generated_from_proto() {
    assert_eq!(MsgId::AckNtf.as_u16(), 1);
    assert_eq!(MsgId::PlayerInfoReq.as_u16(), 100);
    assert_eq!(MsgId::ConfigManifestRsp.as_u16(), 2105);
    assert_eq!(MsgId::PlayerInfoReq.as_str_name(), "PLAYER_INFO_REQ");
    assert_eq!(from_u16(100), Some(MsgId::PlayerInfoReq));
    assert_eq!(from_u16(99), None);
}

#[test]
fn every_generated_request_has_a_distinct_response() {
    let mut seen = HashSet::new();
    for value in 1..=u16::MAX {
        let Some(request) = from_u16(value) else {
            continue;
        };
        assert!(seen.insert(request.as_u16()));
        if request.kind() != Some(MessageKind::Req) {
            continue;
        }
        let response = response_for(request).unwrap();
        assert_ne!(request, response);
        assert_eq!(response.kind(), Some(MessageKind::Rsp));
    }
}

#[test]
fn only_control_range_bypasses_outbox() {
    assert!(!is_outbox_message(MsgId::LoginRsp.as_u16()));
    assert!(!is_outbox_message(MsgId::KickNtf.as_u16()));
    assert!(is_outbox_message(MsgId::PlayerInfoRsp.as_u16()));
    assert!(is_outbox_message(MsgId::MailInfoNtf.as_u16()));
}

#[test]
fn client_registry_has_every_client_message() {
    let registry = client_registry().unwrap();
    let body = prost::Message::encode_to_vec(&pb::PingReq { client_time_ms: 7 });
    assert!(registry.decode(MsgId::PingReq.as_u16(), &body).is_ok());
}

#[test]
fn generated_xmongo_traits_roundtrip_player_data() {
    let mut player = pb::PlayerData {
        gid: 1001,
        profile: Some(pb::PlayerInfo {
            gid: 1001,
            name: "tester".to_string(),
            level: 2,
            icon: 3,
            exp: 4,
        }),
        ..Default::default()
    };
    player.items.insert(2001, 7);

    let bson = player.bson_value().unwrap();
    let decoded = pb::PlayerData::from_bson_value(&bson).unwrap();

    assert_eq!(decoded, player);
}
