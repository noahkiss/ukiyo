//! `recipe_parse {url}`: one recipe post read from the web, then the same parser as pasted
//! text. A post on a catalogue site goes through that site's adapter (`sources`), so its
//! name, sensor and author come out as in the catalogue; a post anywhere else gets the
//! generic reading: every recipe block in the page (`html::blocks`).

use super::catalogue::SourceRecipe;
use super::sources::{self, FeedItem, Fetch};
use super::{html, post, OtherRecipe, Parsed};
use regex::Regex;
use std::sync::LazyLock;
use std::time::Duration;

const USER_AGENT: &str = concat!("ukiyo/", env!("CARGO_PKG_VERSION"), " (recipe import; one page per request)");
/// The largest page read.
const MAX_BYTES: u64 = 8 << 20;

static OG_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<meta[^>]+property=["']og:title["'][^>]*content=["']([^"']*)["']"#).unwrap());
static TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());
static PUBLISHED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<meta[^>]+property=["']article:published_time["'][^>]*content=["'](\d{4}-\d{2}-\d{2})"#).unwrap());
static ARTICLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<article\b.*?</article>").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<meta[^>]+property=["']article:tag["'][^>]*content=["']([^"']*)["']"#).unwrap());

/// Why a page could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Not an http or https URL.
    BadUrl(String),
    /// No answer, a non-2xx status, or not text.
    Failed(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::BadUrl(m) | FetchError::Failed(m) => f.write_str(m),
        }
    }
}

/// The host of an http(s) URL, without `www.`.
pub fn host(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("https://").or_else(|| url.trim().strip_prefix("http://"))?;
    let h = rest.split(['/', '?', '#']).next()?.split('@').next_back()?;
    let h = h.split(':').next()?.to_lowercase();
    (!h.is_empty()).then(|| h.strip_prefix("www.").unwrap_or(&h).to_string())
}

/// Reads the page and parses it. Blocks for at most 20 s.
pub fn parse_url(url: &str) -> Result<Parsed, FetchError> {
    let url = url.trim();
    if host(url).is_none() {
        return Err(FetchError::BadUrl(format!("{url:?} is not an http or https link")));
    }
    let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build().new_agent();
    let mut resp = agent.get(url).header("User-Agent", USER_AGENT).call().map_err(|e| FetchError::Failed(format!("{url}: {e}")))?;
    let body = resp.body_mut().with_config().limit(MAX_BYTES).read_to_string().map_err(|e| FetchError::Failed(format!("{url}: {e}")))?;
    Ok(parse_page(url, &body))
}

fn first(re: &Regex, html: &str) -> Option<String> {
    re.captures(html).map(|c| html::decode_entities(c[1].trim()))
}

/// The recipes in one post page. The first is the result; the rest are `others`.
pub fn parse_page(url: &str, page: &str) -> Parsed {
    let title = first(&OG_TITLE, page).or_else(|| first(&TITLE, page)).unwrap_or_default();
    let tags: Vec<String> = TAG.captures_iter(page).map(|c| html::decode_entities(&c[1])).collect();
    // The post body: the <article> when the page has one, so sidebars stay out.
    let body = ARTICLE.find(page).map(|m| m.as_str()).unwrap_or(page);
    let item = FeedItem {
        title: title.clone(),
        link: url.to_string(),
        date: first(&PUBLISHED, page).unwrap_or_default(),
        creator: String::new(),
        comments: None,
        tags,
        html: body.to_string(),
    };
    let h = host(url).unwrap_or_default();
    let site = sources::all().into_iter().find(|s| host(&(s.info)().homepage).as_deref() == Some(h.as_str()));
    let recipes: Vec<SourceRecipe> = match site.map(|s| s.fetch) {
        Some(Fetch::Feed { recipes, .. }) => recipes(&item),
        _ => {
            let sensor = post::sensor(&item.title, &item.tags).or_else(|| post::sensor_in_text(&html::to_text(&item.html)));
            let name = if title.is_empty() { "Imported recipe".to_string() } else { post::recipe_name(&title) };
            sources::post_recipes(&item, name, sensor, None)
        }
    };
    let mut it = recipes.into_iter();
    let Some(r) = it.next() else {
        return Parsed { name: (!title.is_empty()).then(|| post::recipe_name(&title)), source_url: Some(url.into()), ..Parsed::default() };
    };
    Parsed {
        settings: r.settings,
        notes: r.notes,
        warnings: r.warnings,
        alternatives: vec![],
        sensor: r.sensor,
        name: Some(r.name),
        source_url: Some(url.into()),
        author: r.author,
        others: it.map(|o| OtherRecipe { name: o.name, sensor: o.sensor, settings: o.settings, notes: o.notes, warnings: o.warnings }).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><head><title>Kodak Portra 400 &#8212; Fujifilm X-T5 (X-Trans V) Film Simulation Recipe | Somewhere</title>
<meta property="article:published_time" content="2026-03-01T10:00:00+00:00"></head>
<body><nav><p>Film Simulation: Velvia</p></nav><article><h1>Kodak Portra 400</h1>
<p>Film Simulation: Classic Chrome<br>Dynamic Range: DR400<br>Grain Effect: Weak, Small<br>White Balance: 5200K, +2 Red &amp; -4 Blue<br>Highlight: -1<br>Shadow: +0.5<br>ISO: Auto, up to ISO 6400</p>
</article></body></html>"#;

    #[test]
    fn a_page_on_any_host_is_read_from_its_article() {
        let p = parse_page("https://example.org/2026/03/portra/", PAGE);
        assert_eq!(p.settings.film_simulation.as_deref(), Some("Classic Chrome"), "the article, not the nav");
        assert_eq!(p.settings.color_temp, Some(5200));
        assert_eq!(p.settings.wb_shift_b, Some(-4));
        assert_eq!(p.sensor.as_deref(), Some("X-Trans V"));
        assert!(p.name.as_deref().is_some_and(|n| n.starts_with("Kodak Portra 400")), "{:?}", p.name);
        assert_eq!(p.source_url.as_deref(), Some("https://example.org/2026/03/portra/"));
        assert_eq!(p.notes.len(), 1);
    }

    #[test]
    fn a_catalogue_site_goes_through_its_adapter() {
        let p = parse_page("https://fujixweekly.com/2026/03/01/portra/", PAGE);
        assert_eq!(p.author.as_deref(), Some("Ritchie Roesch"));
        assert_eq!(p.settings.film_simulation.as_deref(), Some("Classic Chrome"));
    }

    #[test]
    fn hosts_and_bad_links() {
        assert_eq!(host("https://www.FujiXWeekly.com/a/b?c").as_deref(), Some("fujixweekly.com"));
        assert_eq!(host("http://127.0.0.1:4823/x").as_deref(), Some("127.0.0.1"));
        assert_eq!(host("ftp://x"), None);
        assert!(matches!(parse_url("not a link"), Err(FetchError::BadUrl(_))));
        let p = parse_page("https://example.org/", "<html><title>No recipe here</title><p>Nothing.</p></html>");
        assert_eq!(p.settings, super::super::Settings::default());
    }

    /// The whole path over HTTP: a local server, so no test touches the network.
    #[test]
    fn a_url_is_fetched_and_parsed() {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PAGE}", PAGE.len());
            }
        });
        let p = parse_url(&format!("http://127.0.0.1:{port}/portra")).unwrap();
        assert_eq!(p.settings.dynamic_range.as_deref(), Some("DR400"));
    }
}
