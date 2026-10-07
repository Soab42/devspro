//! Finding ids, their strict format, the order they are removed in, the exact
//! docker argv each one runs, and which volumes DevsPro itself owns.

/// Compose project names of DevsPro's own stacks, Databases and Mail Catcher,
/// read from the modules that run them. Their volumes hold the user's data.
pub const PROTECTED_PROJECTS: [&str; 2] = [
    crate::modules::databases::PROJECT_NAME,
    crate::modules::mailcatcher::PROJECT_NAME,
];

pub const COMPOSE_PROJECT_LABEL: &str = "com.docker.compose.project";

/// Longest age `buildcache:older:<days>` accepts.
const MAX_DAYS: u32 = 3650;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerId {
    Container(String),
    Image(String),
    Network(String),
    Volume(String),
    BuildCacheAll,
    BuildCacheOlder(u32),
}

fn is_hex_id(s: &str) -> bool {
    (12..=64).contains(&s.len()) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Docker's volume-name rule: starts alphanumeric, then `[A-Za-z0-9_.-]`.
fn is_volume_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphanumeric())
        && s.len() <= 255
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

impl DockerId {
    pub fn parse(id: &str) -> Result<DockerId, String> {
        let bad = || format!("not a docker cleaner id: {id}");
        let (kind, rest) = id.split_once(':').ok_or_else(bad)?;
        match kind {
            "container" if is_hex_id(rest) => Ok(DockerId::Container(rest.to_string())),
            "image" if is_hex_id(rest) => Ok(DockerId::Image(rest.to_string())),
            "network" if is_hex_id(rest) => Ok(DockerId::Network(rest.to_string())),
            "volume" if is_volume_name(rest) => Ok(DockerId::Volume(rest.to_string())),
            "buildcache" if rest == "all" => Ok(DockerId::BuildCacheAll),
            "buildcache" => {
                let days = rest.strip_prefix("older:").ok_or_else(bad)?;
                let digits = !days.is_empty() && days.bytes().all(|b| b.is_ascii_digit());
                match days.parse::<u32>() {
                    Ok(d) if digits && (1..=MAX_DAYS).contains(&d) && days == d.to_string() => {
                        Ok(DockerId::BuildCacheOlder(d))
                    }
                    _ => Err(bad()),
                }
            }
            _ => Err(bad()),
        }
    }

    pub fn id(&self) -> String {
        match self {
            DockerId::Container(i) => format!("container:{i}"),
            DockerId::Image(i) => format!("image:{i}"),
            DockerId::Network(i) => format!("network:{i}"),
            DockerId::Volume(n) => format!("volume:{n}"),
            DockerId::BuildCacheAll => "buildcache:all".to_string(),
            DockerId::BuildCacheOlder(d) => format!("buildcache:older:{d}"),
        }
    }

    /// Containers, then images, then networks, then volumes, then build cache.
    pub fn rank(&self) -> u8 {
        match self {
            DockerId::Container(_) => 0,
            DockerId::Image(_) => 1,
            DockerId::Network(_) => 2,
            DockerId::Volume(_) => 3,
            DockerId::BuildCacheAll | DockerId::BuildCacheOlder(_) => 4,
        }
    }

    /// The whole command line after `docker`. Nothing but a validated id is
    /// ever interpolated, and there is no `-f` on anything that holds data.
    pub fn argv(&self) -> Vec<String> {
        let words: Vec<String> = match self {
            DockerId::Container(i) => vec!["rm".into(), i.clone()],
            DockerId::Image(i) => vec!["image".into(), "rm".into(), i.clone()],
            DockerId::Network(i) => vec!["network".into(), "rm".into(), i.clone()],
            DockerId::Volume(n) => vec!["volume".into(), "rm".into(), n.clone()],
            DockerId::BuildCacheAll => vec!["builder".into(), "prune".into(), "--all".into(), "--force".into()],
            DockerId::BuildCacheOlder(d) => vec![
                "builder".into(),
                "prune".into(),
                "--all".into(),
                "--force".into(),
                "--filter".into(),
                format!("until={}h", u64::from(*d) * 24),
            ],
        };
        words
    }
}

/// A `repo:tag` reference as docker prints it. Anything that could read as a
/// flag or hold whitespace is refused.
fn is_image_ref(s: &str) -> bool {
    s.len() <= 300
        && s.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'/' | b':' | b'@'))
}

/// `docker image rm` for an image: each of its tags, or the bare id when it has
/// none. Removing the last tag removes the image; never `-f`.
pub fn image_argv(id: &str, tags: &[String]) -> Vec<String> {
    let refs: Vec<&String> = tags.iter().filter(|t| is_image_ref(t)).collect();
    let mut argv = vec!["image".to_string(), "rm".to_string()];
    if refs.is_empty() {
        argv.push(id.to_string());
    } else {
        argv.extend(refs.into_iter().cloned());
    }
    argv
}

/// Parses, drops repeats and puts the ids in removal order. One bad id fails
/// the lot, so nothing runs on a malformed request.
pub fn plan(ids: &[String]) -> Result<Vec<DockerId>, String> {
    let mut parsed: Vec<DockerId> = Vec::new();
    for id in ids {
        let p = DockerId::parse(id)?;
        if !parsed.contains(&p) {
            parsed.push(p);
        }
    }
    parsed.sort_by_key(DockerId::rank);
    Ok(parsed)
}

