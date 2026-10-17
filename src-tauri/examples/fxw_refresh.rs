//! Rebuilds `catalog/recipes/fujixweekly.json` from the Fuji X Weekly RSS feed.
//!
//!     cargo run --example fxw_refresh -- [--max-page N] [--offline] [--out PATH]
//!
//! Walks /feed/?paged=1.. until the feed ends (404) or `--max-page`. Each page is cached raw
//! in `local/fxw-feed/` (gitignored), so a re-run reads the cache and fetches only new pages;
//! delete the cache to read the feed again. Requests are at least 2 s apart. `--offline` reads
//! the cache only. Only settings, names, dates, links and sensors are written; never photos or
//! post text.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ukiyo_lib::recipes::catalogue::{fxw_source, from_post, Catalogue, CatalogueRecipe, SCHEMA};

const FEED: &str = "https://fujixweekly.com/feed/";
const USER_AGENT: &str = "ukiyo-recipe-catalogue/0.1 (Fujifilm camera app; bundles recipe settings with a link to each post)";
const GAP: Duration = Duration::from_secs(2);
const CONTENT_NS: &str = "http://purl.org/rss/1.0/modules/content/";

struct Args {
    max_page: Option<u32>,
    offline: bool,
    out: PathBuf,
    cache: PathBuf,
}

fn args() -> Args {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut a = Args { max_page: None, offline: false, out: root.join("catalog/recipes/fujixweekly.json"), cache: root.join("local/fxw-feed") };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--max-page" => a.max_page = it.next().and_then(|v| v.parse().ok()),
            "--offline" => a.offline = true,
            "--out" => a.out = it.next().map(PathBuf::from).expect("--out PATH"),
            "--cache" => a.cache = it.next().map(PathBuf::from).expect("--cache DIR"),
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
    fn get(&mut self, page: u32) -> anyhow::Result<Page> {
        if let Some(t) = self.last {
            let wait = GAP.saturating_sub(t.elapsed());
            std::thread::sleep(wait);
        }
        let url = if page == 1 { FEED.to_string() } else { format!("{FEED}?paged={page}") };
        self.last = Some(Instant::now());
        self.requests += 1;
        match self.agent.get(&url).header("User-Agent", USER_AGENT).call() {
            Ok(mut r) => Ok(Page::Xml(r.body_mut().with_config().limit(20 * 1024 * 1024).read_to_string()?)),
            Err(ureq::Error::StatusCode(404)) => Ok(Page::End),
            Err(e) => Err(anyhow::anyhow!("{url}: {e}")),
        }
    }
}

struct Item {
    title: String,
    link: String,
    date: String,
    tags: Vec<String>,
    html: String,
}

fn items(xml: &str) -> anyhow::Result<Vec<Item>> {
    let doc = roxmltree::Document::parse(xml)?;
    let mut out = Vec::new();
    for item in doc.descendants().filter(|n| n.has_tag_name("item")) {
        let child = |name: &str, ns: Option<&str>| {
            item.children()
                .find(|c| c.tag_name().name() == name && c.tag_name().namespace() == ns)
                .and_then(|c| c.text())
                .unwrap_or_default()
                .to_string()
        };
        let pub_date = child("pubDate", None);
        let date = chrono::DateTime::parse_from_rfc2822(&pub_date).map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default();
        out.push(Item {
            title: child("title", None),
            link: child("link", None),
            date,
            tags: item.children().filter(|c| c.has_tag_name("category")).filter_map(|c| c.text().map(str::to_string)).collect(),
            html: child("encoded", Some(CONTENT_NS)),
        });
    }
    Ok(out)
}

