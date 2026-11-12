//! Mission Control tools: boards, blocks, goals, identity, rules, the data drop, evidence and
//! Nora's proposals. Names use underscores (`mission_board_get`); `mission.board.get` works too.
//!
//! Rule for agents (Nora): write evidence links and draft data-drop entries freely. Never change
//! his words, goals, rules or identity directly: draft a `mission_propose`, raise it in the
//! conversation, and call `mission_proposal_decide` only after he says yes (or no) there.

use aos_core::mission::boards::{Move, NewBoard, NewItem, NewNode};
use aos_core::mission::life::{NewDrop, NewEvidence, NewGoal, NewRule, NewStatement};
use aos_core::mission::proposals::NewProposal;
use aos_core::time::parse_day;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::registry::{Risk, Tool, ToolCtx, ToolError};

type R = Result<Value, ToolError>;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new("mission_overview", "Mission pulse (days with a vote this week, steps done, rules kept) and counts of boards, dreams, goals, rules, open drops and pending proposals.", Risk::Free, overview).read_only(),
        Tool::new("mission_context", "Mission Control as short text: dreams with missing path fields, goals and steps with progress, identity statements, rules, open drops, your drafted proposals. Read before the close-the-day talk.", Risk::Free, context).read_only(),
        Tool::new("mission_board_list", "Visualization boards (top level, or the subpages of `parent`), with card counts and cover image.", Risk::Free, board_list).read_only(),
        Tool::new("mission_board_get", "One board with every card (caption, context, image, link, dream path, stage, goal) and the breadcrumb back up.", Risk::Free, board_get).read_only().positional(&["id"]),
        Tool::new("mission_board_add", "Make a board (kind viz by default; give parent_board_id for a nested board).", Risk::Free, board_add).positional(&["title"]),
        Tool::new("mission_board_update", "Rename a board or change its layout (grid, canvas, story), accent, cover, camera, sensitive or archived flag.", Risk::Free, board_update).positional(&["id"]),
        Tool::new("mission_board_item_add", "Place a card on a board: image (asset_id from POST /api/media, or url), text, link, goal (goal_id) or board (a subpage). Dream path fields optional. Nora: only when he asks.", Risk::Free, item_add).positional(&["board_id", "caption"]),
        Tool::new("mission_board_item_update", "Change a card: caption, context, url, the dream path (why, outcome, obstacle, if_then, first_step, stage, closed_reason) or layout (x, y, w, h, z, rot, color, sensitive).", Risk::Free, item_update).positional(&["id"]),
        Tool::new("mission_board_item_move", "Commit card moves and resizes from the canvas in one batch. Nora never moves cards on a Visualization board.", Risk::Free, item_move),
        Tool::new("mission_board_item_remove", "Take a card off its board (its text block stays).", Risk::Tell, item_remove).positional(&["id"]),
        Tool::new("mission_node_list", "Blocks of a space (viz, ver, newme) or the children of a block, in order. `history` adds superseded and deleted ones.", Risk::Free, node_list).read_only(),
        Tool::new("mission_node_add", "Add a text block (or page, link, image) to a space or under a block.", Risk::Free, node_add).positional(&["space", "text"]),
        Tool::new("mission_node_update", "Change a block in place (typo, style, closed, sensitive, context). For a change in meaning of his words use mission_node_supersede.", Risk::Free, node_update).positional(&["id"]),
        Tool::new("mission_node_supersede", "Replace a block with new text: the old one stays, closed, pointing at the new one (the supersede rule).", Risk::Free, node_supersede).positional(&["id", "text"]),
        Tool::new("mission_node_verify", "Stamp today as the day he confirmed something is still true: a block, goal, statement or rule.", Risk::Free, verify).positional(&["kind", "id"]),
        Tool::new("mission_goal_list", "Version I want to be: goals by horizon (short, mid, long) with steps and substeps, progress from real evidence, vote days in the last 14 and latest evidence.", Risk::Free, goal_list).read_only().positional(&["horizon"]),
        Tool::new("mission_goal_add", "Add a goal, or a step (parent_id) or substep. Section: physical, mental, monetary, social, other. Link task_area or habit_id so his tasks and habits count as progress.", Risk::Free, goal_add).positional(&["title"]),
        Tool::new("mission_goal_update", "Change a goal or step: title, why, status (open, moving, done, dropped, parked), target/current/unit, due_on, horizon, section, links. `deleted: true` removes it.", Risk::Free, goal_update).positional(&["id"]),
        Tool::new("mission_identity_list", "Identity statements (\"I am someone who...\") with vote days in the last 14, hard and soft, and latest evidence.", Risk::Free, identity_list).read_only(),
        Tool::new("mission_identity_add", "Add an identity statement under a section, with the old default it replaces (optional).", Risk::Free, identity_add).positional(&["text"]),
        Tool::new("mission_identity_update", "Change a statement's text, section, old_default or status (draft, active, retired).", Risk::Free, identity_update).positional(&["id"]),
        Tool::new("mission_rule_list", "New Me cards: rules (usool), understanding, read-when-in-doubt, tricks; with kept rate over the last 14 checks and an unchecked flag.", Risk::Free, rule_list).read_only().positional(&["kind"]),
        Tool::new("mission_rule_add", "Add a New Me card: kind usool (a rule, if-then when it has a trigger), understanding, in_doubt or trick.", Risk::Free, rule_add).positional(&["text"]),
        Tool::new("mission_rule_update", "Change a New Me card: text, kind, trigger_when, action_then, status (draft, active, retired), check_mode.", Risk::Free, rule_update).positional(&["id"]),
        Tool::new("mission_rule_check", "Record whether a rule was kept today (or on `date`). A second call the same day corrects it.", Risk::Free, rule_check).positional(&["id", "kept"]),
        Tool::new("mission_drop_list", "The data drop: observations, patterns, learnings, experiments, metrics (open first).", Risk::Free, drop_list).read_only().positional(&["status"]),
        Tool::new("mission_drop_add", "Add a data-drop entry. Nora may add drafts from the journal by herself (they show as Nora's read).", Risk::Free, drop_add).positional(&["text"]),
        Tool::new("mission_drop_update", "Change a data-drop entry: text, kind, status (open, confirmed, rejected, became_rule).", Risk::Free, drop_update).positional(&["id"]),
        Tool::new("mission_evidence_add", "Link proof to Mission Control: a done task, habit, journal entry or drop -> a goal/step, statement, rule, dream or drop. Nora writes these by herself.", Risk::Free, evidence_add),
        Tool::new("mission_evidence_list", "Evidence linked to one goal, statement, rule, dream or drop.", Risk::Free, evidence_list).read_only().positional(&["to_kind", "to_id"]),
        Tool::new("mission_propose", "Draft a change to his words, goals, rules or identity (add, edit, close, retire, link). Nothing changes now: raise it in conversation, then record his answer with mission_proposal_decide.", Risk::Free, propose),
        Tool::new("mission_proposal_list", "Proposals by status (pending and raised by default).", Risk::Free, proposal_list).read_only().positional(&["status"]),
        Tool::new("mission_proposal_raise", "Mark a proposal as raised: you asked him about it in this conversation.", Risk::Free, proposal_raise).positional(&["id"]),
        Tool::new("mission_proposal_decide", "Record his answer to a proposal you raised. accept=true applies it (only after he said yes in this conversation); accept=false keeps his reason so it is never proposed again.", Risk::Tell, proposal_decide).positional(&["id", "accept"]),
    ]
}