/// A volume that belongs to one of DevsPro's own stacks.
pub fn is_protected(name: &str, project: Option<&str>) -> bool {
    project.is_some_and(|p| PROTECTED_PROJECTS.contains(&p))
        || PROTECTED_PROJECTS.iter().any(|p| name.strip_prefix(p).is_some_and(|rest| rest.starts_with('_')))
}

/// A protected volume goes only when the user typed its name. The Review
/// modal reports confirmed finding ids; the bare name is accepted too.
pub fn is_confirmed(name: &str, confirmed: &[String]) -> bool {
    let id = format!("volume:{name}");
    confirmed.iter().any(|c| *c == id || c == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "01ae540525d8";

    #[test]
    fn valid_ids_round_trip() {
        for id in [
            format!("container:{HEX}"),
            format!("image:{}", "a".repeat(64)),
            format!("network:{HEX}"),
            "volume:my_data-1.x".to_string(),
            "buildcache:all".to_string(),
            "buildcache:older:7".to_string(),
        ] {
            assert_eq!(DockerId::parse(&id).unwrap().id(), id);
        }
    }

    #[test]
    fn malformed_ids_are_refused() {
        for id in [
            "",
            "container",
            "container:",
            "container:abc",
            "container:01AE540525D8",
            "container:01ae540525d8 --force",
            "container:-f",
            "image:sha256:0123456789ab",
            "network:../x",
            "volume:",
            "volume:-rf",
            "volume:a b",
            "volume:a/b",
            "volume:../etc",
            "buildcache:",
            "buildcache:older:",
            "buildcache:older:0",
            "buildcache:older:007",
            "buildcache:older:-1",
            "buildcache:older:3651",
            "buildcache:older:7h",
            "buildcache:everything",
            "prune:all",
        ] {
            assert!(DockerId::parse(id).is_err(), "{id:?} should be refused");
        }
    }

    #[test]
    fn plan_orders_removal_and_drops_repeats() {
        let ids: Vec<String> = [
            "buildcache:all".to_string(),
            "volume:data".to_string(),
            format!("network:{HEX}"),
            format!("image:{HEX}"),
            format!("container:{HEX}"),
            format!("container:{HEX}"),
        ]
        .into();
        let order: Vec<String> = plan(&ids).unwrap().iter().map(DockerId::id).collect();
        assert_eq!(
            order,
            vec![
                format!("container:{HEX}"),
                format!("image:{HEX}"),
                format!("network:{HEX}"),
                "volume:data".to_string(),
                "buildcache:all".to_string(),
            ]
        );
    }

    #[test]
    fn one_bad_id_fails_the_plan() {
        let ids = vec![format!("container:{HEX}"), "volume:-x".to_string()];
        assert!(plan(&ids).is_err());
    }

    #[test]
    fn argv_is_fixed() {
        let v = |d: DockerId| d.argv();
        assert_eq!(v(DockerId::Container(HEX.into())), ["rm", HEX]);
        assert_eq!(v(DockerId::Image(HEX.into())), ["image", "rm", HEX]);
        assert_eq!(v(DockerId::Network(HEX.into())), ["network", "rm", HEX]);
        assert_eq!(v(DockerId::Volume("data".into())), ["volume", "rm", "data"]);
        assert_eq!(v(DockerId::BuildCacheAll), ["builder", "prune", "--all", "--force"]);
        assert_eq!(
            v(DockerId::BuildCacheOlder(7)),
            ["builder", "prune", "--all", "--force", "--filter", "until=168h"]
        );
    }

    #[test]
    fn image_argv_removes_each_tag_or_the_bare_id() {
        let tags = vec!["app:latest".to_string(), "reg.io:5000/app:v2".to_string()];
        assert_eq!(image_argv(HEX, &tags), ["image", "rm", "app:latest", "reg.io:5000/app:v2"]);
        assert_eq!(image_argv(HEX, &[]), ["image", "rm", HEX]);
        let bad = vec!["-f".to_string(), "a b".to_string()];
        assert_eq!(image_argv(HEX, &bad), ["image", "rm", HEX]);
        assert!(!image_argv(HEX, &tags).iter().any(|a| a == "-f" || a == "--force"));
    }

    #[test]
    fn protection_is_tied_to_the_stacks_own_constants() {
        assert_eq!(PROTECTED_PROJECTS[0], crate::modules::databases::PROJECT_NAME);
        assert_eq!(PROTECTED_PROJECTS[1], crate::modules::mailcatcher::PROJECT_NAME);
        for p in PROTECTED_PROJECTS {
            assert!(is_protected("any", Some(p)));
            assert!(is_protected(&format!("{p}_data"), None));
        }
    }

    #[test]
    fn devspro_stacks_are_protected() {
        assert!(is_protected("whatever", Some("devspro")));
        assert!(is_protected("whatever", Some("devspro_mailcatcher")));
        assert!(is_protected("devspro_pgdata", None));
        assert!(is_protected("devspro_mailcatcher_data", None));
        assert!(!is_protected("brokercore_data", Some("brokercore")));
        assert!(!is_protected("devsprox_data", None));
        assert!(!is_protected("0a98bd5b166c", None));
    }

    #[test]
    fn confirmation_matches_the_volume_exactly() {
        let confirmed = vec!["volume:devspro_pgdata".to_string(), "other".to_string()];
        assert!(is_confirmed("devspro_pgdata", &confirmed));
        assert!(is_confirmed("other", &confirmed));
        assert!(!is_confirmed("devspro_pg", &confirmed));
        assert!(!is_confirmed("devspro_pgdata2", &confirmed));
    }
}
