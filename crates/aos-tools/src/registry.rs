//! The tool registry: every tool is defined once (name, description, input type, risk, handler)
//! and the HTTP API, the `aos` CLI and the MCP server all call it through [`Registry::call`].

use aos_core::Store;
use chrono::{DateTime, Local, NaiveDate, Utc};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// How careful a caller should be before running a tool (ARCHITECTURE.md §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Just do it.
    Free,
    /// Do it and say so.
    Tell,
    /// Only after asking: messaging other people, spending, deleting.
    Ask,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Free => "free",
            Risk::Tell => "tell",
            Risk::Ask => "ask",
        }
    }
}

/// What a tool call runs against: the store, who is calling, and the caller's clock.
pub struct ToolCtx<'a> {
    pub store: &'a Store,
    /// A device id, `cli`, `mcp`... Written to `change_log.actor`.
    pub actor: &'a str,
    pub now: DateTime<Local>,
}

impl<'a> ToolCtx<'a> {
    pub fn new(store: &'a Store, actor: &'a str) -> Self {
        ToolCtx {
            store,
            actor,
            now: Local::now(),
        }
    }

    pub fn today(&self) -> NaiveDate {
        self.now.date_naive()
    }

    pub fn now_utc(&self) -> DateTime<Utc> {
        self.now.with_timezone(&Utc)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("unknown tool '{0}'")]
    UnknownTool(String),
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error(transparent)]
    Core(#[from] aos_core::Error),
}

impl ToolError {
    /// A short machine-readable code for JSON error bodies.
    pub fn code(&self) -> &'static str {
        use aos_core::Error as E;
        match self {
            ToolError::UnknownTool(_) => "unknown_tool",
            ToolError::InvalidArgs(_) | ToolError::Core(E::Invalid(_)) => "invalid_args",
            ToolError::Core(E::NotFound { .. }) => "not_found",
            ToolError::Core(E::Ambiguous { .. }) => "ambiguous",
            ToolError::Core(E::Unauthorized) => "unauthorized",
            ToolError::Core(_) => "internal",
        }
    }

    /// True when the caller can fix it by changing the arguments.
    pub fn is_caller_error(&self) -> bool {
        self.code() != "internal"
    }
}

type Handler = Box<dyn Fn(&ToolCtx<'_>, Value) -> Result<Value, ToolError> + Send + Sync>;

/// One tool. Build it with [`Tool::new`].
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub risk: Risk,
    /// Doesn't change anything.
    pub read_only: bool,
    /// Argument names the CLI fills from bare words, in order. The last one takes the rest of
    /// the words, so `aos task add Buy milk` works without quotes.
    pub positional: &'static [&'static str],
    /// JSON Schema of the input, generated from the Rust input type.
    pub input_schema: Value,
    handler: Handler,
}

impl Tool {
    /// Define a tool from a typed handler. The input type's doc comments become the schema's
    /// descriptions.
    pub fn new<I, O, F>(
        name: &'static str,
        description: &'static str,
        risk: Risk,
        handler: F,
    ) -> Self
    where
        I: DeserializeOwned + JsonSchema,
        O: Serialize,
        F: Fn(&ToolCtx<'_>, I) -> Result<O, ToolError> + Send + Sync + 'static,
    {
        Tool {
            name,
            description,
            risk,
            read_only: false,
            positional: &[],
            input_schema: input_schema::<I>(),
            handler: Box::new(move |ctx, args| {
                let args = if args.is_null() { json!({}) } else { args };
                let input: I = serde_json::from_value(args)
                    .map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
                let out = handler(ctx, input)?;
                serde_json::to_value(out).map_err(|e| ToolError::Core(e.into()))
            }),
        }
    }

    pub fn positional(mut self, names: &'static [&'static str]) -> Self {
        self.positional = names;
        self
    }

    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub fn call(&self, ctx: &ToolCtx<'_>, args: Value) -> Result<Value, ToolError> {
        (self.handler)(ctx, args)
    }

    /// The tool as plain JSON (for `GET /api/tools` and `aos tools --json`).
    pub fn describe(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "risk": self.risk,
            "read_only": self.read_only,
            "input_schema": self.input_schema,
        })
    }

    /// The type of one input property (`string`, `integer`, `array`...), ignoring `null`.
    pub fn property_type(&self, key: &str) -> Option<String> {
        let prop = self.input_schema.get("properties")?.get(key)?;
        match prop.get("type")? {
            Value::String(t) => Some(t.clone()),
            Value::Array(ts) => ts
                .iter()
                .filter_map(Value::as_str)
                .find(|t| *t != "null")
                .map(String::from),
            _ => None,
        }
    }

    pub fn property_names(&self) -> Vec<String> {
        self.input_schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }
}

/// A flat JSON Schema for `I`: subschemas inlined (some agent harnesses don't follow `$ref`),
/// no `$schema` or `title` noise.
fn input_schema<I: JsonSchema>() -> Value {
    let schema = SchemaSettings::draft2020_12()
        .with(|s| s.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<I>();
    let mut value = schema.to_value();
    if let Some(obj) = value.as_object_mut() {
        obj.remove("$schema");
        obj.remove("title");
        obj.entry("properties").or_insert_with(|| json!({}));
        obj.insert("type".into(), json!("object"));
    }
    value
}

/// All tools, looked up by name.
pub struct Registry {
    tools: Vec<Tool>,
}

impl Registry {
    pub fn new(tools: Vec<Tool>) -> Self {
        let mut names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), tools.len(), "tool names must be unique");
        Registry { tools }
    }

    /// Add one tool (from another crate, e.g. `aos-work`). Panics on a duplicate name, which
    /// is a programming mistake caught at startup.
    pub fn register(&mut self, tool: Tool) -> &mut Self {
        assert!(
            self.get(tool.name).is_none(),
            "tool '{}' registered twice",
            tool.name
        );
        self.tools.push(tool);
        self
    }

    /// Add several tools.
    pub fn extend(&mut self, tools: impl IntoIterator<Item = Tool>) -> &mut Self {
        for t in tools {
            self.register(t);
        }
        self
    }

    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// Look up by name. `task.add` and `task-add` also find `task_add`.
    pub fn get(&self, name: &str) -> Option<&Tool> {
        let canonical = name.trim().replace(['.', '-'], "_");
        self.tools.iter().find(|t| t.name == canonical)
    }

    pub fn call(&self, ctx: &ToolCtx<'_>, name: &str, args: Value) -> Result<Value, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::UnknownTool(name.to_string()))?;
        tool.call(ctx, args)
    }
}
