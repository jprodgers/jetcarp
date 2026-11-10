//! Operations: typed input in, typed output out, the same for every front end.
//!
//! An operation is a type implementing [`Op`] (or [`WorldOp`] when it acts on a world). Its
//! input deserializes from JSON and its output serializes to JSON, both with a JSON Schema, and
//! the output also renders as text for people ([`Report`]). Front ends call operations directly
//! (`Paths::run(&cx, NoInput {}, &Quiet)`), or by name through the registry ([`crate::ops::OPS`],
//! [`call_json`]) when the name comes from outside: `jetcarp api call`, tests, and later the MCP
//! tools and the Rhai `jetcarp(op, #{...})` function.

use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::world::Host;
use schemars::JsonSchema;
use schemars::generate::{Contract, SchemaSettings};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// An output as people read it: what the command line prints, and what most MCP tools return.
pub trait Report {
    /// The text, ending with a newline when it is more than a fragment.
    fn text(&self) -> String;
    /// A check the output itself failed, reported after the output is shown (`jetcarp run
    /// --expect HASH` prints the run, then fails on a different hash).
    fn check(&self) -> Result<(), ApiError> {
        Ok(())
    }
}

/// Progress of a long operation, and whether its caller wants it to stop.
pub trait Progress: Sync {
    /// `done` of `total` (if known), with a line for people.
    fn report(&self, _done: u64, _total: Option<u64>, _msg: &str) {}
    /// The caller cancelled: stop soon and return what there is.
    fn cancelled(&self) -> bool {
        false
    }
}

/// No progress reports, never cancelled.
pub struct Quiet;
impl Progress for Quiet {}

/// Progress callbacks and a cancel flag (the MCP server's `notifications/progress` and
/// `notifications/cancelled`).
#[derive(Clone, Default)]
pub struct Callbacks {
    pub on_report: Option<Arc<dyn Fn(u64, Option<u64>, &str) + Send + Sync>>,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Progress for Callbacks {
    fn report(&self, done: u64, total: Option<u64>, msg: &str) {
        if let Some(f) = &self.on_report {
            f(done, total, msg);
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    }
}

/// No input.
#[derive(Clone, Debug, Default, serde::Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoInput {}

/// An operation that does not touch a world.
pub trait Op: 'static {
    /// `group.name`, e.g. `"reference.rules"`.
    const NAME: &'static str;
    /// One line saying what it does.
    const SUMMARY: &'static str;
    /// It changes nothing (files, the catalog, the world).
    const READ_ONLY: bool;
    type In: DeserializeOwned + JsonSchema;
    type Out: Serialize + JsonSchema + Report;
    fn run(cx: &Ctx, input: Self::In, p: &dyn Progress) -> Result<Self::Out, ApiError>;
}

/// An operation on a world: the app's (its [`Host`] puts every change through the app's command
/// path) or a headless [`crate::world::Session`].
pub trait WorldOp: 'static {
    const NAME: &'static str;
    const SUMMARY: &'static str;
    const READ_ONLY: bool;
    type In: DeserializeOwned + JsonSchema;
    type Out: Serialize + JsonSchema + Report;
    fn run(
        cx: &Ctx,
        host: &mut dyn Host,
        input: Self::In,
        p: &dyn Progress,
    ) -> Result<Self::Out, ApiError>;
}

/// What a call by name returns: the output as JSON and as text, and its own check.
pub struct Called {
    pub value: Value,
    pub text: String,
    pub check: Result<(), ApiError>,
}

/// An operation behind the registry.
pub trait DynOp: Sync {
    fn name(&self) -> &'static str;
    fn summary(&self) -> &'static str;
    fn read_only(&self) -> bool;
    /// It acts on a world.
    fn on_world(&self) -> bool;
    /// JSON Schema of the input.
    fn input_schema(&self) -> Value;
    /// JSON Schema of the output.
    fn output_schema(&self) -> Value;
    /// Run it on JSON input (`null` counts as `{}`). World operations act on `host`, or on the
    /// context's own session when there is none.
    fn call(
        &self,
        cx: &Ctx,
        host: Option<&mut dyn Host>,
        input: Value,
        p: &dyn Progress,
    ) -> Result<Called, ApiError>;
}

