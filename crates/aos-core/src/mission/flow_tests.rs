use serde_json::{Map, Value, json};

use super::boards::{Move, NewBoard, NewItem};
use super::life::{NewEvidence, NewGoal, NewRule, NewStatement};
use super::proposals::NewProposal;
use crate::Store;
use crate::tasks::NewTask;
use crate::time::local_today;

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

#[test]
fn board_cards_dreams_and_subpages() {
    let st = Store::open_temporary().unwrap();
    let b = st.mc_add_board("app", NewBoard { title: "Dreams".into(), ..Default::default() }).unwrap();
    let bid = b["id"].as_str().unwrap().to_string();
    assert!(b["node_id"].is_string(), "viz board gets a page block");
    let card = st
        .mc_add_item("app", NewItem { board_id: bid.clone(), kind: "text".into(), caption: "Run a marathon".into(), ..Default::default() })
        .unwrap();
    assert_eq!(card["stage"], "seed");
    let cid = card["id"].as_str().unwrap().to_string();
    let card = st
        .mc_update_item(
            "app",
            &cid,
            &obj(json!({ "why": "energy", "obstacle": "late nights", "if_then": "if 22:30 then phone away", "first_step": "buy shoes", "context": "for next year" })),
        )
        .unwrap();
    assert_eq!(card["stage"], "rooted");
    assert_eq!(card["context"], "for next year");
    // A subpage card opens a nested board with a breadcrumb back.
    let sub = st.mc_add_item("app", NewItem { board_id: bid.clone(), kind: "board".into(), caption: "Travel".into(), ..Default::default() }).unwrap();
    let child = sub["ref_id"].as_str().unwrap();
    assert_eq!(st.mc_board(child).unwrap()["breadcrumb"][0]["title"], "Dreams");
    let mv = Move { id: cid.clone(), x: Some(500.0), y: Some(80.0), w: None, h: None, z: Some(9), rot: None };
    assert_eq!(st.mc_move_items("app", &[mv]).unwrap(), 1);
    assert_eq!(st.mc_item(&cid).unwrap()["x"], 500.0);
    assert_eq!(st.mc_board(&bid).unwrap()["items"].as_array().unwrap().len(), 2);
    assert_eq!(st.mc_list_boards(None).unwrap()[0]["item_count"], 2);
    // Supersede keeps the old text and moves the card to the new block.
    let nid = card["node_id"].as_str().unwrap().to_string();
    let newer = st.mc_supersede_node("nora", &nid, "Run a half marathon").unwrap();
    assert_eq!(newer["origin"], "nora");
    assert_eq!(st.mc_node(&nid).unwrap()["superseded_by"], newer["id"]);
    assert_eq!(st.mc_item(&cid).unwrap()["caption"], "Run a half marathon");
    assert_eq!(st.mc_item(&cid).unwrap()["stage"], "rooted", "the path follows the new block");
    st.mc_remove_item("app", &cid).unwrap();
    assert_eq!(st.mc_board(&bid).unwrap()["items"].as_array().unwrap().len(), 1);
    assert!(st.mc_verify("app", "node", &nid).is_ok());
}

