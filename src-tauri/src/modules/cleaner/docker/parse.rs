//! Reading what the docker CLI prints: JSON lines from `ps`, `network ls` and
//! `volume ls`, the one JSON document from `system df -v`, and docker's
//! human sizes ("53.2kB", "1.07GB").

use super::ids::COMPOSE_PROJECT_LABEL;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;

/// Docker prints decimal units. Anything unreadable counts as zero.
pub fn parse_size(text: &str) -> u64 {
    let text = text.trim();
    let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let (num, unit) = text.split_at(split);
    let Ok(n) = num.parse::<f64>() else { return 0 };
    let mult: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "pb" => 1e15,
        _ => return 0,
    };
    (n * mult) as u64
}

/// The bytes in "Total reclaimed space: 1.2GB", or zero.
pub fn reclaimed(output: &str) -> u64 {
    output
        .lines()
        .find_map(|l| l.trim().strip_prefix("Total reclaimed space:"))
        .map_or(0, parse_size)
}

fn lines<T: for<'de> Deserialize<'de>>(text: &str) -> Result<Vec<T>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| format!("failed to parse docker output: {e}")))
        .collect()
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PsContainer {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Names", default)]
    pub names: String,
    #[serde(rename = "Image", default)]
    pub image: String,
    #[serde(rename = "Status", default)]
    pub status: String,
}