/// The schema settings: draft 7, subschemas inlined (no `$ref`), no `$schema` line.
pub fn schema_settings(contract: Contract) -> SchemaSettings {
    SchemaSettings::draft07().with(|s| {
        s.inline_subschemas = true;
        s.meta_schema = None;
        s.contract = contract;
    })
}

/// Make a generated schema simpler for MCP clients that read schemas strictly (LM Studio has
/// rejected `anyOf` with `null` and unknown `format`s): an optional value is just its type (it is
/// left out of `required` anyway), and integers lose schemars' `"format": "uint64"` and the like.
pub fn simplify_schema(v: &mut Value) {
    match v {
        Value::Object(o) => {
            if let Some(Value::Array(types)) = o.get_mut("type") {
                types.retain(|t| t != "null");
                if types.len() == 1 {
                    let t = types.remove(0);
                    o.insert("type".into(), t);
                }
            }
            if let Some(Value::Array(alts)) = o.get("anyOf") {
                let keep: Vec<Value> = alts
                    .iter()
                    .filter(|a| a.get("type").is_none_or(|t| t != "null"))
                    .cloned()
                    .collect();
                if keep.len() == 1 && keep.len() < alts.len() {
                    o.remove("anyOf");
                    if let Value::Object(inner) = &keep[0] {
                        for (k, val) in inner {
                            o.entry(k.clone()).or_insert_with(|| val.clone());
                        }
                    }
                } else if keep.len() < alts.len() {
                    o.insert("anyOf".into(), Value::Array(keep));
                }
            }
            if o.get("type")
                .is_some_and(|t| t == "integer" || t == "number")
            {
                o.remove("format");
            }
            if o.get("const").is_some_and(Value::is_null) {
                o.remove("const");
            }
            // `#[serde(default)]` on an optional field says nothing a client needs.
            if o.get("default").is_some_and(Value::is_null) {
                o.remove("default");
            }
            // Doc comments wrap at 100 columns; a description is one paragraph per line.
            if let Some(Value::String(d)) = o.get_mut("description") {
                *d = d
                    .split("\n\n")
                    .map(|p| p.split('\n').collect::<Vec<_>>().join(" "))
                    .collect::<Vec<_>>()
                    .join("\n\n");
            }
            for (_, val) in o.iter_mut() {
                simplify_schema(val);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(simplify_schema),
        _ => {}
    }
}

fn schema_for<T: JsonSchema>(contract: Contract) -> Value {
    let s = schema_settings(contract)
        .into_generator()
        .into_root_schema_for::<T>();
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    simplify_schema(&mut v);
    v
}

/// The JSON Schema of an input type.
pub fn input_schema<T: JsonSchema>() -> Value {
    schema_for::<T>(Contract::Deserialize)
}

/// The JSON Schema of an output type.
pub fn output_schema<T: JsonSchema>() -> Value {
    schema_for::<T>(Contract::Serialize)
}

/// Read an input from JSON; `null` counts as `{}`.
pub fn read_input<T: DeserializeOwned>(name: &str, input: Value) -> Result<T, ApiError> {
    let input = if input.is_null() {
        Value::Object(Default::default())
    } else {
        input
    };
    serde_json::from_value(input).map_err(|e| ApiError::invalid(format!("{name}: {e}")))
}

/// An output as JSON (object keys sorted, as `serde_json::json!` sorts them).
pub fn to_value<T: Serialize>(out: &T) -> Value {
    serde_json::to_value(out).unwrap_or(Value::Null)
}

/// An output as pretty-printed JSON.
pub fn to_pretty<T: Serialize>(out: &T) -> String {
    serde_json::to_string_pretty(&to_value(out)).unwrap_or_default()
}

fn called<T: Serialize + Report>(out: T) -> Called {
    Called {
        value: to_value(&out),
        text: out.text(),
        check: out.check(),
    }
}

/// [`Op`] in the registry.
pub struct Plain<O>(PhantomData<fn() -> O>);
impl<O> Plain<O> {
    pub const NEW: Plain<O> = Plain(PhantomData);
}

/// [`WorldOp`] in the registry.
pub struct OnWorld<O>(PhantomData<fn() -> O>);
impl<O> OnWorld<O> {
    pub const NEW: OnWorld<O> = OnWorld(PhantomData);
}

impl<O: Op> DynOp for Plain<O> {
    fn name(&self) -> &'static str {
        O::NAME
    }
    fn summary(&self) -> &'static str {
        O::SUMMARY
    }
    fn read_only(&self) -> bool {
        O::READ_ONLY
    }
    fn on_world(&self) -> bool {
        false
    }
    fn input_schema(&self) -> Value {
        input_schema::<O::In>()
    }
    fn output_schema(&self) -> Value {
        output_schema::<O::Out>()
    }
    fn call(
        &self,
        cx: &Ctx,
        _host: Option<&mut dyn Host>,
        input: Value,
        p: &dyn Progress,
    ) -> Result<Called, ApiError> {
        let input = read_input(O::NAME, input)?;
        O::run(cx, input, p).map(called)
    }
}

