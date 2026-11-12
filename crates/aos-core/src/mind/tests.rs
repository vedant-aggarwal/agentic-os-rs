use serde_json::{Map, json};

use super::jobs::{Fetched, Reading};
use super::links::NewLink;
use super::*;

fn note(st: &Store, raw: &str) -> Value {
    st.mind_save("owner-phone", NewMindItem { raw: raw.into(), origin: Some("capture".into()), ..Default::default() }).unwrap()
}

#[test]
fn guesses_kinds() {
    assert_eq!(guess_kind("https://example.com/a", None, false), "link");
    assert_eq!(guess_kind("https://youtu.be/abc", None, false), "video");
    assert_eq!(guess_kind("https://www.instagram.com/reel/xyz/", None, false), "video");
    assert_eq!(guess_kind("https://example.com/cat.JPG?x=1", None, false), "image");
    assert_eq!(guess_kind("\u{201C}Stay hungry\u{201D}", None, false), "quote");
    assert_eq!(guess_kind("buy milk", None, false), "note");
    assert_eq!(guess_kind("", None, true), "image");
    assert_eq!(domain_of("https://www.Example.com:8080/x?y").as_deref(), Some("example.com"));
}

#[test]
fn save_keeps_raw_and_queues_enrichment() {
    let st = Store::open_temporary().unwrap();
    let n = note(&st, "Deep work\nFour hours, no phone.");
    assert_eq!(n["kind"], "note");
    assert_eq!(n["title"], "Deep work");
    assert_eq!(n["raw"], "Deep work\nFour hours, no phone.");
    assert_eq!(n["inbox"], 1);
    assert_eq!(n["jobs"][0]["stage"], "tag");

    let l = st.mind_save("owner-phone", NewMindItem { raw: "https://example.com/post".into(), ..Default::default() }).unwrap();
    assert_eq!(l["kind"], "link");
    assert_eq!(l["source_domain"], "example.com");
    assert_eq!(l["jobs"][0]["stage"], "fetch");

    // A sensitive note never reaches Claude.
    let s = st.mind_save("owner-phone", NewMindItem { raw: "private".into(), sensitive: true, ..Default::default() }).unwrap();
    assert_eq!(s["status"], "ready");
    assert!(s["jobs"].as_array().unwrap().is_empty());

    // Raw can't be rewritten.
    let mut p = Map::new();
    p.insert("raw".into(), json!("changed"));
    assert!(st.mind_update("nora", n["id"].as_str().unwrap(), &p).is_err());
    assert!(st.mind_save("x", NewMindItem::default()).is_err());
}

#[test]
fn wiki_links_and_backlinks_resolve_both_ways() {
    let st = Store::open_temporary().unwrap();
    // Link to a title that doesn't exist yet.
    let a = note(&st, "Plan for the week, see [[Reading list]] and [[Gym]]");
    let a_id = a["id"].as_str().unwrap();
    assert!(a["links"][0]["to_id"].is_null());
    let b = note(&st, "Reading list\n- one\n- two");
    let b_id = b["id"].as_str().unwrap();
    assert_eq!(b["backlinks"][0]["from_id"], a_id);
    assert!(b["backlinks"][0]["context"].as_str().unwrap().contains("[[Reading list]]"));
    let a = st.mind_get(a_id).unwrap();
    assert_eq!(a["links"][0]["to_id"], b_id);
    assert_eq!(a["links"][0]["label"], "Reading list");
    // Link by id, and twice is one link.
    let c = note(&st, "Gym");
    let e1 = st.mind_link("owner", NewLink { from_id: b_id.into(), to_id: c["id"].as_str().map(String::from), ..Default::default() }).unwrap();
    let e2 = st.mind_link("owner", NewLink { from_id: b_id.into(), to_id: c["id"].as_str().map(String::from), ..Default::default() }).unwrap();
    assert_eq!(e1["id"], e2["id"]);
    let c = st.mind_get(c["id"].as_str().unwrap()).unwrap();
    assert_eq!(c["backlinks"].as_array().unwrap().len(), 2);
}