fn day(ctx: &ToolCtx<'_>, s: &Option<String>) -> Result<chrono::NaiveDate, ToolError> {
    Ok(match s {
        Some(s) => parse_day(s, ctx.today())?,
        None => ctx.today(),
    })
}

/// Serialize tool args and read them back as the core type (same field names).
fn to<T: serde::de::DeserializeOwned>(v: impl Serialize) -> Result<T, ToolError> {
    serde_json::to_value(v).and_then(serde_json::from_value).map_err(|e| ToolError::InvalidArgs(e.to_string()))
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdArg {
    pub id: String,
}

/// An id plus the fields to change (pass null to clear one).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Patch {
    pub id: String,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

fn overview(ctx: &ToolCtx<'_>, _: NoArgs) -> R {
    Ok(ctx.store.mc_overview(ctx.today())?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextArgs {
    /// Max characters (default 6000).
    pub max: Option<usize>,
}

fn context(ctx: &ToolCtx<'_>, a: ContextArgs) -> R {
    Ok(json!({ "text": ctx.store.mc_context(ctx.today(), a.max.unwrap_or(6000).clamp(500, 40_000))? }))
}

// ---------------------------------------------------------------------------------------------
// Boards

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoardListArgs {
    /// Board id: list its nested boards instead of the top level.
    pub parent: Option<String>,
}

fn board_list(ctx: &ToolCtx<'_>, a: BoardListArgs) -> R {
    Ok(json!({ "boards": ctx.store.mc_list_boards(a.parent.as_deref())? }))
}

fn board_get(ctx: &ToolCtx<'_>, a: IdArg) -> R {
    Ok(ctx.store.mc_board(&a.id)?)
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoardAddArgs {
    /// Client-made id (optional; lets an offline device refer to it at once).
    pub id: Option<String>,
    pub title: String,
    /// viz (default), free or project.
    pub kind: Option<String>,
    pub parent_board_id: Option<String>,
    /// Pillar hue: physical, money, mind, me.
    pub accent: Option<String>,
    #[serde(default)]
    pub sensitive: bool,
}

fn board_add(ctx: &ToolCtx<'_>, a: BoardAddArgs) -> R {
    Ok(ctx.store.mc_add_board(ctx.actor, to::<NewBoard>(a)?)?)
}

fn board_update(ctx: &ToolCtx<'_>, a: Patch) -> R {
    Ok(ctx.store.mc_update_board(ctx.actor, &a.id, &a.fields)?)
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemAddArgs {
    pub id: Option<String>,
    pub board_id: String,
    /// image, text, link, goal or board (a subpage).
    #[serde(default = "text_kind")]
    pub kind: String,
    /// The card's words (a subpage's title).
    #[serde(default)]
    pub caption: String,
    /// What it is for, shown under the card.
    pub context: Option<String>,
    pub url: Option<String>,
    pub asset_id: Option<String>,
    pub goal_id: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub w: Option<f64>,
    pub h: Option<f64>,
    pub color: Option<String>,
    #[serde(default)]
    pub sensitive: bool,
    pub why: Option<String>,
    pub outcome: Option<String>,
    pub obstacle: Option<String>,
    pub if_then: Option<String>,
    pub first_step: Option<String>,
}

fn text_kind() -> String {
    "text".into()
}

fn item_add(ctx: &ToolCtx<'_>, a: ItemAddArgs) -> R {
    let mut v = serde_json::to_value(&a).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
    let path = json!({ "why": a.why, "outcome": a.outcome, "obstacle": a.obstacle, "if_then": a.if_then, "first_step": a.first_step });
    if let Some(o) = v.as_object_mut() {
        for k in ["why", "outcome", "obstacle", "if_then", "first_step"] {
            o.remove(k);
        }
        o.insert("path".into(), path);
    }
    Ok(ctx.store.mc_add_item(ctx.actor, to::<NewItem>(v)?)?)
}

fn item_update(ctx: &ToolCtx<'_>, a: Patch) -> R {
    Ok(ctx.store.mc_update_item(ctx.actor, &a.id, &a.fields)?)
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveArg {
    pub id: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub w: Option<f64>,
    pub h: Option<f64>,
    pub z: Option<i64>,
    pub rot: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemMoveArgs {
    pub moves: Vec<MoveArg>,
}

fn item_move(ctx: &ToolCtx<'_>, a: ItemMoveArgs) -> R {
    let moves: Vec<Move> = to(a.moves)?;
    Ok(json!({ "moved": ctx.store.mc_move_items(ctx.actor, &moves)? }))
}

fn item_remove(ctx: &ToolCtx<'_>, a: IdArg) -> R {
    ctx.store.mc_remove_item(ctx.actor, &a.id)?;
    Ok(json!({ "removed": a.id }))
}

// ---------------------------------------------------------------------------------------------
// Blocks

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeListArgs {
    /// viz, ver or newme.
    pub space: Option<String>,
    /// Block id: list its children.
    pub parent: Option<String>,
    #[serde(default)]
    pub history: bool,
}

fn node_list(ctx: &ToolCtx<'_>, a: NodeListArgs) -> R {
    Ok(json!({ "nodes": ctx.store.mc_list_nodes(a.space.as_deref(), a.parent.as_deref(), a.history)? }))
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeAddArgs {
    pub id: Option<String>,
    /// viz, ver or newme.
    pub space: String,
    #[serde(default)]
    pub text: String,
    pub parent_id: Option<String>,
    /// text (default), page, link, image, divider.
    pub kind: Option<String>,
    /// Craft style: h1 h2 h3 h4 body caption bullet numbered card page.
    pub style: Option<String>,
    pub context: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub sensitive: bool,
    /// Never sync this block to Craft.
    #[serde(default)]
    pub local_only: bool,
}

fn node_add(ctx: &ToolCtx<'_>, a: NodeAddArgs) -> R {
    Ok(ctx.store.mc_add_node(ctx.actor, to::<NewNode>(a)?)?)
}

fn node_update(ctx: &ToolCtx<'_>, a: Patch) -> R {
    Ok(ctx.store.mc_update_node(ctx.actor, &a.id, &a.fields)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupersedeArgs {
    pub id: String,
    pub text: String,
}

fn node_supersede(ctx: &ToolCtx<'_>, a: SupersedeArgs) -> R {
    Ok(ctx.store.mc_supersede_node(ctx.actor, &a.id, &a.text)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyArgs {
    /// node, goal, statement or rule.
    pub kind: String,
    pub id: String,
}

fn verify(ctx: &ToolCtx<'_>, a: VerifyArgs) -> R {
    Ok(json!({ "verified_at": ctx.store.mc_verify(ctx.actor, &a.kind, &a.id)? }))
}

// ---------------------------------------------------------------------------------------------
// Goals and identity

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalListArgs {
    /// short, mid or long (default all).
    pub horizon: Option<String>,
}

fn goal_list(ctx: &ToolCtx<'_>, a: GoalListArgs) -> R {
    Ok(json!({ "goals": ctx.store.mc_goal_tree(a.horizon.as_deref(), ctx.today())? }))
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalAddArgs {
    pub id: Option<String>,
    pub title: String,
    /// Parent goal or step: makes this a step or substep.
    pub parent_id: Option<String>,
    /// short (0-3 months, default), mid (3-12), long (1 year+). Steps inherit it.
    pub horizon: Option<String>,
    /// physical (default), mental, monetary, social, other. Steps inherit it.
    pub section: Option<String>,
    pub why: Option<String>,
    /// The dream (block id) this serves.
    pub dream_node_id: Option<String>,
    /// done (default), count, number, streak14.
    pub measure: Option<String>,
    pub target: Option<f64>,
    pub unit: Option<String>,
    pub due_on: Option<String>,
    /// Area id: done tasks there count as progress.
    pub task_area: Option<String>,
    /// Habit id: done/floor logs count as progress.
    pub habit_id: Option<String>,
}

fn goal_add(ctx: &ToolCtx<'_>, a: GoalAddArgs) -> R {
    Ok(ctx.store.mc_add_goal(ctx.actor, to::<NewGoal>(a)?)?)
}

fn goal_update(ctx: &ToolCtx<'_>, a: Patch) -> R {
    Ok(ctx.store.mc_update_goal(ctx.actor, &a.id, &a.fields)?)
}

fn identity_list(ctx: &ToolCtx<'_>, _: NoArgs) -> R {
    Ok(json!({ "statements": ctx.store.mc_statements(ctx.today())? }))
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdentityAddArgs {
    pub id: Option<String>,
    pub text: String,
    /// physical, mental (default), monetary, social, other.
    pub section: Option<String>,
    /// The old default this replaces (the feared self), optional.
    pub old_default: Option<String>,
}

fn identity_add(ctx: &ToolCtx<'_>, a: IdentityAddArgs) -> R {
    Ok(ctx.store.mc_add_statement(ctx.actor, to::<NewStatement>(a)?)?)
}

fn identity_update(ctx: &ToolCtx<'_>, a: Patch) -> R {
    Ok(ctx.store.mc_update_statement(ctx.actor, &a.id, &a.fields)?)
}

// ---------------------------------------------------------------------------------------------
// New Me

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleListArgs {
    /// usool, understanding, in_doubt, trick (default all).
    pub kind: Option<String>,
}

fn rule_list(ctx: &ToolCtx<'_>, a: RuleListArgs) -> R {
    Ok(json!({ "rules": ctx.store.mc_rules(ctx.today(), a.kind.as_deref())? }))
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleAddArgs {
    pub id: Option<String>,
    pub text: String,
    /// usool (default), understanding, in_doubt, trick.
    pub kind: Option<String>,
    /// "When ..." (the trigger).
    pub trigger_when: Option<String>,
    /// "... then I ..." (the action).
    pub action_then: Option<String>,
    /// The data-drop entry this rule came from.
    pub origin_drop_id: Option<String>,
    /// nightly (default for usool), weekly, none.
    pub check_mode: Option<String>,
}

fn rule_add(ctx: &ToolCtx<'_>, a: RuleAddArgs) -> R {
    Ok(ctx.store.mc_add_rule(ctx.actor, to::<NewRule>(a)?)?)
}

fn rule_update(ctx: &ToolCtx<'_>, a: Patch) -> R {
    Ok(ctx.store.mc_update_rule(ctx.actor, &a.id, &a.fields)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleCheckArgs {
    pub id: String,
    pub kept: bool,
    pub date: Option<String>,
    pub note: Option<String>,
}

fn rule_check(ctx: &ToolCtx<'_>, a: RuleCheckArgs) -> R {
    let d = day(ctx, &a.date)?;
    Ok(ctx.store.mc_check_rule(ctx.actor, &a.id, d, a.kept, a.note.as_deref())?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DropListArgs {
    /// open, confirmed, rejected, became_rule (default: all but rejected).
    pub status: Option<String>,
    pub limit: Option<usize>,
}

fn drop_list(ctx: &ToolCtx<'_>, a: DropListArgs) -> R {
    Ok(json!({ "drops": ctx.store.mc_drops(a.status.as_deref(), a.limit.unwrap_or(50).min(500))? }))
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DropAddArgs {
    pub id: Option<String>,
    pub text: String,
    /// observation (default), pattern, learning, experiment, metric.
    pub kind: Option<String>,
    /// Numbers behind it.
    #[serde(default)]
    pub data: Value,
    /// Where it came from: journal days, task ids.
    #[serde(default)]
    pub sources: Value,
}

fn drop_add(ctx: &ToolCtx<'_>, a: DropAddArgs) -> R {
    Ok(ctx.store.mc_add_drop(ctx.actor, to::<NewDrop>(a)?)?)
}

fn drop_update(ctx: &ToolCtx<'_>, a: Patch) -> R {
    Ok(ctx.store.mc_update_drop(ctx.actor, &a.id, &a.fields)?)
}

// ---------------------------------------------------------------------------------------------
// Evidence and proposals

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceArgs {
    /// task, habit, journal, checkin, drop or manual.
    pub from_kind: String,
    pub from_id: String,
    /// goal (also steps), statement, rule, dream or drop.
    pub to_kind: String,
    pub to_id: String,
    /// advances (default), supports, contradicts, explains.
    pub relation: Option<String>,
    pub note: Option<String>,
    pub at: Option<String>,
}

fn evidence_add(ctx: &ToolCtx<'_>, a: EvidenceArgs) -> R {
    Ok(ctx.store.mc_add_evidence(ctx.actor, to::<NewEvidence>(a)?)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceListArgs {
    pub to_kind: String,
    pub to_id: String,
}

fn evidence_list(ctx: &ToolCtx<'_>, a: EvidenceListArgs) -> R {
    Ok(json!({ "evidence": ctx.store.mc_evidence(&a.to_kind, &a.to_id)? }))
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposeArgs {
    /// goal, step, statement, rule, drop, dream, node, card or evidence.
    pub target_kind: String,
    /// The row to change (not for add).
    pub target_id: Option<String>,
    /// add, edit, close, retire or link.
    pub op: String,
    /// The fields: for add, the same fields as the matching *_add tool; for edit, only the changed fields.
    #[serde(default)]
    pub payload: Value,
    /// His words that led to it.
    pub quote: Option<String>,
    /// Why, in one line he would accept.
    pub reason: Option<String>,
    pub journal_id: Option<String>,
}

fn propose(ctx: &ToolCtx<'_>, a: ProposeArgs) -> R {
    Ok(ctx.store.mc_propose(ctx.actor, to::<NewProposal>(a)?)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposalListArgs {
    /// pending, raised, accepted, rejected, expired (default: pending and raised).
    pub status: Option<String>,
}

fn proposal_list(ctx: &ToolCtx<'_>, a: ProposalListArgs) -> R {
    Ok(json!({ "proposals": ctx.store.mc_proposals(a.status.as_deref())? }))
}

fn proposal_raise(ctx: &ToolCtx<'_>, a: IdArg) -> R {
    Ok(ctx.store.mc_raise(ctx.actor, &a.id)?)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecideArgs {
    pub id: String,
    /// true only after he said yes in this conversation.
    pub accept: bool,
    /// His reason when he says no.
    pub reason: Option<String>,
    /// Fields he changed while agreeing, laid over the payload.
    pub edited: Option<Map<String, Value>>,
}

fn proposal_decide(ctx: &ToolCtx<'_>, a: DecideArgs) -> R {
    Ok(ctx.store.mc_decide(ctx.actor, &a.id, a.accept, a.reason.as_deref(), a.edited.as_ref())?)
}

#[cfg(test)]
mod tests {
    use aos_core::Store;
    use serde_json::json;

    use crate::registry::ToolCtx;

    #[test]
    fn mission_tools_round_trip() {
        let store = Store::open_temporary().unwrap();
        let reg = crate::registry();
        let ctx = ToolCtx::new(&store, "app");
        let call = |name: &str, args: serde_json::Value| reg.call(&ctx, name, args).unwrap();
        let b = call("mission.board.add", json!({ "title": "Dreams" }));
        let bid = b["id"].as_str().unwrap();
        let card = call("mission_board_item_add", json!({ "board_id": bid, "caption": "A calm morning", "why": "focus" }));
        let cid = card["id"].as_str().unwrap();
        call("mission_board_item_update", json!({ "id": cid, "obstacle": "phone", "if_then": "if I wake then no phone", "first_step": "charger outside" }));
        call("mission_board_item_move", json!({ "moves": [{ "id": cid, "x": 40.0, "y": 60.0 }] }));
        let view = call("mission_board_get", json!({ "id": bid }));
        assert_eq!(view["items"][0]["stage"], "rooted");
        let g = call("mission_goal_add", json!({ "title": "Fit body", "horizon": "mid" }));
        call("mission_goal_add", json!({ "title": "Gym", "parent_id": g["id"] }));
        assert_eq!(call("mission_goal_list", json!({}))["goals"][0]["steps_total"], 1);
        let r = call("mission_rule_add", json!({ "text": "Phone away at 22:30", "trigger_when": "22:30", "action_then": "phone in the other room" }));
        call("mission_rule_check", json!({ "id": r["id"], "kept": true }));
        let p = call("mission_propose", json!({ "target_kind": "statement", "op": "add", "payload": { "text": "I keep my word" }, "reason": "said it twice" }));
        call("mission_proposal_decide", json!({ "id": p["id"], "accept": true }));
        assert_eq!(call("mission_identity_list", json!({}))["statements"].as_array().unwrap().len(), 1);
        assert!(call("mission_context", json!({}))["text"].as_str().unwrap().contains("Phone away"));
        assert!(reg.call(&ctx, "mission_goal_update", json!({ "id": g["id"], "bogus": 1 })).is_err());
    }
}
