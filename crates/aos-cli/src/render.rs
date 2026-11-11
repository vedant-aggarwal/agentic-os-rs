//! Human-readable output. `--json` skips all of this.

use aos_core::goals::format_inr;
use aos_tools::Tool;
use serde_json::Value;

/// Pick a view for a tool's output.
pub fn tool_output(tool: &str, v: &Value) -> String {
    match tool {
        "today" => today(v),
        "task_list" => v["tasks"]
            .as_array()
            .map(|ts| tasks(ts, ""))
            .unwrap_or_default(),
        "habit_list" => v["habits"]
            .as_array()
            .map(|hs| habits(hs))
            .unwrap_or_default(),
        "goal_list" => v["goals"]
            .as_array()
            .map(|gs| goals(gs))
            .unwrap_or_default(),
        "task_add" => format!("added {}", task_line(v)),
        "task_done" | "task_skip" | "task_update" => {
            let xp = v["xp_delta"].as_i64().unwrap_or(0);
            let xp = if xp != 0 {
                format!("  ({xp:+} XP)")
            } else {
                String::new()
            };
            format!("{}{xp}", task_line(&v["task"]))
        }
        _ => generic(v, 0),
    }
}

fn short(id: &Value) -> String {
    id.as_str()
        .map(|s| s.chars().take(8).collect())
        .unwrap_or_default()
}

fn task_line(t: &Value) -> String {
    let mark = match t["status"].as_str() {
        Some("done") => "[x]",
        Some("doing") => "[~]",
        Some("skipped") => "[-]",
        Some("dropped") => "[/]",
        _ => "[ ]",
    };
    let mut extra = Vec::new();
    if let Some(d) = t["due"].as_str() {
        extra.push(format!("due {d}"));
    }
    if let Some(s) = t["scheduled"].as_str() {
        extra.push(format!("at {s}"));
    }
    if let Some(a) = t["area_id"].as_str() {
        extra.push(a.to_string());
    }
    if let Some(r) = t["rollup"].as_object() {
        extra.push(format!(
            "{}/{} parts",
            r["done"],
            r["total"].as_u64().unwrap_or(0) - r["dropped"].as_u64().unwrap_or(0)
        ));
    }
    if let Some(r) = t["skip_reason"].as_str() {
        extra.push(format!("skipped: {r}"));
    }
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!("  ({})", extra.join(", "))
    };
    format!(
        "{mark} {}  {}{extra}",
        short(&t["id"]),
        t["title"].as_str().unwrap_or("")
    )
}

fn tasks(list: &[Value], indent: &str) -> String {
    if list.is_empty() {
        return format!("{indent}(no tasks)");
    }
    let mut out = Vec::new();
    for t in list {
        out.push(format!("{indent}{}", task_line(t)));
        for s in t["subtasks"].as_array().into_iter().flatten() {
            out.push(format!("{indent}    {}", task_line(s)));
        }
    }
    out.join("\n")
}

