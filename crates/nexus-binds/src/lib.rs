// Licence: GPL-3.0-or-later, with additional terms under its section 7: see LICENSE-ADDITIONAL-TERMS.md at the repository root.
//! Elite Dangerous key bindings parser and analyser — a native replacement for EDRefCard.
//!
//! * [`bindings`] — the `.binds` data model and parser ([`Bindings::load`], [`Bindings::parse`]).
//! * [`input`] — inputs, devices and human-readable labels ([`Input::display`] → `LShift + A`,
//!   `X52 Pro Btn 5`, `Wheel Up`, …).
//! * [`presets`] — preset discovery ([`find_presets`]), `StartPreset.4.start`, and merging the
//!   four per-category presets into the bindings the game really uses.
//! * [`catalogue`] — friendly name, context and category for every action ([`action_info`]).
//! * [`analysis`] — conflicts, unbound essentials, modifier pitfalls, per-device counts
//!   ([`analyze`]).
//! * [`html`] — standalone printable reference card ([`render_html`]).
//!
//! ```no_run
//! use nexus_binds::{analyze, default_bindings_dir, find_presets, render_html};
//!
//! let dir = default_bindings_dir().expect("no Options folder");
//! let presets = find_presets(&dir)?;
//! let bindings = presets.primary().expect("no presets").load()?;
//! let report = analyze(&bindings);
//! for c in &report.conflicts {
//!     let names: Vec<&str> = c.actions.iter().map(|a| a.name.as_str()).collect();
//!     println!("{} is bound to {}", c.input, names.join(" and "));
//! }
//! std::fs::write("refcard.html", render_html(&bindings, &report))?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod analysis;
pub mod bindings;
pub mod catalogue;
pub mod error;
pub mod html;
pub mod input;
pub mod presets;
mod util;

pub use analysis::{
    Conflict, ConflictAction, DeviceSummary, Importance, MissingBinding, ModifierNote, Report,
    Requirement, Totals, analyze,
};
pub use bindings::{ActionBinding, ActionKind, AxisBinding, Bindings, NO_DEVICE, Slot, Version};
pub use catalogue::{ActionDef, ActionInfo, Category, Context, Scope, action_info};
pub use error::BindsError;
pub use html::{RenderOptions, render_html, render_html_with};
pub use input::{DeviceKind, Input, device_name, device_short_name, key_label};
pub use presets::{
    PresetCategory, PresetDirectory, PresetFile, StartPreset, default_bindings_dir, find_presets,
    merge_by_category, parse_preset_file_name,
};
pub use util::humanize;
