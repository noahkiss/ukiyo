//! Rebuilds the recipe catalogue: reads each source's feed into
//! `catalog/recipes/sources/<id>.json`, then merges them into `catalog/recipes/catalog.json`.
//!
//!     cargo run --example recipes_refresh -- [--source ID]... [--max-page N] [--offline] [--build-only]
//!
//! A feed source walks its WordPress feed (/feed/?paged=1..) until it ends (404) or
//! `--max-page`; a CSV source downloads one file. Responses are cached raw in
//! `local/recipe-feeds/<id>/` (gitignored), so a re-run reads the cache and fetches only new
//! pages; delete the folder to read a source again.
//! Requests are at least 2 s apart. A 429, a 5xx or any other failure stops that source and
//! leaves its file as it was. `--offline` reads the caches only; `--build-only` skips the
//! feeds and merges the source files. Only settings, names, dates, authors, links and sensors
//! are written; never photos or post text.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ukiyo_core::recipes::catalogue::{canonical_url, merge, RecipeCatalog, RedditCount, SourceFile, SourceRecipe, SCHEMA};
use ukiyo_core::recipes::sources::{self, FeedItem, Fetch, Source};

const USER_AGENT: &str = "ukiyo-recipe-catalogue/0.1 (Fujifilm camera app; bundles recipe settings with a link to each post)";
const GAP: Duration = Duration::from_secs(2);
const CONTENT_NS: &str = "http://purl.org/rss/1.0/modules/content/";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const SLASH_NS: &str = "http://purl.org/rss/1.0/modules/slash/";

static HREF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r#"href="(https?://[^"]+)""#).unwrap());

struct Args {
    sources: Vec<String>,
    max_page: Option<u32>,
    offline: bool,
    build_only: bool,
    root: PathBuf,
}

fn args() -> Args {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
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
            comments: child("comments", Some(SLASH_NS)).trim().parse().ok(),
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
    /// Each post read and the links in it, for mentions.
    hrefs: Vec<(String, Vec<String>)>,
    pages: u32,
    posts: u32,
    skipped: u32,
    failed: Option<String>,
}