fn habits(list: &[Value]) -> String {
    list.iter()
        .map(|h| {
            let today =
                h["today"]
                    .as_str()
                    .unwrap_or(if h["due_today"] == true { "-" } else { "off" });
            let s = &h["streak"];
            format!(
                "{:<6} {:<20} streak {} ({}, best {})",
                today,
                h["name"].as_str().unwrap_or(""),
                s["current"],
                s["state"].as_str().unwrap_or(""),
                s["best"]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn goals(list: &[Value]) -> String {
    list.iter()
        .map(|g| {
            let pct = (g["progress"].as_f64().unwrap_or(0.0) * 100.0).round();
            let money = match g["target_paise"].as_i64() {
                Some(t) => format!(
                    "  {} of {}",
                    format_inr(g["earned_paise"].as_i64().unwrap_or(0)),
                    format_inr(t)
                ),
                None => String::new(),
            };
            format!("{:>3}%  {}{money}", pct, g["name"].as_str().unwrap_or(""))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn today(v: &Value) -> String {
    let mut out = Vec::new();
    let s = &v["score"];
    let lvl = &v["xp"]["level"];
    out.push(format!(
        "{}  done {}/{} ({:.0}%)  level {} stage {}  +{} XP today",
        v["date"].as_str().unwrap_or(""),
        s["done"],
        s["counted"],
        s["rate"].as_f64().unwrap_or(0.0) * 100.0,
        lvl["level"],
        lvl["stage"],
        v["xp"]["today"]
    ));
    for area in v["areas"].as_array().into_iter().flatten() {
        out.push(String::new());
        out.push(area["area"].as_str().unwrap_or("No area").to_string());
        out.push(tasks(
            area["tasks"].as_array().map(Vec::as_slice).unwrap_or(&[]),
            "  ",
        ));
    }
    if let Some(hs) = v["habits"].as_array().filter(|h| !h.is_empty()) {
        out.push(String::new());
        out.push("Habits".into());
        out.push(
            habits(hs)
                .lines()
                .map(|l| format!("  {l}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if let Some(gs) = v["goals"].as_array().filter(|g| !g.is_empty()) {
        out.push(String::new());
        out.push("Goals".into());
        out.push(
            goals(gs)
                .lines()
                .map(|l| format!("  {l}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    out.push(String::new());
    let mood = v["moods"]
        .as_array()
        .and_then(|m| m.last())
        .map_or("not logged".to_string(), |m| {
            format!("valence {}, energy {}", m["valence"], m["energy"])
        });
    out.push(format!(
        "Mood: {mood}   Journal: {}",
        if v["journal_written"] == true {
            "written"
        } else {
            "not yet"
        }
    ));
    for a in v["alarms"].as_array().into_iter().flatten() {
        out.push(format!(
            "Alarm {}  {}  ({})",
            local_time(&a["at"]),
            a["label"].as_str().unwrap_or(""),
            short(&a["id"])
        ));
    }
    out.join("\n")
}

/// Plain `key: value` lines; nulls and empty lists are left out.
pub fn generic(v: &Value, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    match v {
        Value::Object(map) => map
            .iter()
            .filter(|(_, v)| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
            .map(|(k, v)| match v {
                Value::Object(_) | Value::Array(_) => {
                    format!("{pad}{k}:\n{}", generic(v, depth + 1))
                }
                _ => format!("{pad}{k}: {}", scalar(v)),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Object(_) => {
                    // Render one level deeper, then turn the first indent into the "- " marker.
                    let inner = generic(item, depth + 1);
                    match inner.strip_prefix(&format!("{pad}  ")) {
                        Some(rest) => format!("{pad}- {rest}"),
                        None => inner,
                    }
                }
                _ => format!("{pad}- {}", scalar(item)),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => format!("{pad}{}", scalar(v)),
    }
}

/// An RFC 3339 timestamp as local `YYYY-MM-DD HH:MM`.
fn local_time(v: &Value) -> String {
    let raw = v.as_str().unwrap_or("");
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| raw.to_string())
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `aos describe <tool>`: usage line and options from the schema.
pub fn describe(t: &Tool) -> String {
    let cmd = t.name.replacen('_', " ", 1);
    let pos: Vec<String> = t.positional.iter().map(|p| format!("[{p}]")).collect();
    let mut out = vec![
        format!("aos {cmd} {}", pos.join(" "))
            .trim_end()
            .to_string(),
        String::new(),
        t.description.to_string(),
        format!("Risk: {}", t.risk.as_str()),
        String::new(),
        "Options:".into(),
    ];
    let props = t.input_schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let required: Vec<&str> = t.input_schema["required"]
        .as_array()
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for (name, p) in &props {
        let ty = t.property_type(name).unwrap_or_else(|| "string".into());
        let req = if required.contains(&name.as_str()) {
            " (required)"
        } else {
            ""
        };
        out.push(format!(
            "  --{:<16} {:<8} {}{req}",
            name.replace('_', "-"),
            ty,
            p["description"].as_str().unwrap_or("")
        ));
    }
    out.join("\n")
}
