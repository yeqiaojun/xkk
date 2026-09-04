use std::collections::HashSet;

use super::*;
use xmongo::BsonPathGetter;

#[test]
fn msg_ids_are_generated_from_proto() {
    assert_eq!(MsgId::AckNtf.as_u16(), 3000);
    assert_eq!(MsgId::PlayerInfoReq.as_u16(), 100);
    assert_eq!(MsgId::PlayerInfoReq.as_str_name(), "PLAYER_INFO_REQ");
    assert_eq!(from_u16(100), Some(MsgId::PlayerInfoReq));
    assert_eq!(from_u16(99), None);
}

#[test]
fn retired_query_config_ids_remain_unassigned() {
    let registry = message_registry().unwrap();
    for id in 2102..=2105 {
        assert_eq!(from_u16(id), None);
        assert_eq!(registry.message_name(id), None);
    }
}

#[test]
fn every_generated_request_has_a_distinct_response() {
    let registry = message_registry().unwrap();
    let mut seen = HashSet::new();
    for value in 1..=u16::MAX {
        let Some(request) = from_u16(value) else {
            continue;
        };
        assert!(seen.insert(request.as_u16()));
        if request.kind() != Some(MessageKind::Req) {
            continue;
        }
        let response_id = registry.response_id(request.as_u16()).unwrap();
        let response = from_u16(response_id).unwrap();
        assert_ne!(request, response);
        assert_eq!(response.kind(), Some(MessageKind::Rsp));
        assert_eq!(registry.request_id(response_id), Some(request.as_u16()));
    }
}

#[test]
fn only_control_and_named_session_messages_bypass_outbox() {
    assert!(!is_outbox_message(MsgId::LoginRsp.as_u16()));
    assert!(!is_outbox_message(MsgId::KickNtf.as_u16()));
    assert!(is_outbox_message(MsgId::LogicLoginRsp.as_u16()));
    assert!(is_outbox_message(MsgId::PlayerInfoRsp.as_u16()));
    assert!(is_outbox_message(MsgId::MailInfoNtf.as_u16()));
}

#[test]
fn complete_registry_has_every_generated_message() {
    let registry = message_registry().unwrap();
    registry.validate_complete().unwrap();
    let body = prost::Message::encode_to_vec(&pb::PingReq { client_time_ms: 7 });
    assert!(registry.decode(MsgId::PingReq.as_u16(), &body).is_ok());

    for raw in 1..=u16::MAX {
        let Some(msgid) = from_u16(raw) else {
            continue;
        };
        let (name, _) = registry.format_json(raw, &[]);
        assert_ne!(name, "UNKNOWN", "message {msgid:?} was not registered");
    }
}

#[test]
fn generated_message_mapping_is_bidirectional() {
    let registry = message_registry().unwrap();
    assert_eq!(registry.message_id_for_type::<pb::PingReq>(), Some(MsgId::PingReq.as_u16()));

    let ping = registry.new_message(MsgId::PingReq.as_u16()).unwrap();
    assert!(ping.downcast_ref::<pb::PingReq>().is_some());

    for raw in 1..=u16::MAX {
        let Some(msgid) = from_u16(raw) else {
            continue;
        };
        let message = registry.new_message(raw).expect("generated message must have a factory");
        assert_eq!(message.msgid(), msgid.as_u16());
    }
}

#[test]
fn generated_xmongo_traits_roundtrip_player_data() {
    let mut player = pb::PlayerData {
        gid: 1001,
        profile: Some(pb::PlayerInfo { gid: 1001, name: "tester".to_string(), level: 2, icon: 3, exp: 4 }),
        ..Default::default()
    };
    player.items.insert(2001, 7);

    let bson = player.bson_value().unwrap();
    let decoded = pb::PlayerData::from_bson_value(&bson).unwrap();

    assert_eq!(decoded, player);
}

#[test]
fn generated_xmongo_traits_roundtrip_public_player_data() {
    let player = pb::PublicPlayerData {
        gid: 1002,
        mail: Some(pb::MailData {
            mails: vec![pb::Mail { mail_id: 7, title: "welcome".to_string(), content: "hello".to_string(), ..Default::default() }],
        }),
    };

    let bson = player.bson_value().unwrap();
    let decoded = pb::PublicPlayerData::from_bson_value(&bson).unwrap();

    assert_eq!(decoded, player);
}
