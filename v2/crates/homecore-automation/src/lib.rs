//! homecore-automation — ADR-129 HOMECORE-AUTO
//!
//! Automation engine, trigger evaluator, MiniJinja template evaluator, and
//! script action executor for the HOMECORE Home Assistant port.
//!
//! ## Layout
//!
//! - [`automation`] — `Automation` struct: id, alias, mode, triggers, conditions, actions
//! - [`trigger`] — `Trigger` enum + `EvaluateTrigger` trait
//! - [`condition`] — `Condition` enum + async `evaluate` method + `EvalContext`
//! - [`action`] — `Action` enum + async `execute` method + `ExecutionContext`
//! - [`template`] — MiniJinja environment with HA-compat globals (states, state_attr, is_state, now)
//! - [`engine`] — `AutomationEngine`: subscribes to event bus, drives trigger→condition→action pipeline
//! - [`error`] — crate-wide `AutomationError`

pub mod action;
pub mod automation;
pub mod condition;
pub mod engine;
pub mod error;
pub mod runmode;
pub mod template;
pub mod trigger;

pub use action::{Action, ExecutionContext};
pub use automation::{Automation, RunMode};
pub use condition::{Condition, EvalContext};
pub use engine::AutomationEngine;
pub use error::AutomationError;
pub use template::TemplateEnvironment;
pub use trigger::{EvaluateTrigger, Trigger, TriggerContext};