#[test]
fn goals_progress_from_tasks_and_rules() {
    let st = Store::open_temporary().unwrap();
    let today = local_today();
    let g = st.mc_add_goal("app", NewGoal { title: "Fit body".into(), horizon: Some("mid".into()), ..Default::default() }).unwrap();
    let gid = g["id"].as_str().unwrap();
    let s1 = st.mc_add_goal("app", NewGoal { title: "Gym 3x a week".into(), parent_id: Some(gid.into()), ..Default::default() }).unwrap();
    let s2 = st.mc_add_goal("app", NewGoal { title: "Sleep by 23:00".into(), parent_id: Some(gid.into()), ..Default::default() }).unwrap();
    assert_eq!(s1["horizon"], "mid", "steps inherit the horizon");
    let task = st.add_task("app", NewTask { title: "Gym".into(), ..Default::default() }).unwrap();
    st.complete_task("app", &task.id, None, today).unwrap();
    let ev = NewEvidence { from_kind: "task".into(), from_id: task.id.clone(), to_kind: "goal".into(), to_id: s1["id"].as_str().unwrap().into(), ..Default::default() };
    assert_eq!(st.mc_add_evidence("nora", ev.clone()).unwrap()["linked"], true);
    assert_eq!(st.mc_add_evidence("nora", ev).unwrap()["linked"], false, "linking twice is a no-op");
    st.mc_update_goal("app", s2["id"].as_str().unwrap(), &obj(json!({ "status": "done" }))).unwrap();
    let tree = st.mc_goal_tree(None, today).unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0]["votes_14d"], 1);
    assert!((tree[0]["progress"].as_f64().unwrap() - 0.5).abs() < 1e-9);
    assert_eq!(tree[0]["children"][0]["last_evidence"][0]["from_kind"], "task");

    let r = st.mc_add_rule("app", NewRule { text: "Never call more than 3 times in a row".into(), ..Default::default() }).unwrap();
    let rid = r["id"].as_str().unwrap();
    st.mc_check_rule("app", rid, today, true, None).unwrap();
    st.mc_check_rule("app", rid, today - chrono::Duration::days(1), false, None).unwrap();
    assert_eq!(st.mc_rules(today, None).unwrap()[0]["kept_rate"], 0.5);
    st.mc_add_statement("app", NewStatement { text: "I train whatever the mood".into(), section: Some("physical".into()), ..Default::default() }).unwrap();
    let ctx = st.mc_context(today, 4000).unwrap();
    assert!(ctx.contains("Fit body") && ctx.contains("Never call"), "{ctx}");
    let o = st.mc_overview(today).unwrap();
    assert_eq!(o["vote_days_7"], 1);
    assert_eq!(o["steps_done_7"], 1);
}

#[test]
fn proposals_apply_only_on_yes_and_supersede_owner_text() {
    let st = Store::open_temporary().unwrap();
    let today = local_today();
    let r = st.mc_add_rule("app", NewRule { text: "Sleep at 11".into(), ..Default::default() }).unwrap();
    let rid = r["id"].as_str().unwrap().to_string();
    let p = st
        .mc_propose(
            "nora",
            NewProposal {
                target_kind: "rule".into(),
                target_id: Some(rid.clone()),
                op: "edit".into(),
                payload: json!({ "text": "Phone out of the room at 22:30" }),
                reason: Some("missed sleep 3 times".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(p["status"], "pending");
    assert_eq!(st.mc_rules(today, None).unwrap()[0]["text"], "Sleep at 11", "nothing changes before his yes");
    st.mc_raise("nora", p["id"].as_str().unwrap()).unwrap();
    assert_eq!(st.mc_decide("nora", p["id"].as_str().unwrap(), true, None, None).unwrap()["status"], "accepted");
    let rules = st.mc_rules(today, None).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["text"], "Phone out of the room at 22:30");
    assert_ne!(rules[0]["id"], rid, "owner text is superseded, not overwritten");

    let add = st.mc_propose("nora", NewProposal { target_kind: "goal".into(), op: "add".into(), payload: json!({ "title": "Read 10 pages" }), ..Default::default() }).unwrap();
    st.mc_decide("app", add["id"].as_str().unwrap(), false, Some("not now"), None).unwrap();
    let again = st.mc_propose("nora", NewProposal { target_kind: "goal".into(), op: "add".into(), payload: json!({ "title": "Read 10 pages" }), ..Default::default() });
    assert!(again.is_err(), "a refused change is not proposed again");
    assert!(st.mc_goal_tree(None, today).unwrap().is_empty());
}

#[test]
fn assets_dedupe_by_hash() {
    let dir = std::env::temp_dir().join(format!("aos-media-{}", crate::ids::new_id()));
    // SAFETY: tests in this binary that read AOS_MEDIA run in this test only.
    unsafe { std::env::set_var("AOS_MEDIA", &dir) };
    let st = Store::open_temporary().unwrap();
    let a = st.mc_add_asset("app", b"\x89PNGfake", "image/png", None).unwrap();
    let b = st.mc_add_asset("app", b"\x89PNGfake", "image/png", None).unwrap();
    assert_eq!(a["id"], b["id"]);
    let (path, mime) = st.mc_asset_file(a["id"].as_str().unwrap()).unwrap();
    assert!(path.exists() && mime == "image/png");
    assert!(st.mc_add_asset("app", b"x", "text/html", None).is_err());
    let _ = std::fs::remove_dir_all(dir);
}
