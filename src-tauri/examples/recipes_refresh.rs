//! Rebuilds the recipe catalogue: reads each source's feed into
//! `catalog/recipes/sources/<id>.json`, then merges them into `catalog/recipes/catalog.json`.
//!
//!     cargo run --example recipes_refresh -- [--source ID]... [--max-page N] [--offline] [--build-only]
//!
//! Each source walks its WordPress feed (/feed/?paged=1..) until it ends (404) or
//! `--max-page`. Pages are cached raw in `local/recipe-feeds/<id>/` (gitignored), so a re-run
//! reads the cache and fetches only new pages; delete the folder to read a feed again.
//! Requests are at least 2 s apart. A 429, a 5xx or any other failure stops that source and
//! leaves its file as it was. `--offline` reads the caches only; `--build-only` skips the
//! feeds and merges the source files. Only settings, names, dates, authors, links and sensors
//! are written; never photos or post text.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ukiyo_lib::recipes::catalogue::{merge, RecipeCatalog, SourceFile, SourceRecipe, SCHEMA};
use ukiyo_lib::recipes::sources::{self, FeedItem, Source};

const USER_AGENT: &str = "ukiyo-recipe-catalogue/0.1 (Fujifilm camera app; bundles recipe settings with a link to each post)";
const GAP: Duration = Duration::from_secs(2);
const CONTENT_NS: &str = "http://purl.org/rss/1.0/modules/content/";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";

struct Args {
    sources: Vec<String>,
    max_page: Option<u32>,
    offline: bool,
    build_only: bool,
    root: PathBuf,
}

fn args() -> Args {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let mut a = Args { sources: Vec::new(), max_page: None, offline: false, build_only: false, root };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--source" => a.sources.push(it.next().expect("--source ID")),
            "--max-page" => a.max_page = it.next().and_then(|v| v.parse().ok()),
            "--offline" => a.offline = true,
            "--build-only" => a.build_only = true,
            other => panic!("unknown argument {other}"),
        }
    }
    a
}

struct Fetcher {
    agent: ureq::Agent,
    last: Option<Instant>,
    requests: u32,
}

enum Page {
    Xml(String),
    End,
}

impl Fetcher {
    fn get(&mut self, url: &str) -> anyhow::Result<Page> {
        if let Some(t) = self.last {
            std::thread::sleep(GAP.saturating_sub(t.elapsed()));
        }
        self.last = Some(Instant::now());
        self.requests += 1;
        match self.agent.get(url).header("User-Agent", USER_AGENT).call() {
            Ok(mut r) => Ok(Page::Xml(r.body_mut().with_config().limit(20 * 1024 * 1024).read_to_string()?)),
            Err(ureq::Error::StatusCode(404)) => Ok(Page::End),
            Err(e) => Err(anyhow::anyhow!("{url}: {e}")),
        }
    }
}

fn items(xml: &str) -> anyhow::Result<Vec<FeedItem>> {
    let doc = roxmltree::Document::parse(xml)?;
    let mut out = Vec::new();
    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let child = |name: &str, ns: Option<&str>| item.children().find(|c| c.tag_name().name() == name && c.tag_name().namespace() == ns).and_then(|c| c.text()).unwrap_or_default().to_string();
        let date = chrono::DateTime::parse_from_rfc2822(&child("pubDate", None)).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
        out.push(FeedItem {
            title: child("title", None),
            link: child("link", None),
            date,
            creator: child("creator", Some(DC_NS)),
            tags: item.children().filter(|c| c.has_tag_name("category")).filter_map(|c| c.text().map(str::to_string)).collect(),
            html: child("encoded", Some(CONTENT_NS)),
        });
    }
    Ok(out)
}

/// One recipe per line, so a refresh diffs by recipe.
fn write_lines<T: serde::Serialize>(path: &Path, head: &[(&str, String)], recipes: &[T]) -> anyhow::Result<()> {
    let mut s = String::from("{\n");
    for (k, v) in head {
        s += &format!("  \"{k}\": {v},\n");
    }
    s += "  \"recipes\": [\n";
    let lines: Vec<String> = recipes.iter().map(|r| serde_json::to_string(r).map(|j| format!("    {j}"))).collect::<Result<_, _>>()?;
    s += &lines.join(",\n");
    s += "\n  ]\n}\n";
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, s)?;
    Ok(())
}

struct Read {
    recipes: Vec<SourceRecipe>,
    pages: u32,
    posts: u32,
    skipped: u32,
    failed: Option<String>,
}

