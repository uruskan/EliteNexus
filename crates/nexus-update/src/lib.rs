// Licence: GPL-3.0-or-later, with additional terms under its section 7: see LICENSE-ADDITIONAL-TERMS.md at the repository root.
//! Elite Nexus updates: which release is the newest, whether it's genuine, and putting it in place.
//!
//! Every release carries `update.json` (the [`Manifest`]: the version and, for each platform, the archive's name,
//! address, size and SHA-256) and `update.json.minisig`, its signature with the key whose public half is compiled in
//! (`src/minisign.pub`). The app reads the latest release's manifest, checks the signature, downloads the archive for
//! its platform, checks the archive's SHA-256 against the manifest, takes the program from it ([`archive`]) and puts it
//! in place once it has closed ([`install`]). Nothing here touches the data folder or uploads anything.
//!
//! The manifest's address and fields are a promise to every version that has shipped: each later release must keep
//! serving them (AGENTS.md › Releases).

pub mod archive;
pub mod install;
pub mod manifest;
pub mod notes;
pub mod version;

pub use manifest::{Asset, Manifest, ManifestError};
pub use version::Version;

/// The repository releases come from.
pub const REPO: &str = "uruskan/EliteNexus";
/// The manifest's file name in every release.
pub const MANIFEST: &str = "update.json";
/// Its signature's.
pub const SIGNATURE: &str = "update.json.minisig";
/// The public key releases are signed with (a minisign key file; its secret half is the release workflow's
/// `UPDATE_SIGNING_KEY`).
pub const PUBLIC_KEY: &str = include_str!("minisign.pub");
/// Every release, newest first.
pub const RELEASES_PAGE: &str = "https://github.com/uruskan/EliteNexus/releases";
/// The releases' feed: stable releases and betas alike, without the API's hourly limit.
pub const FEED: &str = "https://github.com/uruskan/EliteNexus/releases.atom";

/// A file of the latest stable release: GitHub's download address, not its API, so no hourly limit applies. A beta is
/// never the latest release.
pub fn latest_file(file: &str) -> String {
    format!("https://github.com/{REPO}/releases/latest/download/{file}")
}

/// A file of the release tagged `tag`.
pub fn release_file(tag: &str, file: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{tag}/{file}")
}

/// The release page of `tag`.
pub fn release_page(tag: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/{tag}")
}

/// CHANGELOG.md as it was at `tag`: "What's new" for a version not installed yet.
pub fn changelog_at(tag: &str) -> String {
    format!("https://raw.githubusercontent.com/{REPO}/{tag}/CHANGELOG.md")
}

/// The release tags in the releases' feed (`releases.atom`), as its entries link to them (`…/releases/tag/v0.9.0`).
pub fn feed_tags(feed: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for part in feed.split("/releases/tag/").skip(1) {
        let tag: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+')).collect();
        if !tag.is_empty() && !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// The newest version among `tags`, betas included, and its tag.
pub fn newest_tag(tags: &[String]) -> Option<(Version, String)> {
    tags.iter().filter_map(|t| Version::parse(t).map(|v| (v, t.clone()))).max_by(|a, b| a.0.cmp(&b.0))
}

/// SHA-256 in lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_gives_its_tags_and_the_newest_wins() {
        let feed = r#"<feed><entry><id>tag:github.com,2008:Repository/1/v0.9.0-beta.1</id><link rel="alternate" type="text/html" href="https://github.com/uruskan/EliteNexus/releases/tag/v0.9.0-beta.1"/></entry>
<entry><link href="https://github.com/uruskan/EliteNexus/releases/tag/v0.8.3"/></entry><entry><link href="https://github.com/uruskan/EliteNexus/releases/tag/v0.8.3"/></entry>
<entry><link href="https://github.com/uruskan/EliteNexus/releases/tag/nightly"/></entry></feed>"#;
        let tags = feed_tags(feed);
        assert_eq!(tags, ["v0.9.0-beta.1", "v0.8.3", "nightly"]);
        let (v, tag) = newest_tag(&tags).expect("newest");
        assert_eq!((v.to_string().as_str(), tag.as_str()), ("0.9.0-beta.1", "v0.9.0-beta.1"));
        assert!(newest_tag(&feed_tags("<feed/>")).is_none());
    }

    #[test]
    fn addresses() {
        assert_eq!(latest_file(MANIFEST), "https://github.com/uruskan/EliteNexus/releases/latest/download/update.json");
        assert_eq!(release_file("v0.9.0-beta.1", SIGNATURE), "https://github.com/uruskan/EliteNexus/releases/download/v0.9.0-beta.1/update.json.minisig");
        assert_eq!(changelog_at("v0.8.3"), "https://raw.githubusercontent.com/uruskan/EliteNexus/v0.8.3/CHANGELOG.md");
        assert!(FEED.starts_with(RELEASES_PAGE));
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn the_compiled_in_key_reads() {
        assert!(minisign_verify::PublicKey::decode(PUBLIC_KEY.trim()).is_ok(), "src/minisign.pub");
    }
}
