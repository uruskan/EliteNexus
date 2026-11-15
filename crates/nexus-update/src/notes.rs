//! "What's new": the changelog's sections for the versions between two releases, in blocks the app can draw.
//!
//! CHANGELOG.md is hard-wrapped Markdown: `## [0.8.1] - 2026-11-11` starts a version, `### Fixed` a group, `- ` an
//! item (indented two spaces per level), and a wrapped line continues the line before it.

use crate::version::Version;

/// One version's section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub version: String,
    /// The day it came out (YYYY-MM-DD), as the changelog says.
    pub date: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `### Fixed`.
    Heading(String),
    /// A paragraph or a list item, its wrapped lines joined, at `indent` levels; `marker` is `•` for an item and
    /// `1.` for a numbered one.
    Text { indent: u8, marker: Option<String>, text: String },
}

/// The sections of `changelog` for the versions after `from`, up to and including `to`, newest first. "Unreleased"
/// is never among them.
pub fn between(changelog: &str, from: &Version, to: &Version) -> Vec<Section> {
    sections(changelog).into_iter().filter(|s| Version::parse(&s.version).is_some_and(|v| &v > from && &v <= to)).collect()
}

/// Every version's section, as the changelog orders them.
pub fn sections(changelog: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;
    // Whether the last line was blank: a line after one starts a new block.
    let mut gap = true;
    for line in changelog.lines() {
        let line = line.trim_end();
        if let Some(head) = line.strip_prefix("## ") {
            out.extend(current.take());
            // `[0.8.1] - 2026-11-11`; "Unreleased" has no brackets and is skipped.
            current = head.strip_prefix('[').and_then(|h| h.split_once(']')).map(|(version, rest)| Section {
                version: version.trim().to_string(),
                date: rest.trim().trim_start_matches(['-', '–', '—']).trim().to_string(),
                blocks: Vec::new(),
            });
            gap = true;
            continue;
        }
        if line.starts_with("# ") {
            out.extend(current.take());
            continue;
        }
        let Some(section) = current.as_mut() else { continue };
        if line.trim().is_empty() {
            gap = true;
            continue;
        }
        if let Some(h) = line.strip_prefix("### ") {
            section.blocks.push(Block::Heading(h.trim().to_string()));
            gap = true;
            continue;
        }
        let spaces = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();
        let indent = (spaces / 2).min(6) as u8;
        let item = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")).map(|t| ("•".to_string(), t)).or_else(|| {
            let (n, rest) = trimmed.split_once(". ")?;
            (!n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit())).then(|| (format!("{n}."), rest))
        });
        match (item, section.blocks.last_mut()) {
            (Some((marker, text)), _) => section.blocks.push(Block::Text { indent, marker: Some(marker), text: text.trim().to_string() }),
            // A wrapped line: it continues the block before it.
            (None, Some(Block::Text { text, .. })) if !gap => {
                text.push(' ');
                text.push_str(trimmed);
            }
            // A paragraph, inside an item when indented.
            (None, _) => section.blocks.push(Block::Text { indent, marker: None, text: trimmed.to_string() }),
        }
        gap = false;
    }
    out.extend(current);
    out
}

/// A run of text and how it's drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    /// `code`: a monospace font.
    pub code: bool,
}

/// The inline Markdown the changelog uses: `**bold**`, `` `code` `` and links, which keep only their text.
pub fn spans(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut push = |s: &str, bold: bool, code: bool| {
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.bold == bold && last.code == code => last.text.push_str(s),
            _ => out.push(Span { text: s.to_string(), bold, code }),
        }
    };
    let mut bold = false;
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("**") {
            bold = !bold;
            rest = r;
        } else if let Some(r) = rest.strip_prefix('`')
            && let Some(end) = r.find('`')
        {
            push(&r[..end], bold, true);
            rest = &r[end + 1..];
        } else if let Some(r) = rest.strip_prefix('[')
            && let Some(close) = r.find("](")
            && let Some(end) = r[close + 2..].find(')')
        {
            // The link's text, which may itself be bold; the address is left out.
            let inner = &r[..close];
            for s in spans(inner) {
                push(&s.text, bold || s.bold, s.code);
            }
            rest = &r[close + 2 + end + 1..];
        } else {
            let next = rest.char_indices().skip(1).find(|(_, c)| matches!(c, '*' | '`' | '[')).map_or(rest.len(), |(i, _)| i);
            push(&rest[..next], bold, false);
            rest = &rest[next..];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\nIntro text.\n\n## Unreleased\n\n### Fixed\n- **Not yet**: released.\n\n\
## [0.8.2] - 2026-11-20\n\nA paragraph that\nwraps.\n\n### Added\n- **Updates**: Elite Nexus tells you\n  when a new version is out.\n  - nested item\n    that wraps\n\n  A paragraph inside the item.\n- `code` and [a link](https://example.com).\n\n### Fixed\n1. First.\n2. Second.\n\n\
## [0.8.1] - 2026-11-11\n\n### Fixed\n- One fix.\n\n## [0.8.0] - 2026-11-05\n\n- Old.\n";

    fn v(s: &str) -> Version {
        Version::parse(s).expect("version")
    }

    #[test]
    fn sections_between_two_versions() {
        let s = between(LOG, &v("0.8.0"), &v("0.8.2"));
        assert_eq!(s.iter().map(|s| s.version.as_str()).collect::<Vec<_>>(), ["0.8.2", "0.8.1"], "newest first, never Unreleased");
        assert_eq!(s[0].date, "2026-11-20");
        assert!(between(LOG, &v("0.8.2"), &v("0.8.2")).is_empty(), "nothing newer");
        assert_eq!(between(LOG, &v("0.7.0"), &v("0.8.0")).len(), 1);
    }

    #[test]
    fn wrapped_lines_and_lists() {
        let s = &sections(LOG)[0];
        let text = |indent: u8, marker: Option<&str>, t: &str| Block::Text { indent, marker: marker.map(str::to_string), text: t.to_string() };
        assert_eq!(
            s.blocks,
            [
                text(0, None, "A paragraph that wraps."),
                Block::Heading("Added".into()),
                text(0, Some("•"), "**Updates**: Elite Nexus tells you when a new version is out."),
                text(1, Some("•"), "nested item that wraps"),
                text(1, None, "A paragraph inside the item."),
                text(0, Some("•"), "`code` and [a link](https://example.com)."),
                Block::Heading("Fixed".into()),
                text(0, Some("1."), "First."),
                text(0, Some("2."), "Second."),
            ]
        );
    }

    #[test]
    fn inline_markdown() {
        let s = |text: &str, bold: bool, code: bool| Span { text: text.into(), bold, code };
        assert_eq!(spans("**Updates**: tells you"), [s("Updates", true, false), s(": tells you", false, false)]);
        assert_eq!(spans("run `cargo` [here](https://x.y) or **[there](https://z)**."), [s("run ", false, false), s("cargo", false, true), s(" here or ", false, false), s("there", true, false), s(".", false, false)]);
        assert_eq!(spans("a * b and [not a link"), [s("a * b and [not a link", false, false)]);
        assert_eq!(spans("× › ≈ ünïcode"), [s("× › ≈ ünïcode", false, false)]);
    }

    #[test]
    fn the_real_changelog_reads() {
        let log = include_str!("../../../CHANGELOG.md");
        let all = sections(log);
        assert!(all.len() > 5, "{} sections", all.len());
        for s in &all {
            assert!(Version::parse(&s.version).is_some(), "{} is a version", s.version);
            assert!(!s.blocks.is_empty(), "{} has text", s.version);
        }
    }
}
