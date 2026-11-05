//! The built-in knowledge base: Markdown documents compiled into the executable, split into
//! `##` sections and searched with BM25. No files, no setup, no embeddings service.

use std::collections::HashMap;
use std::sync::LazyLock;

/// (document title, Markdown text). The app's user guide is included from the repository docs.
const DOCS: &[(&str, &str)] = &[
    ("Making money", include_str!("../knowledge/money.md")),
    ("Travel and navigation", include_str!("../knowledge/travel.md")),
    ("Exobiology", include_str!("../knowledge/exobiology.md")),
    ("Mining", include_str!("../knowledge/mining.md")),
    ("Engineering", include_str!("../knowledge/engineering.md")),
    ("Fleet carriers", include_str!("../knowledge/carriers.md")),
    ("Combat and missions", include_str!("../knowledge/combat.md")),
    ("Colonisation, Powerplay and the BGS", include_str!("../knowledge/galaxy.md")),
    ("Websites and tools", include_str!("../knowledge/links.md")),
    ("Elite Nexus user guide", include_str!("../../../docs/USER_GUIDE.md")),
];

#[derive(Debug)]
struct Section {
    title: String,
    text: String,
    len: f64,
    tf: HashMap<String, u32>,
}

struct Index {
    sections: Vec<Section>,
    df: HashMap<String, u32>,
    avg_len: f64,
}

static INDEX: LazyLock<Index> = LazyLock::new(build);

fn tokens(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|w| {
            let w = w.to_lowercase();
            // Crude plural folding: "hotspots" = "hotspot", "materials" = "material".
            match w.strip_suffix('s') {
                Some(stem) if stem.len() >= 3 && !w.ends_with("ss") => stem.to_string(),
                _ => w,
            }
        })
}

fn build() -> Index {
    let mut sections = Vec::new();
    for (doc, md) in DOCS {
        let mut title = doc.to_string();
        let mut body = String::new();
        let flush = |title: &str, body: &mut String, sections: &mut Vec<Section>| {
            let text = body.trim().to_string();
            if !text.is_empty() {
                let mut tf = HashMap::new();
                let full = format!("{title} {title} {text}"); // headings count double
                let mut n = 0.0;
                for t in tokens(&full) {
                    *tf.entry(t).or_insert(0) += 1;
                    n += 1.0;
                }
                sections.push(Section { title: title.to_string(), text, len: n, tf });
            }
            body.clear();
        };
        for line in md.lines() {
            if let Some(h) = line.strip_prefix("## ") {
                flush(&title, &mut body, &mut sections);
                title = format!("{doc} — {}", h.trim());
            } else if !line.starts_with("# ") {
                body.push_str(line);
                body.push('\n');
            }
        }
        flush(&title, &mut body, &mut sections);
    }
    let mut df: HashMap<String, u32> = HashMap::new();
    for s in &sections {
        for t in s.tf.keys() {
            *df.entry(t.clone()).or_insert(0) += 1;
        }
    }
    let avg_len = sections.iter().map(|s| s.len).sum::<f64>() / sections.len().max(1) as f64;
    Index { sections, df, avg_len }
}

/// The best-matching sections as (title, text), most relevant first.
pub fn search(query: &str, k: usize) -> Vec<(String, String)> {
    const K1: f64 = 1.2;
    const B: f64 = 0.75;
    let idx = &*INDEX;
    let n = idx.sections.len() as f64;
    let mut q: Vec<String> = tokens(query).collect();
    q.sort();
    q.dedup();
    let mut scored: Vec<(f64, usize)> = idx
        .sections
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let score: f64 = q
                .iter()
                .filter_map(|t| {
                    let f = f64::from(*s.tf.get(t)?);
                    let df = f64::from(*idx.df.get(t).unwrap_or(&0));
                    let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
                    Some(idf * f * (K1 + 1.0) / (f + K1 * (1.0 - B + B * s.len / idx.avg_len)))
                })
                .sum();
            (score, i)
        })
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().take(k).map(|(_, i)| (idx.sections[i].title.clone(), idx.sections[i].text.clone())).collect()
}

/// Number of sections (for the status line / tests).
pub fn section_count() -> usize {
    INDEX.sections.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_relevant_sections() {
        assert!(section_count() > 30, "{} sections", section_count());
        let hits = search("how do I boost my jump range with neutron stars", 3);
        assert!(hits[0].0.to_lowercase().contains("neutron"), "{:?}", hits.iter().map(|h| &h.0).collect::<Vec<_>>());
        let hits = search("sampling distance colony range", 3);
        assert!(hits.iter().any(|h| h.0.contains("Exobiology")), "{:?}", hits.iter().map(|h| &h.0).collect::<Vec<_>>());
        let hits = search("pause EDDN uploads", 3);
        assert!(hits.iter().any(|h| h.0.contains("user guide") || h.1.contains("EDDN")));
        let hits = search("where can I kill ships legally without getting a bounty", 3);
        assert!(hits.iter().any(|h| h.0.contains("Combat")), "{:?}", hits.iter().map(|h| &h.0).collect::<Vec<_>>());
        assert!(search("zzzz qqqq", 3).is_empty());
    }
}
