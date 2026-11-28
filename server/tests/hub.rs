use tokio::sync::mpsc;
use tvdisplay::hub::{HubEvent, TvHub};

#[tokio::test]
async fn starts_offline_and_comes_online_after_attach() {
    let hub = TvHub::new();
    assert!(!hub.online().await);
    let (tx, _rx) = mpsc::channel(4);
    let (_id, previous) = hub.attach(tx).await;
    assert!(previous.is_none());
    assert!(hub.online().await);
}

#[tokio::test]
async fn replace_returns_the_previous_sender() {
    let hub = TvHub::new();
    let (first_tx, mut first_rx) = mpsc::channel(4);
    let (second_tx, _second_rx) = mpsc::channel(4);
    let (first_id, _) = hub.attach(first_tx).await;
    let (_second_id, previous) = hub.attach(second_tx).await;
    let previous = previous.expect("previous tv");
    previous.send(HubEvent::Close(4000)).await.unwrap();
    match first_rx.recv().await {
        Some(HubEvent::Close(4000)) => {}
        other => panic!("unexpected {other:?}"),
    }
    hub.release(first_id).await;
    assert!(hub.online().await);
}

#[tokio::test]
async fn release_only_clears_if_it_is_still_current() {
    let hub = TvHub::new();
    let (first_tx, _first_rx) = mpsc::channel(4);
    let (second_tx, _second_rx) = mpsc::channel(4);
    let (first_id, _) = hub.attach(first_tx).await;
    let (_second_id, _) = hub.attach(second_tx).await;
    hub.release(first_id).await;
    assert!(hub.online().await);
}

#[tokio::test]
async fn push_delivers_to_the_current_connection() {
    let hub = TvHub::new();
    let (tx, mut rx) = mpsc::channel(4);
    let _ = hub.attach(tx).await;
    assert!(hub.push(r#"{"type":"content","revision":1}"#).await);
    match rx.recv().await {
        Some(HubEvent::Push(body)) => assert!(body.contains("revision")),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn push_without_a_tv_returns_false() {
    let hub = TvHub::new();
    assert!(!hub.push("{}").await);
}
