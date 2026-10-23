//! `recipe_parse {url}`: one recipe post read from the web, then the same parser as pasted
//! text. A post on a catalogue site goes through that site's adapter (`sources`), so its
//! name, sensor and author come out as in the catalogue; a post anywhere else gets the
//! generic reading: every recipe block in the page (`html::blocks`).
//!
//! Only public hosts are read. A link to this machine or its network (loopback, private,
//! link-local, a name that resolves to one) is refused, at the first hop and at each
//! redirect, so an agent cannot read a local page through the import.

use super::catalogue::SourceRecipe;
use super::sources::{self, FeedItem, Fetch};
use super::{html, post, OtherRecipe, Parsed};
use regex::Regex;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::LazyLock;
use std::time::Duration;

const USER_AGENT: &str = concat!("ukiyo/", env!("CARGO_PKG_VERSION"), " (recipe import; one page per request)");
/// The largest page read.
const MAX_BYTES: u64 = 8 << 20;
/// Redirects followed, each checked like the first link.
const MAX_REDIRECTS: usize = 5;

static OG_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<meta[^>]+property=["']og:title["'][^>]*content=["']([^"']*)["']"#).unwrap());
static TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());
static PUBLISHED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<meta[^>]+property=["']article:published_time["'][^>]*content=["'](\d{4}-\d{2}-\d{2})"#).unwrap());
static ARTICLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<article\b.*?</article>").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<meta[^>]+property=["']article:tag["'][^>]*content=["']([^"']*)["']"#).unwrap());

/// Why a page could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Not an http or https URL, or not a public host.
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

/// The host of an http(s) URL as written, lowercased, without the port.
fn host_as_written(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("https://").or_else(|| url.trim().strip_prefix("http://"))?;
    let h = rest.split(['/', '?', '#']).next()?.split('@').next_back()?;
    let h = if h.starts_with('[') { h.split(']').next()?.trim_start_matches('[') } else { h.split(':').next()? };
    let h = h.to_lowercase();
    (!h.is_empty()).then_some(h)
}

/// The host of an http(s) URL, without `www.`.
pub fn host(url: &str) -> Option<String> {
    host_as_written(url).map(|h| h.strip_prefix("www.").unwrap_or(&h).to_string())
}

/// An address outside this machine and its networks.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            // 100.64/10 is carrier NAT and the tailnet.
            !(v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast() || (o[0] == 100 && (64..128).contains(&o[1])))
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public(IpAddr::V4(v4)),
            // fc00::/7 unique local, fe80::/10 link local.
            None => !(v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80),
        },
    }
}

/// Refuses a host that is this machine or on its network, by name or by what it resolves to.
fn public_host(h: &str) -> Result<(), FetchError> {
    let local = || FetchError::BadUrl(format!("{h} is not a public host; the import reads public pages only"));
    if h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") || h.ends_with(".internal") {
        return Err(local());
    }
    let addrs: Vec<IpAddr> = match h.parse::<IpAddr>() {
        Ok(ip) => vec![ip],
        Err(_) => (h, 443).to_socket_addrs().map_err(|e| FetchError::Failed(format!("{h}: {e}")))?.map(|a| a.ip()).collect(),
    };
    if addrs.is_empty() || addrs.into_iter().any(|ip| !is_public(ip)) {
        return Err(local());
    }
    Ok(())
}

/// The page at `url`, following at most [`MAX_REDIRECTS`] redirects. `allow_local` skips the
/// public-host check: tests only.
fn fetch(url: &str, allow_local: bool) -> Result<String, FetchError> {
    let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).max_redirects(0).max_redirects_will_error(false).http_status_as_error(false).build().new_agent();
    let mut url = url.trim().to_string();
    for _ in 0..=MAX_REDIRECTS {
        let h = host_as_written(&url).ok_or_else(|| FetchError::BadUrl(format!("{url:?} is not an http or https link")))?;
        if !allow_local {
            public_host(&h)?;
        }
        let mut resp = agent.get(&url).header("User-Agent", USER_AGENT).call().map_err(|e| FetchError::Failed(format!("{url}: {e}")))?;
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            let Some(to) = resp.headers().get("location").and_then(|l| l.to_str().ok()) else {
                return Err(FetchError::Failed(format!("{url}: {status} without a location")));
            };
            url = match to {
                t if t.starts_with("http://") || t.starts_with("https://") => t.to_string(),
                t if t.starts_with("//") => format!("{}:{t}", url.split("://").next().unwrap_or("https")),
                t if t.starts_with('/') => format!("{}{t}", url.split('/').take(3).collect::<Vec<_>>().join("/")),
                t => return Err(FetchError::Failed(format!("{url}: redirect to {t:?}"))),
            };
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(FetchError::Failed(format!("{url}: status {status}")));
        }
        return resp.body_mut().with_config().limit(MAX_BYTES).read_to_string().map_err(|e| FetchError::Failed(format!("{url}: {e}")));
    }
    Err(FetchError::Failed(format!("{url}: more than {MAX_REDIRECTS} redirects")))
}

