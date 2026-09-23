use super::*;
use uuid::Uuid;

fn change(table: &str, kind: &str, record: Value) -> String {
    json!({
        "topic": TOPIC,
        "event": "postgres_changes",
        "ref": null,
        "payload": { "ids": [1], "data": { "schema": "public", "table": table, "type": kind, "record": record, "old_record": {}, "errors": null } },
    })
    .to_string()
}

#[test]
fn task_changes_ask_for_a_pull() {
    assert_eq!(parse(&change("tasks", "UPDATE", json!({ "id": Uuid::nil() }))), Incoming::TaskChanged);
    assert_eq!(parse(&change("tasks", "DELETE", json!({}))), Incoming::TaskChanged);
}

#[test]
fn new_comments_are_read() {
    let record = json!({
        "id": Uuid::from_u128(1), "task_id": Uuid::from_u128(2), "user_id": Uuid::from_u128(3),
        "body": "Bakıyorum", "created_at": "2026-09-23T10:00:00.123456+00:00",
    });
    match parse(&change("task_comments", "INSERT", record)) {
        Incoming::Comment(c) => assert_eq!((c.task_id, c.body.as_str()), (Uuid::from_u128(2), "Bakıyorum")),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(parse(&change("task_comments", "INSERT", json!({ "id": 5 }))), Incoming::Ignore, "malformed record");
}

#[test]
fn channel_errors_rejoin_and_the_rest_is_ignored() {
    let reply = |status: &str| json!({ "topic": TOPIC, "event": "phx_reply", "ref": "1", "payload": { "status": status, "response": {} } }).to_string();
    assert!(matches!(parse(&reply("error")), Incoming::Rejoin(_)));
    assert_eq!(parse(&reply("ok")), Incoming::Ignore);
    let system = json!({ "topic": TOPIC, "event": "system", "payload": { "status": "error", "message": "token expired" } }).to_string();
    assert!(matches!(parse(&system), Incoming::Rejoin(_)));
    let closed = json!({ "topic": TOPIC, "event": "phx_close", "payload": {} }).to_string();
    assert!(matches!(parse(&closed), Incoming::Rejoin(_)));
    let heartbeat = json!({ "topic": "phoenix", "event": "phx_reply", "payload": { "status": "ok" } }).to_string();
    assert_eq!(parse(&heartbeat), Incoming::Ignore);
    assert_eq!(parse("not json"), Incoming::Ignore);
}

#[test]
fn join_subscribes_with_the_users_token() {
    let message = join("jwt");
    assert_eq!(message["event"], "phx_join");
    assert_eq!(message["payload"]["access_token"], "jwt");
    assert_eq!(message["payload"]["config"]["postgres_changes"].as_array().unwrap().len(), 2);
}
