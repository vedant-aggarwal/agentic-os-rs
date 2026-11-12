//! Mission Control in Nora's day (research/mission-control.md 2.4, decisions of 2026-11-12).
//!
//! The 21:00 close-the-day call and chat read Mission Control and, after the call, write
//! evidence and drafts through the `mission_*` tools (they are in her MCP catalog with every
//! other registry tool). Changes to his words, goals, rules or identity are only proposed:
//! she raises them in conversation and applies them when he agrees there. No morning deck.

use aos_core::Store;

/// Extra step for the close-the-day call script.
pub const CLOSE_DAY_STEP: &str = "6. Mission Control (two minutes, only if there's time): ask one WOOP question (what's in the way, and the if-then plan) for the stalest dream or step listed below, and ask kept or broken for one or two rules. If you drafted a proposal earlier, raise one, in plain words, and wait for his yes or no. Never push.";

/// Line for the chat system prompt.
pub const CHAT_RULE: &str = "- Mission Control (dreams, Version I want to be, New Me): link proof with `mission_evidence_add` and add drafts with `mission_drop_add` freely. Never change his words, goals, rules or identity directly: draft `mission_propose`, raise it with him in the chat (`mission_proposal_raise`), and only after his yes call `mission_proposal_decide` with accept=true (his no: accept=false with his reason).";

/// What the write-up after a close-the-day call should do with Mission Control.
pub fn writeup_extra(close_day: bool) -> &'static str {
    if close_day {
        "\nMission Control: read `mission_context`. For each done task or habit from today that clearly serves a goal, step, identity statement or rule, link it with `mission_evidence_add`. Record rules he said he kept or broke with `mission_rule_check`. Add what you noticed about him as `mission_drop_add` drafts (kind observation or pattern). Anything that would change his words, goals, rules or identity: `mission_propose` with his quote, to raise in the next conversation. Never accept a proposal here."
    } else {
        ""
    }
}

/// Mission Control lines for the call context (short; the full view is `mission_context`).
pub fn context(core: &Store, max: usize) -> String {
    match core.mc_context(aos_core::time::local_today(), max) {
        Ok(s) if !s.trim().is_empty() => format!("Mission Control:\n{s}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writeup_mentions_tools_only_for_close_day() {
        assert!(writeup_extra(true).contains("mission_evidence_add"));
        assert!(writeup_extra(false).is_empty());
        let st = Store::open_temporary().unwrap();
        assert!(context(&st, 2000).contains("Mission pulse"));
    }
}
