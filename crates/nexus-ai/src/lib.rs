//! The built-in assistant ("Ask"): a small tool-calling agent over the commander's own data.
//!
//! * [`provider`]: which model service answers: Google Gemini (default, has a free tier), Anthropic
//!   Claude, a local model (Ollama / LM Studio) or any OpenAI-compatible service.
//! * [`agent::ask`]: question → model → tool calls (run by the app: markets, journal, routes, …)
//!   → answer. Synchronous; run it on a worker thread with a tokio [`tokio::runtime::Handle`].
//! * [`knowledge`]: game knowledge and the app's user guide, compiled in and searched locally.

mod anthropic;
pub mod agent;
pub mod chat;
pub mod knowledge;
mod openai;
pub mod provider;

pub use agent::{Answer, Backend, Remote, Step, ToolHost, ask, trim_history};
pub use chat::{Reply, ToolCall, ToolSpec, Turn, Usage};
pub use provider::{AiError, ProviderConfig, ProviderKind, list_models, resolve_model};

/// Instructions for the model. Static, so providers can cache it; the commander's situation is
/// added to each question instead.
pub const SYSTEM_PROMPT: &str = "You are Nexus, the assistant built into Elite Nexus, a companion app for the game Elite Dangerous (Odyssey). \
You help the commander play: find money-making opportunities, where to buy and sell, routes, engineering, exobiology, mining, game mechanics and how to use this app.

How to work:
- Use the tools for anything about the live galaxy or the commander: prices, stations, distances, routes, planets (biology, first footfall), cargo, ship, credits, journal history. Never invent prices, stations, systems or distances. If a tool finds nothing, say so plainly and suggest how to get data (the live EDDN feed fills the market database over time; Market > Database & feed has a one-click Spansh import of every station).
- Use the knowledge tool for game mechanics, strategies and the app's own features before answering from memory; the game changes between updates.
- For money questions, compare several options with tools (price deals, selling the cargo hold, trade routes, mining hotspots, exobiology or road-to-riches routes) and recommend the best for the commander's ship, cargo space and location.
- To buy several goods (a mission's, say), plan one trip with shopping_trip instead of looking each good up near the commander: the next seller should be near the last stop.
- Market data has an age: mention it when it is more than a few hours old, and note that fleet-carrier prices change quickly. Footfall has an age too: say \"not footfalled as of 3 days ago\", never just \"unfootfalled\".

How to answer:
- Short and concrete: lead with the recommendation (system, station, price, distance, expected credits), then at most a few bullet points. Under 150 words unless asked for more.
- Credits as 12.4M Cr or 1.2B Cr; distances in ly; landing pad sizes S/M/L.
- Plain text with simple Markdown (bold, bullet lists). No tables, no raw JSON.
- If the question is unclear, make a sensible assumption and say what you assumed.";

/// One line of context about the commander, prepended to each question.
pub fn with_context(context: &str, question: &str) -> String {
    if context.trim().is_empty() {
        question.to_string()
    } else {
        format!("[Context: {}]\n{}", context.trim(), question)
    }
}