impl<O: WorldOp> DynOp for OnWorld<O> {
    fn name(&self) -> &'static str {
        O::NAME
    }
    fn summary(&self) -> &'static str {
        O::SUMMARY
    }
    fn read_only(&self) -> bool {
        O::READ_ONLY
    }
    fn on_world(&self) -> bool {
        true
    }
    fn input_schema(&self) -> Value {
        input_schema::<O::In>()
    }
    fn output_schema(&self) -> Value {
        output_schema::<O::Out>()
    }
    fn call(
        &self,
        cx: &Ctx,
        host: Option<&mut dyn Host>,
        input: Value,
        p: &dyn Progress,
    ) -> Result<Called, ApiError> {
        let input = read_input(O::NAME, input)?;
        match host {
            Some(h) => O::run(cx, h, input, p).map(called),
            None => {
                let mut s = cx.session();
                let h = s.as_mut().expect("session");
                O::run(cx, h, input, p).map(called)
            }
        }
    }
}

/// The operation called `name`.
pub fn find(name: &str) -> Option<&'static dyn DynOp> {
    crate::ops::OPS.iter().copied().find(|o| o.name() == name)
}

fn unknown(name: &str) -> ApiError {
    let names: Vec<&str> = crate::ops::OPS.iter().map(|o| o.name()).collect();
    ApiError::not_found(format!("no operation {name}"))
        .hint(format!("operations: {}", names.join(", ")))
}

/// Call an operation by name; world operations act on the context's own session.
pub fn call(cx: &Ctx, name: &str, input: Value, p: &dyn Progress) -> Result<Called, ApiError> {
    find(name)
        .ok_or_else(|| unknown(name))?
        .call(cx, None, input, p)
}

/// Call an operation by name on `host`'s world.
pub fn call_on(
    cx: &Ctx,
    host: &mut dyn Host,
    name: &str,
    input: Value,
    p: &dyn Progress,
) -> Result<Called, ApiError> {
    find(name)
        .ok_or_else(|| unknown(name))?
        .call(cx, Some(host), input, p)
}

/// Call an operation by name with JSON in and out; a failed output check is an error.
pub fn call_json(cx: &Ctx, name: &str, input: Value) -> Result<Value, ApiError> {
    let c = call(cx, name, input, &Quiet)?;
    c.check.map(|()| c.value)
}

/// [`call_json`] on `host`'s world.
pub fn call_json_on(
    cx: &Ctx,
    host: &mut dyn Host,
    name: &str,
    input: Value,
) -> Result<Value, ApiError> {
    let c = call_on(cx, host, name, input, &Quiet)?;
    c.check.map(|()| c.value)
}