pub fn parse_containers(text: &str) -> Result<Vec<PsContainer>, String> {
    lines(text)
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Network {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Name", default)]
    pub name: String,
}

/// bridge, host and none are Docker's own and cannot be removed anyway.
pub fn parse_networks(text: &str) -> Result<Vec<Network>, String> {
    Ok(lines::<Network>(text)?
        .into_iter()
        .filter(|n| !matches!(n.name.as_str(), "bridge" | "host" | "none"))
        .collect())
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Volume {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Labels", default)]
    labels: String,
}

impl Volume {
    /// `volume ls` flattens labels to `k=v,k=v`; a compose project name has no comma.
    pub fn project(&self) -> Option<String> {
        self.labels
            .split(',')
            .filter_map(|p| p.split_once('='))
            .find(|(k, _)| k.trim() == COMPOSE_PROJECT_LABEL)
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }
}

pub fn parse_volumes(text: &str) -> Result<Vec<Volume>, String> {
    lines(text)
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DfImage {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Repository")]
    pub repository: String,
    #[serde(rename = "Tag")]
    pub tag: String,
    #[serde(rename = "Size")]
    pub size: String,
    #[serde(rename = "UniqueSize")]
    pub unique_size: String,
    #[serde(rename = "Containers")]
    pub containers: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DfContainer {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Size")]
    pub size: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DfVolume {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Size")]
    pub size: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DfCache {
    #[serde(rename = "InUse")]
    pub in_use: String,
    #[serde(rename = "LastUsedAt")]
    pub last_used_at: String,
    #[serde(rename = "Size")]
    pub size: String,
}

/// `docker system df -v --format json`; each array may be null.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Df {
    #[serde(rename = "Images")]
    images: Option<Vec<DfImage>>,
    #[serde(rename = "Containers")]
    containers: Option<Vec<DfContainer>>,
    #[serde(rename = "Volumes")]
    volumes: Option<Vec<DfVolume>>,
    #[serde(rename = "BuildCache")]
    build_cache: Option<Vec<DfCache>>,
}

pub fn parse_df(text: &str) -> Result<Df, String> {
    serde_json::from_str(text.trim()).map_err(|e| format!("failed to parse docker system df: {e}"))
}

/// An id without `sha256:`, cut to the 12 characters `ps` prints.
pub fn short_id(id: &str) -> &str {
    let id = id.strip_prefix("sha256:").unwrap_or(id);
    &id[..id.len().min(12)]
}

/// One image: all its tags, and what removing it frees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub id: String,
    pub tags: Vec<String>,
    pub bytes: u64,
    pub containers: u32,
}

impl ImageInfo {
    pub fn dangling(&self) -> bool {
        self.tags.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheInfo {
    pub bytes: u64,
    pub last_used: Option<DateTime<Utc>>,
    pub in_use: bool,
}

impl Df {
    /// One entry per image id; docker lists an image once per tag.
    pub fn images(&self) -> Vec<ImageInfo> {
        let mut out: Vec<ImageInfo> = Vec::new();
        for row in self.images.iter().flatten() {
            let id = short_id(&row.id).to_string();
            let tag = (row.repository != "<none>" && !row.repository.is_empty())
                .then(|| format!("{}:{}", row.repository, row.tag));
            let bytes = parse_size(if row.unique_size.is_empty() { &row.size } else { &row.unique_size });
            let containers = row.containers.trim().parse().unwrap_or(0);
            match out.iter_mut().find(|i| i.id == id) {
                Some(i) => i.tags.extend(tag),
                None => out.push(ImageInfo { id, tags: tag.into_iter().collect(), bytes, containers }),
            }
        }
        out
    }

    /// Writable-layer size by 12-character container id.
    pub fn container_sizes(&self) -> HashMap<String, u64> {
        self.containers.iter().flatten().map(|c| (short_id(&c.id).to_string(), parse_size(&c.size))).collect()
    }

    pub fn volume_sizes(&self) -> HashMap<String, u64> {
        self.volumes.iter().flatten().map(|v| (v.name.clone(), parse_size(&v.size))).collect()
    }

    pub fn cache(&self) -> Vec<CacheInfo> {
        self.build_cache
            .iter()
            .flatten()
            .map(|c| CacheInfo {
                bytes: parse_size(&c.size),
                last_used: parse_docker_time(&c.last_used_at),
                in_use: c.in_use.eq_ignore_ascii_case("true"),
            })
            .collect()
    }
}

/// "2026-09-07 05:12:30.223021101 +0000 UTC": the date, time and offset.
pub fn parse_docker_time(text: &str) -> Option<DateTime<Utc>> {
    let mut parts = text.split_whitespace();
    let stamp = format!("{} {} {}", parts.next()?, parts.next()?, parts.next()?);
    DateTime::parse_from_str(&stamp, "%Y-%m-%d %H:%M:%S%.f %z").ok().map(|t| t.with_timezone(&Utc))
}

/// Sizes of the in-use-free cache: everything, and what was last used before `cutoff`.
pub fn cache_totals(cache: &[CacheInfo], cutoff: DateTime<Utc>) -> (u64, u64) {
    let free = cache.iter().filter(|c| !c.in_use);
    let all = free.clone().map(|c| c.bytes).sum();
    let old = free.filter(|c| c.last_used.is_some_and(|t| t < cutoff)).map(|c| c.bytes).sum();
    (all, old)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_decimal() {
        assert_eq!(parse_size("53.2kB"), 53_200);
        assert_eq!(parse_size("1.07GB"), 1_070_000_000);
        assert_eq!(parse_size("0B"), 0);
        assert_eq!(parse_size("447MB"), 447_000_000);
        assert_eq!(parse_size("N/A"), 0);
        assert_eq!(parse_size(""), 0);
        assert_eq!(parse_size("5 weird"), 0);
    }

    #[test]
    fn reads_reclaimed_space() {
        assert_eq!(reclaimed("ab12\nTotal reclaimed space: 2.5GB\n"), 2_500_000_000);
        assert_eq!(reclaimed("nothing"), 0);
    }

    #[test]
    fn parses_ps_json_lines() {
        let text = concat!(
            r#"{"ID":"bd830e46b3c6","Names":"web-1","Image":"nginx","Status":"Exited (0) 42 hours ago","Extra":1}"#,
            "\n\n",
            r#"{"ID":"01ae540525d8","Names":"db","Image":"pg","Status":"Created"}"#,
            "\n"
        );
        let c = parse_containers(text).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].names, "web-1");
        assert_eq!(c[0].status, "Exited (0) 42 hours ago");
        assert!(parse_containers("not json").is_err());
        assert!(parse_containers("").unwrap().is_empty());
    }

    #[test]
    fn networks_skip_docker_builtins() {
        let text = concat!(
            r#"{"ID":"79d060342d7c","Name":"bridge"}"#, "\n",
            r#"{"ID":"1111111111aa","Name":"host"}"#, "\n",
            r#"{"ID":"2222222222bb","Name":"none"}"#, "\n",
            r#"{"ID":"a9ae4e7f62e2","Name":"bc-net"}"#, "\n"
        );
        let n = parse_networks(text).unwrap();
        assert_eq!(n, vec![Network { id: "a9ae4e7f62e2".into(), name: "bc-net".into() }]);
    }

    #[test]
    fn volume_project_comes_from_the_label() {
        let text = concat!(
            r#"{"Name":"brokercore_pg","Labels":"com.docker.compose.project=brokercore,com.docker.compose.volume=pg"}"#, "\n",
            r#"{"Name":"abc","Labels":"com.docker.volume.anonymous="}"#, "\n",
            r#"{"Name":"devspro_x","Labels":""}"#, "\n"
        );
        let v = parse_volumes(text).unwrap();
        assert_eq!(v[0].project().as_deref(), Some("brokercore"));
        assert_eq!(v[1].project(), None);
        assert_eq!(v[2].project(), None);
    }

    #[test]
    fn df_images_merge_tags_and_find_dangling() {
        let df = parse_df(
            r#"{"Images":[
              {"ID":"sha256:aaaaaaaaaaaa1111","Repository":"app","Tag":"latest","Size":"447MB","UniqueSize":"106.5MB","Containers":"0"},
              {"ID":"sha256:aaaaaaaaaaaa1111","Repository":"app","Tag":"v2","Size":"447MB","UniqueSize":"106.5MB","Containers":"0"},
              {"ID":"sha256:bbbbbbbbbbbb2222","Repository":"<none>","Tag":"<none>","Size":"10MB","UniqueSize":"","Containers":"0"},
              {"ID":"sha256:cccccccccccc3333","Repository":"db","Tag":"1","Size":"1MB","UniqueSize":"1MB","Containers":"2"}],
             "Containers":null,"Volumes":[{"Name":"v1","Size":"2kB"}],"BuildCache":null}"#,
        )
        .unwrap();
        let images = df.images();
        assert_eq!(images.len(), 3);
        assert_eq!(images[0].id, "aaaaaaaaaaaa");
        assert_eq!(images[0].tags, vec!["app:latest", "app:v2"]);
        assert_eq!(images[0].bytes, 106_500_000);
        assert!(images[1].dangling());
        assert_eq!(images[1].bytes, 10_000_000);
        assert_eq!(images[2].containers, 2);
        assert_eq!(df.volume_sizes()["v1"], 2_000);
        assert!(df.cache().is_empty());
        assert!(parse_df("nope").is_err());
    }

    #[test]
    fn cache_totals_split_by_age_and_skip_in_use() {
        let df = parse_df(
            r#"{"BuildCache":[
              {"InUse":"false","LastUsedAt":"2026-09-07 05:12:30.223021101 +0000 UTC","Size":"5MB"},
              {"InUse":"false","LastUsedAt":"2026-10-06 05:12:30 +0000 UTC","Size":"3MB"},
              {"InUse":"true","LastUsedAt":"2026-01-01 00:00:00 +0000 UTC","Size":"100MB"},
              {"InUse":"false","LastUsedAt":"","Size":"1MB"}]}"#,
        )
        .unwrap();
        let cutoff = "2026-10-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(cache_totals(&df.cache(), cutoff), (9_000_000, 5_000_000));
    }

    #[test]
    fn docker_times_respect_the_offset() {
        let t = parse_docker_time("2026-08-24 13:22:58.145427893 +0600 +06").unwrap();
        assert_eq!(t.to_rfc3339(), "2026-08-24T07:22:58.145427893+00:00");
        assert!(parse_docker_time("").is_none());
    }

    #[test]
    fn short_ids_drop_the_prefix() {
        assert_eq!(short_id("sha256:3c988764b52a49a2"), "3c988764b52a");
        assert_eq!(short_id("abc"), "abc");
    }
}
