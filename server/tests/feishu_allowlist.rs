use tvdisplay::feishu::allowlist::{chat_is_allowed, is_allowed_group, parse_allowed_chat_ids};

#[test]
fn parse_trims_and_drops_empty_segments() {
    assert_eq!(
        parse_allowed_chat_ids(" oc_aaa , ,oc_bbb"),
        vec!["oc_aaa", "oc_bbb"]
    );
}

#[test]
fn exact_match_only() {
    let allowed = parse_allowed_chat_ids("oc_aaa,oc_bbb");
    assert!(chat_is_allowed("oc_aaa", &allowed));
    assert!(!chat_is_allowed("oc_aa", &allowed));
    assert!(!chat_is_allowed(" oc_aaa", &allowed));
}

#[test]
fn private_chats_are_never_allowed() {
    let allowed = parse_allowed_chat_ids("oc_aaa");
    assert!(is_allowed_group("oc_aaa", "group", &allowed));
    assert!(!is_allowed_group("oc_aaa", "p2p", &allowed));
    assert!(!is_allowed_group("oc_zzz", "group", &allowed));
}
