//! Turn command-line words into a tool name and JSON arguments, using the tool's own schema.
//!
//! ```text
//! aos task add Buy milk --due tomorrow --xp-weight 2
//!     tool: task_add   args: {"title": "Buy milk", "due": "tomorrow", "xp_weight": 2}
//! aos habit log journal floor
//!     tool: habit_log  args: {"habit": "journal", "status": "floor"}
//! aos mood log 0.4 --labels calm,proud
//!     tool: mood_log   args: {"valence": 0.4, "labels": ["calm", "proud"]}
//! ```
//!
//! Values are converted by the schema type of each property (integer, number, boolean, array,
//! string). `null` or `none` clears a nullable field. Nothing about a tool is declared twice.

use serde_json::{Map, Value};

use crate::registry::{Registry, Tool, ToolError};

/// Find the tool named by the first one or two words (`task add`, `today`, `task_add`).
/// Returns the tool and how many words its name used.
pub fn resolve<'r>(registry: &'r Registry, words: &[String]) -> Option<(&'r Tool, usize)> {
    if words.len() >= 2
        && !words[1].starts_with('-')
        && let Some(t) = registry.get(&format!("{}_{}", words[0], words[1]))
    {
        return Some((t, 2));
    }
    let first = words.first()?;
    // A bare group means its listing: `aos task` -> task_list, `aos xp` -> xp_summary.
    registry
        .get(first)
        .or_else(|| registry.get(&format!("{first}_list")))
        .or_else(|| registry.get(&format!("{first}_summary")))
        .map(|t| (t, 1))
}

/// Build the JSON arguments for `tool` from the words after its name.
pub fn parse(tool: &Tool, words: &[String]) -> Result<Value, ToolError> {
    let mut args = Map::new();
    let mut bare: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = &words[i];
        if let Some(flag) = w.strip_prefix("--").filter(|f| !f.is_empty()) {
            let (key, inline) = match flag.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (flag, None),
            };
            let key = key.replace('-', "_");
            let ty = tool.property_type(&key).ok_or_else(|| {
                ToolError::InvalidArgs(format!(
                    "{} has no option --{}; options: {}",
                    tool.name,
                    key.replace('_', "-"),
                    tool.property_names()
                        .iter()
                        .map(|n| format!("--{}", n.replace('_', "-")))
                        .collect::<Vec<_>>()
                        .join(" ")
                ))
            })?;
            let raw = match inline {
                Some(v) => v,
                None if ty == "boolean" && words.get(i + 1).is_none_or(|n| n.starts_with("--")) => {
                    "true".into()
                }
                None => {
                    i += 1;
                    words.get(i).cloned().ok_or_else(|| {
                        ToolError::InvalidArgs(format!("--{} needs a value", key.replace('_', "-")))
                    })?
                }
            };
            args.insert(key.clone(), convert(&key, &ty, &raw)?);
        } else {
            bare.push(w.clone());
        }
        i += 1;
    }

    // Bare words fill the positional arguments; the last positional takes the rest.
    let slots = tool.positional;
    if !bare.is_empty() && slots.is_empty() {
        return Err(ToolError::InvalidArgs(format!(
            "{} takes no bare words (got '{}'); use --options",
            tool.name,
            bare.join(" ")
        )));
    }
    for (n, slot) in slots.iter().enumerate() {
        if bare.is_empty() {
            break;
        }
        let raw = if n + 1 == slots.len() {
            std::mem::take(&mut bare).join(" ")
        } else {
            bare.remove(0)
        };
        if args.contains_key(*slot) {
            return Err(ToolError::InvalidArgs(format!("{slot} given twice")));
        }
        let ty = tool.property_type(slot).unwrap_or_else(|| "string".into());
        args.insert(slot.to_string(), convert(slot, &ty, &raw)?);
    }
    Ok(Value::Object(args))
}

fn convert(key: &str, ty: &str, raw: &str) -> Result<Value, ToolError> {
    let bad = |what: &str| ToolError::InvalidArgs(format!("{key}: '{raw}' is not {what}"));
    if matches!(raw, "null" | "none") && ty != "boolean" {
        return Ok(Value::Null);
    }
    Ok(match ty {
        "integer" => Value::from(raw.parse::<i64>().map_err(|_| bad("a whole number"))?),
        "number" => serde_json::Number::from_f64(raw.parse::<f64>().map_err(|_| bad("a number"))?)
            .map(Value::Number)
            .ok_or_else(|| bad("a finite number"))?,
        "boolean" => match raw.to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "1" => Value::Bool(true),
            "false" | "no" | "n" | "0" => Value::Bool(false),
            _ => return Err(bad("true or false")),
        },
        "array" => Value::Array(
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Value::from(s.to_string()))
                .collect(),
        ),
        _ => Value::from(raw.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn run(line: &str) -> (String, Value) {
        let reg = crate::registry();
        let w = words(line);
        let (tool, used) = resolve(&reg, &w).expect("tool");
        (tool.name.to_string(), parse(tool, &w[used..]).unwrap())
    }

    #[test]
    fn task_add_words() {
        let (name, args) = run("task add Buy milk now --due tomorrow --xp-weight 2");
        assert_eq!(name, "task_add");
        assert_eq!(
            args,
            json!({"title": "Buy milk now", "due": "tomorrow", "xp_weight": 2})
        );
    }

    #[test]
    fn several_positionals_and_types() {
        assert_eq!(
            run("habit log journal floor").1,
            json!({"habit": "journal", "status": "floor"})
        );
        assert_eq!(
            run("mood log 0.4 --labels calm,proud").1,
            json!({"valence": 0.4, "labels": ["calm", "proud"]})
        );
        assert_eq!(
            run("task skip ab12 too big --genuine false").1,
            json!({"id": "ab12", "reason": "too big", "genuine": false})
        );
        assert_eq!(run("alarm list --all").1, json!({"all": true}));
        assert_eq!(
            run("task update ab12 --due none").1,
            json!({"id": "ab12", "due": null})
        );
        assert_eq!(run("today").0, "today");
        assert_eq!(run("xp").0, "xp_summary");
        assert_eq!(run("task --due today").0, "task_list");
        assert_eq!(run("task_list --due=today").1, json!({"due": "today"}));
    }

    #[test]
    fn mistakes_are_explained() {
        let reg = crate::registry();
        let tool = reg.get("task_add").unwrap();
        let e = parse(tool, &words("x --colour red")).unwrap_err();
        assert!(e.to_string().contains("--due"));
        let tool = reg.get("xp_summary").unwrap();
        assert!(parse(tool, &words("stray")).is_err());
        let tool = reg.get("task_add").unwrap();
        assert!(parse(tool, &words("x --xp-weight lots")).is_err());
    }
}
