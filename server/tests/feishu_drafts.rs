use std::time::{Duration, Instant};
use tvdisplay::feishu::drafts::{Draft, DraftError, DraftStore, DRAFT_TTL, MAX_DRAFTS};

fn draft(id: &str, at: Instant) -> Draft {
    Draft {
        id: id.into(),
        text: format!("text-{id}"),
        image: Some(b"\xff\xd8\xff".to_vec()),
        created_at: at,
    }
}

#[test]
fn limits_and_ttl_constants() {
    assert_eq!(MAX_DRAFTS, 5);
    assert_eq!(DRAFT_TTL, Duration::from_secs(600));
}

#[test]
fn take_returns_the_pending_draft_once() {
    let mut store = DraftStore::new();
    let now = Instant::now();
    store.insert(draft("d1", now), now);
    let taken = store.take_for_publish("d1", now).unwrap();
    assert_eq!(taken.id, "d1");
    assert_eq!(taken.image.as_deref(), Some(b"\xff\xd8\xff".as_ref()));
    match store.take_for_publish("d1", now) {
        Err(DraftError::AlreadySent) => {}
        other => panic!("expected AlreadySent, got {other:?}"),
    }
}

#[test]
fn expired_draft_cannot_be_published() {
    let mut store = DraftStore::new();
    let now = Instant::now();
    store.insert(draft("d1", now), now);
    let later = now + DRAFT_TTL + Duration::from_secs(1);
    match store.take_for_publish("d1", later) {
        Err(DraftError::ExpiredOrMissing) => {}
        other => panic!("expected ExpiredOrMissing, got {other:?}"),
    }
}

#[test]
fn sixth_pending_draft_evicts_the_oldest() {
    let mut store = DraftStore::new();
    let now = Instant::now();
    for index in 1..=5 {
        assert!(store.insert(draft(&format!("d{index}"), now), now).is_none());
    }
    let evicted = store.insert(draft("d6", now), now);
    assert_eq!(evicted.as_deref(), Some("d1"));
    match store.take_for_publish("d1", now) {
        Err(DraftError::ExpiredOrMissing) => {}
        other => panic!("evicted draft must be expired, got {other:?}"),
    }
    assert!(store.take_for_publish("d6", now).is_ok());
}

#[test]
fn sent_drafts_do_not_count_toward_the_cap() {
    let mut store = DraftStore::new();
    let now = Instant::now();
    store.insert(draft("d1", now), now);
    store.take_for_publish("d1", now).unwrap();
    for index in 2..=6 {
        assert!(
            store.insert(draft(&format!("d{index}"), now), now).is_none(),
            "already-sent drafts must not force eviction"
        );
    }
    match store.take_for_publish("d1", now) {
        Err(DraftError::AlreadySent) => {}
        other => panic!("sent marker should remain, got {other:?}"),
    }
}
