//! Every conformance case's raw answer, one per line, in file and case
//! order: the text each host must reproduce byte for byte.

use serde_json::{Value, json};
use std::fs;

pub fn request(case: &Value) -> Value {
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
    req
}

fn main() {
    let mut files: Vec<_> = fs::read_dir("conformance")
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.retain(|f| f.extension().is_some_and(|e| e == "json"));
    files.sort();
    for f in files {
        let doc: Value = serde_json::from_str(&fs::read_to_string(&f).unwrap()).unwrap();
        for case in doc["cases"].as_array().unwrap() {
            println!("{}", mbexpr::handle(&request(case).to_string()));
        }
    }
}
