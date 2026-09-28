//! The batch operations a host calls: several expressions per record, a
//! filter over a collection, fuel, and errors that name their record.

use serde_json::{Value, json};

fn call(req: Value) -> Value {
    serde_json::from_str(&mbexpr::handle(&req.to_string())).unwrap()
}

#[test]
fn several_fields_per_record() {
    let a = call(json!({
        "op": "eval",
        "exprs": {"lied": "top[0].token.text != tracked.truth.token", "layer": "layer"},
        "records": [
            {"layer": 34, "top": [{"token": {"text": " Rome"}}], "tracked": {"truth": {"token": " Paris"}}},
            {"layer": 12, "top": [{"token": {"text": " Paris"}}], "tracked": {"truth": {"token": " Paris"}}},
        ],
    }));
    assert_eq!(
        a["values"],
        json!([{"lied": true, "layer": 34}, {"lied": false, "layer": 12}])
    );
}

#[test]
fn a_filter_keeps_true_and_counts_null() {
    let a = call(json!({
        "op": "filter",
        "expr": "layer == 34 and lied",
        "records": [{"layer": 34, "lied": true}, {"layer": 34, "lied": null}, {"layer": 12, "lied": true}, {"layer": 34, "lied": false}],
    }));
    assert_eq!(a["kept"], json!([0]));
    assert_eq!(a["unknown"], json!(1));
}

#[test]
fn a_filter_refuses_a_non_condition_and_names_the_record() {
    let a =
        call(json!({"op": "filter", "expr": "theme", "records": [{"theme": true}, {"theme": 1}]}));
    assert_eq!(a["ok"], json!(false));
    assert_eq!(a["error"]["kind"], json!("type"));
    assert_eq!(a["error"]["record"], json!(1));
}

#[test]
fn undefined_numbers_are_counted_across_a_batch() {
    let a = call(
        json!({"op": "eval", "expr": "k / n", "records": [{"k": 1, "n": 2}, {"k": 1, "n": 0}, {"k": 3, "n": 0}]}),
    );
    assert_eq!(a["values"], json!([0.5, null, null]));
    assert_eq!(a["undefined"], json!({"division by zero": 2}));
}

#[test]
fn fuel_stops_a_runaway_and_says_so() {
    let big: Vec<i64> = (0..10_000).collect();
    let a = call(
        json!({"op": "eval", "expr": "sum([x * 2 for x in xs])", "record": {"xs": big}, "fuel": 1000}),
    );
    assert_eq!(a["error"]["kind"], json!("limit"));
}

#[test]
fn deep_nesting_is_a_limit_not_a_crash() {
    let src = format!("{}1{}", "(".repeat(500), ")".repeat(500));
    let a = call(json!({"op": "check", "expr": src}));
    assert_eq!(a["error"]["kind"], json!("limit"));
}

#[test]
fn a_long_chain_is_a_limit_not_a_crash() {
    let src = vec!["1"; 5000].join(" + ");
    let a = call(json!({"op": "eval", "expr": src}));
    assert_eq!(a["error"]["kind"], json!("limit"));
}

#[test]
fn a_bad_request_is_answered() {
    let a: Value = serde_json::from_str(&mbexpr::handle("not json")).unwrap();
    assert_eq!(a["error"]["kind"], json!("request"));
}