#[test]
fn search_finds_words_tags_and_claude_reading() {
    let st = Store::open_temporary().unwrap();
    let a = note(&st, "Ideas for the shop window display");
    let b = st.mind_save("p", NewMindItem { raw: "https://example.com/chair".into(), tags: vec!["Furniture".into()], ..Default::default() }).unwrap();
    let b_id = b["id"].as_str().unwrap();
    st.mind_apply_fetch(b_id, &Fetched { title: Some("Oak lounge chair".into()), body_text: Some("A walnut and oak chair.".into()), ..Default::default() }).unwrap();
    st.mind_job_finish(b_id, "fetch", None).unwrap();
    st.mind_apply_reading(b_id, &Reading { description: Some("A low wooden chair in warm light".into()), tags: vec!["chair".into(), "wood".into()], ..Default::default() }, "haiku").unwrap();

    let hits = st.mind_search(&MindQuery { text: "window".into(), ..Default::default() }).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["id"], a["id"]);
    let hits = st.mind_search(&MindQuery { text: "wood".into(), ..Default::default() }).unwrap();
    assert_eq!(hits[0]["id"], b_id);
    let hits = st.mind_search(&MindQuery { text: "#furniture".into(), ..Default::default() }).unwrap();
    assert_eq!(hits.len(), 1);
    let hits = st.mind_search(&MindQuery { text: "walnut".into(), ..Default::default() }).unwrap();
    assert_eq!(hits.len(), 1, "page text is searchable");
    let all = st.mind_search(&MindQuery::default()).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|i| i["tags"].as_array().unwrap().iter().any(|t| t == "furniture")));

    // His title wins over the fetched one; a removed tag never comes back.
    let mut p = Map::new();
    p.insert("tags_remove".into(), json!(["wood"]));
    st.mind_update("owner", b_id, &p).unwrap();
    st.mind_apply_reading(b_id, &Reading { tags: vec!["wood".into()], ..Default::default() }, "haiku").unwrap();
    let got = st.mind_get(b_id).unwrap();
    assert!(!got["tags"].as_array().unwrap().iter().any(|t| t["tag"] == "wood"));
    assert_eq!(got["title"], "Oak lounge chair");
}

#[test]
fn jobs_run_in_order_and_retry() {
    let st = Store::open_temporary().unwrap();
    let l = st.mind_save("p", NewMindItem { raw: "https://example.com/p".into(), ..Default::default() }).unwrap();
    let id = l["id"].as_str().unwrap();
    let (jid, stage) = st.mind_next_job(&["fetch", "thumb"]).unwrap().unwrap();
    assert_eq!((jid.as_str(), stage.as_str()), (id, "fetch"));
    assert!(st.mind_next_job(&["fetch"]).unwrap().is_none(), "running job is not handed out twice");
    st.mind_apply_fetch(id, &Fetched { image_url: Some("https://example.com/i.jpg".into()), ..Default::default() }).unwrap();
    st.mind_job_finish(id, "fetch", None).unwrap();
    let (_, stage) = st.mind_next_job(&["thumb", "tag"]).unwrap().unwrap();
    assert_eq!(stage, "thumb");
    st.mind_job_finish(id, "thumb", Some("timeout")).unwrap();
    // Retry is scheduled later, not now.
    assert!(st.mind_next_job(&["thumb"]).unwrap().is_none());
    let got = st.mind_get(id).unwrap();
    assert_eq!(got["jobs"].as_array().unwrap().iter().find(|j| j["stage"] == "thumb").unwrap()["error"], "timeout");
}

#[test]
fn append_supersede_and_board_reference() {
    let st = Store::open_temporary().unwrap();
    let d = st.mind_save("p", NewMindItem { kind: Some("decision".into()), raw: "Sell the old bike".into(), ..Default::default() }).unwrap();
    let id = d["id"].as_str().unwrap();
    let after = st.mind_append("nora", id, "He said this in the evening call.").unwrap();
    assert_eq!(after["raw"], "Sell the old bike");
    assert_eq!(after["notes"][0]["by"], "nora");
    let newer = st.mind_supersede("owner", id, "Keep the old bike").unwrap();
    assert_eq!(newer["links"][0]["rel"], "supersedes");
    let old = st.mind_get(id).unwrap();
    assert_eq!(old["superseded_by"], newer["id"]);
    assert_eq!(old["raw"], "Sell the old bike");
    // Superseded items leave the grid.
    assert_eq!(st.mind_search(&MindQuery::default()).unwrap().len(), 1);

    let board = st.mc_add_board("owner", crate::mission::boards::NewBoard { title: "Ideas".into(), kind: Some("free".into()), ..Default::default() }).unwrap();
    let card = st.mind_add_to_board("owner", newer["id"].as_str().unwrap(), board["id"].as_str().unwrap()).unwrap();
    assert_eq!(card["mind_item_id"], newer["id"]);
    assert_eq!(card["caption"], "Keep the old bike");
    let got = st.mind_get(newer["id"].as_str().unwrap()).unwrap();
    assert_eq!(got["boards"][0]["title"], "Ideas");
    assert!(got["links"].as_array().unwrap().iter().any(|l| l["to_kind"] == "board"));
}
