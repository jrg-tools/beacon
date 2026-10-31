//! What the admin types for a project, normalised and validated before it reaches the database.

use axum::http::StatusCode;
use serde::Deserialize;

use crate::error::{Result, status};

const DEFAULT_EMOJI: &str = "📡";

#[derive(Deserialize)]
pub struct ProjectInput {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub emoji: String,
    pub repo: String,
    pub assets: String,
}

/// Letters, digits and `extra`; never only dots, so nothing reads as a path segment.
fn safe(s: &str, extra: &str) -> bool {
    !s.trim_matches('.').is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// One emoji, ZWJ sequences and skin tones included (👩🏽‍💻 is 4 chars, 👨‍👩‍👧‍👦 is 7). Letters are
/// refused; digits, `#` and `*` stay for keycaps (1️⃣).
fn emoji_ok(s: &str) -> bool {
    (1..=10).contains(&s.chars().count())
        && !s.is_ascii()
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c.is_ascii_alphabetic())
}

impl ProjectInput {
    /// Normalises what the admin typed; slugs and asset names end up in URLs and headers.
    pub fn validate(mut self) -> Result<Self> {
        let bad = |message| Err(status(StatusCode::BAD_REQUEST, message));
        self.slug = self.slug.trim().to_ascii_lowercase();
        self.name = self.name.trim().to_string();
        self.emoji = match self.emoji.trim() {
            "" => DEFAULT_EMOJI.into(),
            e => e.into(),
        };
        self.repo = self
            .repo
            .trim()
            .trim_start_matches("https://github.com/")
            .trim_end_matches('/')
            .into();
        let assets: Vec<&str> = self
            .assets
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if !safe(&self.slug, "-") {
            return bad("slug: letters, digits and dashes");
        }
        if self.name.is_empty() {
            return bad("name is required");
        }
        if !emoji_ok(&self.emoji) {
            return bad("emoji: a single emoji");
        }
        if !matches!(self.repo.split_once('/'), Some((o, r)) if safe(o, "-_.") && safe(r, "-_.")) {
            return bad("repo: owner/name");
        }
        if assets.is_empty() || !assets.iter().all(|a| safe(a, "-_.")) {
            return bad("assets: comma-separated file names");
        }
        self.assets = assets.join(",");
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(repo: &str, assets: &str, emoji: &str) -> ProjectInput {
        ProjectInput {
            slug: " AB98 ".into(),
            name: "AB98".into(),
            emoji: emoji.into(),
            repo: repo.into(),
            assets: assets.into(),
        }
    }

    #[test]
    fn validates_project_input() {
        let Ok(p) = input(
            "https://github.com/jrg-tools/AB98/",
            "firmware.bin, firmware-filesystem.bin",
            " 📼 ",
        )
        .validate() else {
            panic!("valid input rejected");
        };
        assert_eq!(
            (p.slug.as_str(), p.repo.as_str(), p.emoji.as_str()),
            ("ab98", "jrg-tools/AB98", "📼")
        );
        assert_eq!(p.assets, "firmware.bin,firmware-filesystem.bin");
        assert!(input("../etc", "firmware.bin", "").validate().is_err());
        assert!(input("a/b", "x\"y.bin", "").validate().is_err());
        assert!(input("a/b", " , ", "").validate().is_err());
    }

    #[test]
    fn validates_emoji() {
        let emoji = |e: &str| input("a/b", "f.bin", e).validate().map(|p| p.emoji);
        assert_eq!(emoji("").ok().as_deref(), Some(DEFAULT_EMOJI));
        assert_eq!(emoji("👨‍👩‍👧‍👦").ok().as_deref(), Some("👨‍👩‍👧‍👦"));
        assert_eq!(emoji("👩🏽‍💻").ok().as_deref(), Some("👩🏽‍💻"));
        assert!(emoji("a very long text").is_err());
        assert!(emoji("1️⃣").is_ok());
        assert!(emoji("📷 📼").is_err());
        assert!(emoji("abc").is_err());
        assert!(emoji("x📼").is_err());
    }
}
