use semver::Version;

use super::ReleaseRow;

/// `v1.2.3` or `1.2.3`; tags that are not semver are not firmware releases.
pub fn parse_version(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().trim_start_matches(['v', 'V'])).ok()
}

/// A prerelease by GitHub's flag or by a `-rc1` style suffix.
pub fn is_prerelease(release: &ReleaseRow, version: &Version) -> bool {
    release.prerelease || !version.pre.is_empty()
}

/// The highest version; prereleases only on the beta channel.
pub fn pick_latest(releases: Vec<ReleaseRow>, beta: bool) -> Option<(Version, ReleaseRow)> {
    releases
        .into_iter()
        .filter_map(|r| parse_version(&r.tag).map(|v| (v, r)))
        .filter(|(v, r)| beta || !is_prerelease(r, v))
        .max_by(|a, b| a.0.cmp(&b.0))
}

/// A build that reports no semver (a dev build) is always offered the release.
pub fn update_available(latest: &Version, current: Option<&str>) -> bool {
    current
        .and_then(parse_version)
        .is_none_or(|current| *latest > current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool) -> ReleaseRow {
        ReleaseRow {
            id: String::new(),
            project_id: String::new(),
            tag: tag.into(),
            prerelease,
            notes: String::new(),
            published_at: String::new(),
        }
    }

    #[test]
    fn picks_highest_stable_semver() {
        let rows = vec![
            release("v1.9.0", false),
            release("v1.10.0", false),
            release("v2.0.0-rc1", false),
            release("v1.11.0", true),
            release("nightly", false),
        ];
        assert_eq!(pick_latest(rows.clone(), false).unwrap().1.tag, "v1.10.0");
        assert_eq!(pick_latest(rows, true).unwrap().1.tag, "v2.0.0-rc1");
        assert!(pick_latest(vec![release("nightly", false)], false).is_none());
    }

    #[test]
    fn compares_against_current() {
        let latest = Version::new(1, 2, 0);
        assert!(update_available(&latest, Some("v1.1.9")));
        assert!(!update_available(&latest, Some("1.2.0")));
        assert!(!update_available(&latest, Some("v1.3.0")));
        assert!(update_available(&latest, Some("dev")));
        assert!(update_available(&latest, None));
    }
}