/// One recipe per line, so a refresh diffs by recipe.
fn write_catalogue(path: &Path, c: &Catalogue) -> anyhow::Result<()> {
    let mut s = String::from("{\n");
    s += &format!("  \"schema\": {},\n  \"fetchedAt\": {},\n  \"source\": {},\n  \"recipes\": [\n", c.schema, serde_json::to_string(&c.fetched_at)?, serde_json::to_string(&c.source)?);
    let lines: Vec<String> = c.recipes.iter().map(|r| serde_json::to_string(r).map(|j| format!("    {j}"))).collect::<Result<_, _>>()?;
    s += &lines.join(",\n");
    s += "\n  ]\n}\n";
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, s)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let a = args();
    std::fs::create_dir_all(&a.cache)?;
    let mut fetcher = Fetcher { agent: ureq::Agent::new_with_defaults(), last: None, requests: 0 };
    let mut recipes: Vec<CatalogueRecipe> = Vec::new();
    let mut seen_urls = HashSet::new();
    let (mut posts, mut skipped, mut pages) = (0u32, 0u32, 0u32);
    let mut page = 1;
    loop {
        if a.max_page.is_some_and(|m| page > m) {
            break;
        }
        let cached = a.cache.join(format!("page-{page:03}.xml"));
        let xml = if cached.exists() {
            std::fs::read_to_string(&cached)?
        } else if a.offline {
            break;
        } else {
            match fetcher.get(page)? {
                Page::Xml(x) => {
                    std::fs::write(&cached, &x)?;
                    x
                }
                Page::End => break,
            }
        };
        let list = items(&xml)?;
        if list.is_empty() {
            break;
        }
        pages += 1;
        for it in list {
            // New posts shift the pages; a post seen on an earlier page is not counted twice.
            if !seen_urls.insert(it.link.clone()) {
                continue;
            }
            posts += 1;
            let found = from_post(&it.title, &it.link, &it.date, &it.tags, &it.html);
            if found.is_empty() {
                skipped += 1;
            }
            recipes.extend(found);
        }
        eprint!("\rpage {page} ({} recipes)", recipes.len());
        page += 1;
    }
    eprintln!();
    recipes.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.id.cmp(&b.id)));
    let catalogue = Catalogue { schema: SCHEMA, fetched_at: chrono::Local::now().format("%Y-%m-%d").to_string(), source: fxw_source(), recipes };
    write_catalogue(&a.out, &catalogue)?;

    let recipes = &catalogue.recipes;
    let warned = recipes.iter().filter(|r| !r.warnings.is_empty()).count();
    let total: usize = recipes.iter().map(|r| r.warnings.len()).sum();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_label: BTreeMap<String, usize> = BTreeMap::new();
    for w in recipes.iter().flat_map(|r| &r.warnings) {
        *by_kind.entry(format!("{:?}", w.kind)).or_default() += 1;
        let key = if w.label.is_empty() { format!("(unlabelled) {}", w.value) } else { format!("{}: {}", w.label, w.message) };
        *by_label.entry(key).or_default() += 1;
    }
    let no_sim = recipes.iter().filter(|r| r.settings.film_simulation.is_none()).count();
    let no_sensor = recipes.iter().filter(|r| r.sensor.is_none()).count();
    let mut sensors: BTreeMap<String, usize> = BTreeMap::new();
    for r in recipes {
        *sensors.entry(r.sensor.clone().unwrap_or_else(|| "(none)".into())).or_default() += 1;
    }
    println!("pages read: {pages}, requests made: {}", fetcher.requests);
    println!("posts: {posts}, posts with no recipe block (skipped): {skipped}");
    println!("recipes: {}, with warnings: {warned}, warnings: {total}", recipes.len());
    println!("recipes with no film simulation: {no_sim}, with no sensor: {no_sensor}");
    println!("sensors: {sensors:?}");
    println!("warnings by kind: {by_kind:?}");
    let mut top: Vec<_> = by_label.into_iter().collect();
    top.sort_by_key(|t| std::cmp::Reverse(t.1));
    println!("top warnings:");
    for (k, n) in top.iter().take(25) {
        println!("  {n:4}  {k}");
    }
    println!("wrote {} ({} bytes)", a.out.display(), std::fs::metadata(&a.out)?.len());
    Ok(())
}
