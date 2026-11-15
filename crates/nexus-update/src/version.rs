//! Release versions (`0.8.3`, `v0.9.0-beta.1`), in semver's order: a pre-release comes before its release, `beta.2`
//! after `beta.1`, `0.10.0` after `0.9.0`.

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The pre-release part (`beta.1`), empty for a release.
    pub pre: Vec<Ident>,
}

/// One dot-separated part of a pre-release: a number compares as a number, text as text, and a number comes first.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ident {
    Num(u64),
    Text(String),
}

impl Version {
    /// Reads `0.8.3`, `v0.8.3` or `0.9.0-beta.1`; build metadata after a `+` is ignored. None for anything else.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let s = s.split_once('+').map_or(s, |(v, _)| v);
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (s, None),
        };
        let mut nums = core.split('.').map(|p| if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) { p.parse::<u64>().ok() } else { None });
        let (major, minor, patch) = (nums.next()??, nums.next()??, nums.next()??);
        if nums.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => {
                let mut ids = Vec::new();
                for p in pre.split('.') {
                    if p.is_empty() || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                        return None;
                    }
                    ids.push(match p.parse::<u64>() {
                        Ok(n) if p.bytes().all(|b| b.is_ascii_digit()) => Ident::Num(n),
                        _ => Ident::Text(p.to_string()),
                    });
                }
                ids
            }
        };
        Some(Version { major, minor, patch, pre })
    }

    /// This build's version (the workspace's, which the app shows).
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version { major: 0, minor: 0, patch: 0, pre: Vec::new() })
    }

    /// A pre-release (a beta).
    pub fn is_pre(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            // A release comes after its pre-releases.
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let o = match (a, b) {
                        (Ident::Num(a), Ident::Num(b)) => a.cmp(b),
                        (Ident::Num(_), Ident::Text(_)) => Ordering::Less,
                        (Ident::Text(_), Ident::Num(_)) => Ordering::Greater,
                        (Ident::Text(a), Ident::Text(b)) => a.cmp(b),
                    };
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                // All the same so far: more parts come later (`beta.1.1` after `beta.1`).
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        for (i, p) in self.pre.iter().enumerate() {
            f.write_str(if i == 0 { "-" } else { "." })?;
            match p {
                Ident::Num(n) => write!(f, "{n}")?,
                Ident::Text(t) => f.write_str(t)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("{s} should read"))
    }

    #[test]
    fn reads_tags_and_versions() {
        assert_eq!(v("v0.8.3"), Version { major: 0, minor: 8, patch: 3, pre: Vec::new() });
        assert_eq!(v("0.9.0-beta.1").pre, vec![Ident::Text("beta".into()), Ident::Num(1)]);
        assert_eq!(v("1.2.3+build.5"), v("1.2.3"), "build metadata is ignored");
        assert_eq!(v(" V1.0.0 ").to_string(), "1.0.0");
        assert_eq!(v("0.9.0-beta.1").to_string(), "0.9.0-beta.1");
        for bad in ["", "0.8", "0.8.3.1", "x.8.3", "0.8.3-", "0.8.3-beta..1", "0.8.3-beta_1", "-1.0.0", "0.8.+3", "nightly"] {
            assert!(Version::parse(bad).is_none(), "{bad:?} isn't a version");
        }
    }

    #[test]
    fn orders_as_semver_does() {
        let chain = ["0.8.1", "0.8.3", "0.9.0-alpha", "0.9.0-beta", "0.9.0-beta.1", "0.9.0-beta.2", "0.9.0-beta.10", "0.9.0-beta.10.1", "0.9.0-rc.1", "0.9.0", "0.10.0", "1.0.0"];
        for pair in chain.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
            assert!(v(pair[1]) > v(pair[0]));
        }
        assert_eq!(v("v0.8.3").cmp(&v("0.8.3")), Ordering::Equal);
        // A number before text in the same place.
        assert!(v("1.0.0-1") < v("1.0.0-alpha"));
        assert!(!v("0.8.3").is_pre());
        assert!(v("0.9.0-beta.1").is_pre());
    }

    #[test]
    fn this_build_has_a_version() {
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }
}