/// Reads the page and parses it. Blocks for at most 20 s.
pub fn parse_url(url: &str) -> Result<Parsed, FetchError> {
    let url = url.trim();
    if host(url).is_none() {
        return Err(FetchError::BadUrl(format!("{url:?} is not an http or https link")));
    }
    let body = fetch(url, false)?;
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
    use std::io::{Read as _, Write as _};

    const PAGE: &str = r#"<html><head><title>Kodak Portra 400 &#8212; Fujifilm X-T5 (X-Trans V) Film Simulation Recipe | Somewhere</title>
<meta property="article:published_time" content="2026-03-01T10:00:00+00:00"></head>
<body><nav><p>Film Simulation: Velvia</p></nav><article><h1>Kodak Portra 400</h1>
<p>Film Simulation: Classic Chrome<br>Dynamic Range: DR400<br>Grain Effect: Weak, Small<br>White Balance: 5200K, +2 Red &amp; -4 Blue<br>Highlight: -1<br>Shadow: +0.5<br>ISO: Auto, up to ISO 6400</p>
</article></body></html>"#;

    /// A local server that answers one request with `head` and `body`, and the port it is on.
    fn serve_once(head: &'static str, body: &'static str) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = write!(s, "{head}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        port
    }

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
        assert_eq!(host_as_written("https://www.example.org:8443/x").as_deref(), Some("www.example.org"));
        assert_eq!(host_as_written("http://[::1]:4823/x").as_deref(), Some("::1"));
        assert_eq!(host("ftp://x"), None);
        assert!(matches!(parse_url("not a link"), Err(FetchError::BadUrl(_))));
        let p = parse_page("https://example.org/", "<html><title>No recipe here</title><p>Nothing.</p></html>");
        assert_eq!(p.settings, super::super::Settings::default());
    }

    /// The whole path over HTTP, through a redirect: local servers, so no test touches the
    /// network.
    #[test]
    fn a_url_is_fetched_through_a_redirect_and_parsed() {
        let page = serve_once("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n", PAGE);
        let head: &'static str = Box::leak(format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{page}/portra\r\n").into_boxed_str());
        let first = serve_once(head, "");
        let p = parse_page(&format!("http://127.0.0.1:{first}/go"), &fetch(&format!("http://127.0.0.1:{first}/go"), true).unwrap());
        assert_eq!(p.settings.dynamic_range.as_deref(), Some("DR400"));
    }

    /// This machine and its networks are never read, whatever the link says.
    #[test]
    fn local_hosts_are_refused() {
        let port = serve_once("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n", PAGE);
        for url in [format!("http://127.0.0.1:{port}/x"), "http://localhost/x".into(), "http://10.0.0.1/x".into(), "http://192.168.1.1/x".into(), "http://169.254.169.254/latest/meta-data/".into(), "http://100.100.100.100/x".into(), "http://[::1]/x".into(), "http://[fd7a:115c:a1e0::1]/x".into(), "http://printer.local/x".into()] {
            match parse_url(&url) {
                Err(FetchError::BadUrl(m)) => assert!(m.contains("not a public host"), "{url}: {m}"),
                other => panic!("{url}: {other:?}"),
            }
        }
        assert!(is_public("93.184.216.34".parse().unwrap()) && is_public("2606:4700::1".parse().unwrap()));
        assert!(!is_public("::ffff:127.0.0.1".parse().unwrap()) && !is_public("fe80::1".parse().unwrap()));
    }
}