fn read_source(a: &Args, src: &Source, fetcher: &mut Fetcher) -> anyhow::Result<Read> {
    let id = (src.info)().id;
    let cache = a.root.join("local/recipe-feeds").join(&id);
    std::fs::create_dir_all(&cache)?;
    let mut r = Read { recipes: Vec::new(), hrefs: Vec::new(), pages: 0, posts: 0, skipped: 0, failed: None };
    let (feed_page, recipes) = match src.fetch {
        Fetch::Feed { page, recipes } => (page, recipes),
        Fetch::Csv { url, recipes } => {
            let cached = cache.join("recipes.csv");
            let text = if cached.exists() {
                std::fs::read_to_string(&cached)?
            } else if a.offline {
                return Ok(r);
            } else {
                match fetcher.get(url) {
                    Ok(Page::Xml(t)) => {
                        std::fs::write(&cached, &t)?;
                        t
                    }
                    Ok(Page::End) => return Ok(Read { failed: Some(format!("{url}: 404")), ..r }),
                    Err(e) => return Ok(Read { failed: Some(e.to_string()), ..r }),
                }
            };
            r.pages = 1;
            r.recipes = recipes(&text);
            r.posts = r.recipes.len() as u32;
            r.recipes.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.id.cmp(&b.id)));
            return Ok(r);
        }
    };
    let mut seen = HashSet::new();
    let mut page = 1;
    while a.max_page.is_none_or(|m| page <= m) {
        let cached = cache.join(format!("page-{page:03}.xml"));
        let xml = if cached.exists() {
            std::fs::read_to_string(&cached)?
        } else if a.offline {
            break;
        } else {
            match fetcher.get(&feed_page(page)) {
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
            r.hrefs.push((canonical_url(&it.link), HREF.captures_iter(&it.html).map(|c| canonical_url(&c[1])).collect()));
            let found = recipes(&it);
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
        // Every source is read, from its cache when it is not being fetched, so mentions count
        // links from all of them.
        let mut reads = Vec::new();
        for src in &all {
            let info = (src.info)();
            let fetch = a.sources.is_empty() || a.sources.contains(&info.id);
            let opts = Args { offline: a.offline || !fetch, sources: Vec::new(), max_page: a.max_page, build_only: false, root: a.root.clone() };
            let r = read_source(&opts, src, &mut fetcher)?;
            println!("{}: pages {}, posts {}, posts with no recipe block (skipped) {}", info.id, r.pages, r.posts, r.skipped);
            reads.push((info, r));
        }
        let recipe_urls: HashSet<String> = reads.iter().flat_map(|(_, r)| r.recipes.iter().map(|x| canonical_url(&x.url))).collect();
        let mut mentioned: BTreeMap<String, HashSet<String>> = BTreeMap::new();
        let mut out_links: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (_, r) in &reads {
            for (post, hrefs) in &r.hrefs {
                let mut mine: Vec<String> = hrefs.iter().filter(|h| *h != post && recipe_urls.contains(*h)).cloned().collect();
                mine.sort();
                mine.dedup();
                for h in hrefs.iter().filter(|h| *h != post) {
                    mentioned.entry(h.clone()).or_default().insert(post.clone());
                }
                out_links.insert(post.clone(), mine);
            }
        }
        for (info, mut r) in reads {
            if let Some(e) = &r.failed {
                println!("{}: STOPPED, file left as it was: {e}", info.id);
                continue;
            }
            if r.pages == 0 {
                println!("{}: no cache and not fetched; file left as it was", info.id);
                continue;
            }
            for x in &mut r.recipes {
                let u = canonical_url(&x.url);
                x.mentions = mentioned.get(&u).map_or(0, |s| s.len() as u32);
                x.links = out_links.get(&u).cloned().unwrap_or_default();
            }
            summarise(&info.id, &r.recipes);
            let path = dir.join("sources").join(format!("{}.json", info.id));
            let head = [("schema", SCHEMA.to_string()), ("id", serde_json::to_string(&info.id)?), ("name", serde_json::to_string(&info.name)?), ("homepage", serde_json::to_string(&info.homepage)?), ("credit", serde_json::to_string(&info.credit)?), ("compilation", info.compilation.to_string()), ("fetchedAt", serde_json::to_string(&today)?)];
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
    // Reddit counts come from a one-off search (docs/recipes.md); absent, they are left out.
    let reddit_path = dir.join("reddit.json");
    let reddit: BTreeMap<String, RedditCount> = if reddit_path.exists() {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&reddit_path)?)?;
        serde_json::from_value(v["counts"].clone())?
    } else {
        BTreeMap::new()
    };
    let (catalog, report): (RecipeCatalog, _) = merge(&files, &today, &reddit);
    let families: Vec<String> = catalog.families.iter().map(|f| serde_json::to_string(f).map(|j| format!("    {j}"))).collect::<Result<_, _>>()?;
    let families = if families.is_empty() { "[]".to_string() } else { format!("[\n{}\n  ]", families.join(",\n")) };
    let head = [("schema", SCHEMA.to_string()), ("builtAt", serde_json::to_string(&catalog.built_at)?), ("sources", serde_json::to_string(&catalog.sources)?), ("families", families)];
    let path = dir.join("catalog.json");
    write_lines(&path, &head, &catalog.recipes)?;
    println!("merge: {} publications -> {} recipes; {} published more than once, {} of them by more than one source; {} with no film simulation left out; {} compilation rows folded into the post they link to", report.publications, report.recipes, report.merged, report.merged_across_sources, report.left_out_without_sim, report.matched_by_link);
    let mut by_field: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &report.near_duplicates {
        *by_field.entry(n.field).or_default() += 1;
    }
    println!("near-duplicates (one field apart, kept separate): {} pairs, by field {by_field:?}", report.near_duplicates.len());
    for n in report.near_duplicates.iter().take(15) {
        println!("  {}  ~  {}  ({})", n.a, n.b, n.field);
    }
    let in_families: usize = catalog.families.iter().map(|f| f.variants.len()).sum();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for j in &report.family_joins {
        *reasons.entry(format!("{:?}", j.2)).or_default() += 1;
    }
    println!("families: {} holding {in_families} recipes; joins by reason {reasons:?}", catalog.families.len());
    let names: BTreeMap<&str, &str> = catalog.recipes.iter().map(|r| (r.id.as_str(), r.name.as_str())).collect();
    println!("joins on near-duplicate settings alone (check these):");
    for (a, b, reason, diff) in &report.family_joins {
        if format!("{reason:?}") == "NearDuplicate" {
            println!("  {} | {}  ({})", names.get(a.as_str()).unwrap_or(&""), names.get(b.as_str()).unwrap_or(&""), diff.join(", "));
        }
    }
    let mut ranked: Vec<(&str, f64, u32, u32)> = catalog.families.iter().map(|f| (f.title.as_str(), f.popularity.score, f.popularity.comments, f.popularity.mentions)).collect();
    let in_fam: HashSet<&str> = catalog.families.iter().flat_map(|f| f.variants.iter().map(|v| v.recipe.as_str())).collect();
    ranked.extend(catalog.recipes.iter().filter(|r| !in_fam.contains(r.id.as_str())).map(|r| (r.name.as_str(), r.popularity.score, r.popularity.comments, r.popularity.mentions)));
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let with_reddit = catalog.families.iter().filter(|f| f.popularity.reddit.as_ref().is_some_and(|r| r.threads > 0)).count() + catalog.recipes.iter().filter(|r| !in_fam.contains(r.id.as_str()) && r.popularity.reddit.as_ref().is_some_and(|x| x.threads > 0)).count();
    println!("cards: {}; with comments {}, with mentions {}, with Reddit threads {with_reddit}", ranked.len(), ranked.iter().filter(|x| x.2 > 0).count(), ranked.iter().filter(|x| x.3 > 0).count());
    println!("top 20 by score:");
    for (name, score, c, m) in ranked.iter().take(20) {
        println!("  {score:6.2}  {name}  (comments {c}, mentions {m})");
    }
    println!("wrote {} ({} bytes)", path.display(), std::fs::metadata(&path)?.len());
    Ok(())
}