fn read_source(a: &Args, src: &Source, fetcher: &mut Fetcher) -> anyhow::Result<Read> {
    let id = (src.info)().id;
    let cache = a.root.join("local/recipe-feeds").join(&id);
    std::fs::create_dir_all(&cache)?;
    let mut r = Read { recipes: Vec::new(), pages: 0, posts: 0, skipped: 0, failed: None };
    let mut seen = HashSet::new();
    let mut page = 1;
    while a.max_page.is_none_or(|m| page <= m) {
        let cached = cache.join(format!("page-{page:03}.xml"));
        let xml = if cached.exists() {
            std::fs::read_to_string(&cached)?
        } else if a.offline {
            break;
        } else {
            match fetcher.get(&(src.feed_page)(page)) {
                Ok(Page::Xml(x)) => {
                    std::fs::write(&cached, &x)?;
                    x
                }
                Ok(Page::End) => break,
                Err(e) => {
                    r.failed = Some(e.to_string());
                    break;
                }
            }
        };
        let list = items(&xml)?;
        if list.is_empty() {
            break;
        }
        r.pages += 1;
        for it in list {
            // New posts shift the pages; a post seen on an earlier page is not counted twice.
            if !seen.insert(it.link.clone()) {
                continue;
            }
            r.posts += 1;
            let found = (src.recipes)(&it);
            if found.is_empty() {
                r.skipped += 1;
            }
            r.recipes.extend(found);
        }
        if page % 25 == 0 {
            eprintln!("{id}: page {page} ({} recipes)", r.recipes.len());
        }
        page += 1;
    }
    r.recipes.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.id.cmp(&b.id)));
    Ok(r)
}

fn summarise(id: &str, recipes: &[SourceRecipe]) {
    let warned = recipes.iter().filter(|r| !r.warnings.is_empty()).count();
    let total: usize = recipes.iter().map(|r| r.warnings.len()).sum();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_label: BTreeMap<String, usize> = BTreeMap::new();
    let mut sensors: BTreeMap<String, usize> = BTreeMap::new();
    for r in recipes {
        *sensors.entry(r.sensor.clone().unwrap_or_else(|| "(none)".into())).or_default() += 1;
        for w in &r.warnings {
            *by_kind.entry(format!("{:?}", w.kind)).or_default() += 1;
            let key = if w.label.is_empty() { format!("(unlabelled) {}", w.value) } else { format!("{}: {}", w.label, w.message) };
            *by_label.entry(key).or_default() += 1;
        }
    }
    println!("{id}: recipes {}, with warnings {warned}, warnings {total}", recipes.len());
    println!("{id}: no film simulation {}, sensors {sensors:?}", recipes.iter().filter(|r| r.settings.film_simulation.is_none()).count());
    println!("{id}: warnings by kind {by_kind:?}");
    let mut top: Vec<_> = by_label.into_iter().collect();
    top.sort_by_key(|t| std::cmp::Reverse(t.1));
    for (k, n) in top.iter().take(12) {
        println!("  {n:4}  {k}");
    }
}

fn main() -> anyhow::Result<()> {
    let a = args();
    let dir = a.root.join("catalog/recipes");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut fetcher = Fetcher { agent: ureq::Agent::new_with_defaults(), last: None, requests: 0 };
    let all = sources::all();

    if !a.build_only {
        for src in all.iter().filter(|s| a.sources.is_empty() || a.sources.contains(&(s.info)().id)) {
            let info = (src.info)();
            let r = read_source(&a, src, &mut fetcher)?;
            println!("{}: pages {}, posts {}, posts with no recipe block (skipped) {}", info.id, r.pages, r.posts, r.skipped);
            if let Some(e) = &r.failed {
                println!("{}: STOPPED, file left as it was: {e}", info.id);
                continue;
            }
            summarise(&info.id, &r.recipes);
            let path = dir.join("sources").join(format!("{}.json", info.id));
            let head = [("schema", SCHEMA.to_string()), ("id", serde_json::to_string(&info.id)?), ("name", serde_json::to_string(&info.name)?), ("homepage", serde_json::to_string(&info.homepage)?), ("credit", serde_json::to_string(&info.credit)?), ("fetchedAt", serde_json::to_string(&today)?)];
            write_lines(&path, &head, &r.recipes)?;
            println!("wrote {} ({} bytes)", path.display(), std::fs::metadata(&path)?.len());
        }
        println!("requests made: {}", fetcher.requests);
    }

    // The merge reads every source file, in the order `sources::all` lists them.
    let mut files: Vec<SourceFile> = Vec::new();
    for src in &all {
        let path = dir.join("sources").join(format!("{}.json", (src.info)().id));
        if path.exists() {
            files.push(serde_json::from_str(&std::fs::read_to_string(&path)?)?);
        }
    }
    let (catalog, report): (RecipeCatalog, _) = merge(&files, &today);
    let head = [("schema", SCHEMA.to_string()), ("builtAt", serde_json::to_string(&catalog.built_at)?), ("sources", serde_json::to_string(&catalog.sources)?)];
    let path = dir.join("catalog.json");
    write_lines(&path, &head, &catalog.recipes)?;
    println!("merge: {} publications -> {} recipes; {} published more than once, {} of them by more than one source; {} with no film simulation left out", report.publications, report.recipes, report.merged, report.merged_across_sources, report.left_out_without_sim);
    let mut by_field: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &report.near_duplicates {
        *by_field.entry(n.field).or_default() += 1;
    }
    println!("near-duplicates (one field apart, kept separate): {} pairs, by field {by_field:?}", report.near_duplicates.len());
    for n in report.near_duplicates.iter().take(15) {
        println!("  {}  ~  {}  ({})", n.a, n.b, n.field);
    }
    println!("wrote {} ({} bytes)", path.display(), std::fs::metadata(&path)?.len());
    Ok(())
}
