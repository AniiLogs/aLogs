//! Where the game keeps its Unity asset bundles: the ones shipped in StreamingAssets (named files, `*.uab`) and the
//! ones it downloads into its cache (`CacheBundleFiles/<2 hex>/<hash>/cdata.uab`), whose names are in the package
//! manifests (`PackageManifest_DefaultPackage_<build>.bytes`: each bundle's name, a length byte before it, then the
//! 16 bytes of the hash its cache folder is named by).
//!
//! An install may have every bundle in the cache and none in StreamingAssets (the Python meter only looked there for
//! the font, so found none on such an install).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const REL: &str = r"cvs\res\uab\win\DefaultPackage";

/// The game's pmdata.bin and its install folder, from the game's own log or Steam (icons.py `_game`).
pub fn game(player_log: Option<&Path>) -> Option<(PathBuf, PathBuf)> {
    let pm = alogs_gamedata::gamedata::find_game_data(player_log)?;
    let s = pm.to_string_lossy().into_owned();
    let at = s.to_lowercase().rfind("aniimo_data")?;
    Some((pm.clone(), PathBuf::from(s[..at].trim_end_matches(['\\', '/']))))
}

/// StreamingAssets' bundle folder.
pub fn shipped_dir(game: &Path) -> PathBuf {
    game.join("Aniimo_Data").join("StreamingAssets").join(REL)
}

/// The download cache's folder.
pub fn cache_dir(game: &Path) -> PathBuf {
    game.join("Aniimo_Data").join(REL)
}

/// The game build the shipped manifest names (icons.py `_build`).
pub fn build(game: &Path) -> Option<String> {
    std::fs::read_to_string(shipped_dir(game).join("PackageManifest_DefaultPackage.version")).ok().map(|s| s.trim().to_string())
}

fn sorted_files(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && keep(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// The shipped bundles (`*.uab` in StreamingAssets), sorted.
pub fn shipped(game: &Path) -> Vec<PathBuf> {
    sorted_files(&shipped_dir(game), |n| n.ends_with(".uab"))
}

/// Every downloaded bundle (`CacheBundleFiles/*/*/cdata.uab`), sorted.
pub fn cached(game: &Path) -> Vec<PathBuf> {
    let root = cache_dir(game).join("CacheBundleFiles");
    let mut out = Vec::new();
    let mut subs: Vec<PathBuf> = std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    subs.sort();
    for s in subs {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&s).into_iter().flatten().flatten().map(|e| e.path().join("cdata.uab")).filter(|p| p.is_file()).collect();
        dirs.sort();
        out.extend(dirs);
    }
    out
}

/// Bundle names and their cache hashes from a manifest's bytes: every `<length byte><name>.uab<16 hash bytes>`
/// whose hash is one of `hashes` (the cache's folders).
pub fn manifest_entries(d: &[u8], hashes: &HashSet<String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let printable = |b: u8| (0x21..0x7f).contains(&b);
    let mut i = 0;
    while let Some(k) = d[i..].windows(4).position(|w| w == b".uab") {
        let end = i + k + 4;
        i = end;
        let Some(hash) = d.get(end..end + 16) else { break };
        let hash: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        if !hashes.contains(&hash) {
            continue;
        }
        // the longest printable run before ".uab" that its length byte agrees with
        let mut name = None;
        for len in (5..=255usize).rev() {
            if len + 1 > end {
                continue;
            }
            let s = &d[end - len..end];
            if d[end - len - 1] as usize == len && s.iter().all(|&b| printable(b)) {
                name = Some(String::from_utf8_lossy(s).into_owned());
                break;
            }
        }
        if let Some(n) = name {
            out.push((n, hash));
        }
    }
    out
}

/// Downloaded bundles by name: name -> cdata.uab, from the manifests of the newest build that names each.
pub fn cached_by_name(game: &Path) -> HashMap<String, PathBuf> {
    let root = cache_dir(game).join("CacheBundleFiles");
    let files = cached(game);
    let hashes: HashSet<String> = files.iter().filter_map(|p| p.parent()?.file_name().map(|n| n.to_string_lossy().to_lowercase())).collect();
    let mut manifests: Vec<(u64, PathBuf)> = Vec::new();
    for dir in [cache_dir(game).join("ManifestFiles"), shipped_dir(game)] {
        for p in sorted_files(&dir, |n| n.starts_with("PackageManifest_DefaultPackage_") && n.ends_with(".bytes")) {
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            if let Ok(v) = stem.trim_start_matches("PackageManifest_DefaultPackage_").parse::<u64>() {
                manifests.push((v, p));
            }
        }
    }
    manifests.sort_by_key(|m| std::cmp::Reverse(m.0));
    let mut out = HashMap::new();
    for (_, p) in manifests {
        let Ok(d) = std::fs::read(&p) else { continue };
        for (name, hash) in manifest_entries(&d, &hashes) {
            out.entry(name).or_insert_with(|| root.join(&hash[..2]).join(&hash).join("cdata.uab"));
        }
    }
    out
}
