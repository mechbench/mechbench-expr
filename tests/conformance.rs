//! Every case in `conformance/*.json`, through the engine's one entry
//! point, as a host calls it. Values compare exactly, integer against
//! float included: `2` is not `2.0`.

use serde_json::{Value, json};
use std::fs;

fn run(case: &Value) -> Result<(), String> {
    let mut req = json!({});
    for k in ["record", "params", "header"] {
        if let Some(v) = case.get(k) {
            req[k] = v.clone();
        }
    }
    if let Some(src) = case.get("check") {
        req["op"] = json!("check");
        req["expr"] = src.clone();
    } else if let Some(t) = case.get("template") {
        req["op"] = json!("template");
        req["template"] = t.clone();
    } else {
        req["op"] = json!("eval");
        req["expr"] = case["expr"].clone();
    }
    let answer: Value =
        serde_json::from_str(&mbexpr::handle(&req.to_string())).map_err(|e| e.to_string())?;
    if let Some(kind) = case.get("expect_error") {
        if answer["ok"] == json!(false) && answer["error"]["kind"] == *kind {
            return Ok(());
        }
        return Err(format!("expected a {kind} error, got {answer}"));
    }
    if answer["ok"] != json!(true) {
        return Err(format!("failed: {}", answer["error"]));
    }
    if case.get("check").is_some() {
        for k in ["canonical", "reads"] {
            if let Some(want) = case.get(k)
                && answer[k] != *want
            {
                return Err(format!("{k}: expected {want}, got {}", answer[k]));
            }
        }
        return Ok(());
    }
    let got = &answer["values"][0];
    if got != &case["expect"] {
        return Err(format!("expected {}, got {got}", case["expect"]));
    }
    let undefined = answer.get("undefined").cloned().unwrap_or(json!({}));
    let want = case.get("undefined").cloned().unwrap_or(json!({}));
    if undefined != want {
        return Err(format!("undefined: expected {want}, got {undefined}"));
    }
    Ok(())
}

#[test]
fn conformance() {
    let mut files: Vec<_> = fs::read_dir("conformance")
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.sort();
    let mut failures = Vec::new();
    let mut n = 0;
    for f in files {
        let doc: Value = serde_json::from_str(&fs::read_to_string(&f).unwrap()).unwrap();
        for case in doc["cases"].as_array().unwrap() {
            n += 1;
            if let Err(e) = run(case) {
                let what = case
                    .get("expr")
                    .or(case.get("template"))
                    .or(case.get("check"))
                    .cloned()
                    .unwrap_or_default();
                failures.push(format!(
                    "{}: {what}: {e}",
                    f.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {n} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
