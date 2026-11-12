//! Mind OS link fetcher and thumbnails (no network in the pure parts, so they are testable).
//!
//! - [`fetch_page`]: a link's title, description, image, site name and readable text. Checks
//!   robots.txt first, 15 s timeout, 3 MB cap, http(s) only, no local or private hosts.
//! - [`thumbnail`]: a <= 512 px JPEG and the dominant colour, for cards and for Claude.

use std::time::Duration;

use aos_core::mind::jobs::Fetched;

pub const USER_AGENT: &str = "AgenticOS-Mind/1 (personal library; respects robots.txt)";
const MAX_PAGE: usize = 3 * 1024 * 1024;
pub const MAX_IMAGE: usize = 12 * 1024 * 1024;
pub const THUMB_PX: u32 = 512;

fn client(timeout: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .connect_timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())
}

/// Refuse obvious local and private hosts (the server must not become a proxy into the LAN).
pub fn allowed_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http(s) links".into());
    }
    let host = url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or("");
    let host = if host.starts_with('[') { host.split(']').next().unwrap_or("").trim_start_matches('[') } else { host.split(':').next().unwrap_or("") };
    let h = host.to_ascii_lowercase();
    let private = h.is_empty()
        || h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || match h.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast(),
            Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback() || ip.is_unspecified() || (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80,
            Err(_) => false,
        };
    if private { Err("local or private address".into()) } else { Ok(()) }
}

/// Does robots.txt let us read `path`? Groups for `*` (or our agent) apply; the longest
/// matching rule wins, Allow on a tie. `*` and `$` in rules are honoured simply.
pub fn robots_allows(robots: &str, path: &str) -> bool {
    let mut applies = false;
    let mut in_agents = false;
    let mut best: Option<(usize, bool)> = None;
    for line in robots.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        match k.as_str() {
            "user-agent" => {
                if !in_agents {
                    applies = false;
                }
                in_agents = true;
                let ua = v.to_ascii_lowercase();
                if ua == "*" || ua.contains("agenticos") {
                    applies = true;
                }
            }
            "allow" | "disallow" => {
                in_agents = false;
                if !applies || v.is_empty() {
                    continue;
                }
                if rule_matches(v, path) {
                    let allow = k == "allow";
                    let len = v.len();
                    if best.is_none_or(|(l, a)| len > l || (len == l && allow && !a)) {
                        best = Some((len, allow));
                    }
                }
            }
            _ => in_agents = false,
        }
    }
    best.is_none_or(|(_, allow)| allow)
}

fn rule_matches(rule: &str, path: &str) -> bool {
    let anchored = rule.ends_with('$');
    let rule = rule.trim_end_matches('$');
    let parts: Vec<&str> = rule.split('*').collect();
    let mut pos = 0;
    for (i, p) in parts.iter().enumerate() {
        if i == 0 {
            if !path.starts_with(p) {
                return false;
            }
            pos = p.len();
        } else if let Some(at) = path[pos..].find(p) {
            pos += at + p.len();
        } else {
            return false;
        }
    }
    !anchored || pos == path.len() || rule.ends_with('*')
}

/// Fetch a link and read it. An image link returns just `image_url`.
pub async fn fetch_page(url: &str) -> Result<Fetched, String> {
    allowed_url(url)?;
    let c = client(15)?;
    // robots.txt: a missing or unreadable file allows everything.
    if let Some(origin) = origin_of(url) {
        let path = url[origin.len()..].to_string();
        let path = if path.is_empty() { "/".to_string() } else { path };
        if let Ok(r) = client(5)?.get(format!("{origin}/robots.txt")).send().await
            && r.status().is_success()
            && let Ok(text) = r.text().await
            && !robots_allows(&text, &path)
        {
            return Err("robots.txt asks robots not to read this page; saved the link only".into());
        }
    }
    let resp = c.get(url).header("Accept", "text/html,application/xhtml+xml;q=0.9,image/*;q=0.8,*/*;q=0.5").send().await.map_err(|e| format!("couldn't open the link: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("the site answered {status}"));
    }
    let final_url = resp.url().to_string();
    let ctype = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    if ctype.starts_with("image/") {
        return Ok(Fetched { image_url: Some(final_url.clone()), url: Some(final_url), ..Default::default() });
    }
    if !(ctype.contains("html") || ctype.is_empty()) {
        return Ok(Fetched { url: Some(final_url), ..Default::default() });
    }
    let bytes = read_capped(resp, MAX_PAGE).await?;
    let html = String::from_utf8_lossy(&bytes);
    let mut f = parse_html(&html, &final_url);
    f.url = Some(final_url);
    Ok(f)
}

async fn read_capped(mut resp: reqwest::Response, max: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        out.extend_from_slice(&chunk);
        if out.len() > max {
            out.truncate(max);
            break;
        }
    }
    Ok(out)
}

/// Download an image (<= 12 MB). Returns the bytes and the type the server named.
pub async fn fetch_image(url: &str) -> Result<(Vec<u8>, String), String> {
    allowed_url(url)?;
    let resp = client(20)?.get(url).send().await.map_err(|e| format!("couldn't download the image: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("the image host answered {}", resp.status()));
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_IMAGE) {
        return Err("image over 12 MB".into());
    }
    let mime = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).and_then(|v| v.split(';').next()).unwrap_or("").trim().to_ascii_lowercase();
    let bytes = read_capped(resp, MAX_IMAGE + 1).await?;
    if bytes.len() > MAX_IMAGE {
        return Err("image over 12 MB".into());
    }
    Ok((bytes, mime))
}

fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    Some(format!("{scheme}://{host}"))
}

/// Resolve `href` against the page URL.
pub fn absolute(base: &str, href: &str) -> String {
    let href = href.trim();
    if href.starts_with("https://") || href.starts_with("http://") {
        return href.to_string();
    }
    if let Some(rest) = href.strip_prefix("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return format!("{scheme}://{rest}");
    }
    let Some(origin) = origin_of(base) else { return href.to_string() };
    if href.starts_with('/') {
        return format!("{origin}{href}");
    }
    let path = base[origin.len()..].split(['?', '#']).next().unwrap_or("/");
    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    format!("{origin}{dir}/{href}")
}

/// Decode the HTML entities that show up in titles and text.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(';').filter(|&e| e <= 10);
        let decoded = end.and_then(|e| {
            let name = &tail[1..e];
            let ch = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some(' '),
                "mdash" => Some('—'),
                "ndash" => Some('–'),
                "hellip" => Some('…'),
                "rsquo" => Some('\u{2019}'),
                "lsquo" => Some('\u{2018}'),
                "rdquo" => Some('\u{201D}'),
                "ldquo" => Some('\u{201C}'),
                n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32),
                n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            ch.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &tail[e + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The value of attribute `name` inside one tag's text (`<meta property="og:x" content="...">`).
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let after = lower[at + name.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let vstart = tag.len() - after.len() + 1;
            let v = tag[vstart..].trim_start();
            let (q, body) = match v.chars().next() {
                Some(c @ ('"' | '\'')) => (Some(c), &v[1..]),
                _ => (None, v),
            };
            let end = match q {
                Some(q) => body.find(q).unwrap_or(body.len()),
                None => body.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(body.len()),
            };
            return Some(decode_entities(body[..end].trim()));
        }
        from = at + name.len();
    }
    None
}

/// Remove `<name ...>...</name>` blocks (case-insensitive) from `html`.
fn drop_blocks(html: &str, names: &[&str]) -> String {
    let mut s = html.to_string();
    for n in names {
        let mut out = String::with_capacity(s.len());
        let lower = s.to_ascii_lowercase();
        let (open, close) = (format!("<{n}"), format!("</{n}>"));
        let mut pos = 0;
        while let Some(i) = lower[pos..].find(&open) {
            let at = pos + i;
            // `<nav` must not match `<navigation-x>` style custom tags.
            let next = lower.as_bytes().get(at + open.len()).copied().unwrap_or(b'>');
            if !(next == b'>' || next.is_ascii_whitespace() || next == b'/') {
                out.push_str(&s[pos..at + open.len()]);
                pos = at + open.len();
                continue;
            }
            out.push_str(&s[pos..at]);
            pos = match lower[at..].find(&close) {
                Some(j) => at + j + close.len(),
                None => s.len(),
            };
        }
        out.push_str(&s[pos..]);
        s = out;
    }
    s
}

/// The inside of the first `<name>` element, if any.
fn inner_of<'a>(html: &'a str, lower: &str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}");
    let mut from = 0;
    loop {
        let i = from + lower[from..].find(&open)?;
        let next = lower.as_bytes().get(i + open.len()).copied().unwrap_or(b'>');
        if next == b'>' || next.is_ascii_whitespace() {
            let start = i + lower[i..].find('>')? + 1;
            let end = lower[start..].find(&format!("</{name}>")).map(|e| start + e).unwrap_or(html.len());
            return Some(&html[start..end]);
        }
        from = i + open.len();
    }
}

/// Plain text from HTML: block tags become line breaks, the rest is stripped.
pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find('>').unwrap_or(tail.len() - 1);
        let tag = tail[1..end].trim_start_matches('/').to_ascii_lowercase();
        let name: String = tag.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if matches!(name.as_str(), "p" | "br" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "tr" | "section" | "article" | "pre" | "figcaption") {
            out.push('\n');
        }
        rest = &tail[(end + 1).min(tail.len())..];
    }
    out.push_str(rest);
    decode_entities(&out)
}

/// Read a page's meta tags and its readable text.
pub fn parse_html(html: &str, url: &str) -> Fetched {
    let lower = html.to_ascii_lowercase();
    let head_end = lower.find("<body").unwrap_or(html.len().min(200_000));
    let head = &html[..head_end];
    let mut f = Fetched::default();
    let (mut desc, mut tw_title, mut tw_desc, mut tw_img) = (None, None, None, None);
    let hl = head.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(i) = hl[pos..].find("<meta") {
        let at = pos + i;
        let end = hl[at..].find('>').map(|e| at + e).unwrap_or(head.len());
        let tag = &head[at..end];
        let key = attr(tag, "property").or_else(|| attr(tag, "name")).unwrap_or_default().to_ascii_lowercase();
        if let Some(v) = attr(tag, "content").filter(|v| !v.is_empty()) {
            match key.as_str() {
                "og:title" => f.title = f.title.or(Some(v)),
                "og:description" => f.description = f.description.or(Some(v)),
                "og:image" | "og:image:url" | "og:image:secure_url" => f.image_url = f.image_url.or(Some(absolute(url, &v))),
                "og:site_name" => f.site_name = f.site_name.or(Some(v)),
                "og:type" => f.video = v.to_ascii_lowercase().starts_with("video"),
                "description" => desc = desc.or(Some(v)),
                "twitter:title" => tw_title = tw_title.or(Some(v)),
                "twitter:description" => tw_desc = tw_desc.or(Some(v)),
                "twitter:image" | "twitter:image:src" => tw_img = tw_img.or(Some(absolute(url, &v))),
                _ => {}
            }
        }
        pos = end.max(at + 5);
    }
    if f.title.is_none() {
        f.title = tw_title.or_else(|| inner_of(head, &hl, "title").map(|t| decode_entities(t.trim())).filter(|t| !t.is_empty()));
    }
    f.description = f.description.or(tw_desc).or(desc);
    f.image_url = f.image_url.or(tw_img);
    f.title = f.title.map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "));

    // Readable text: the article (or main, or body) without scripts, menus and chrome.
    let body = inner_of(html, &lower, "article").or_else(|| inner_of(html, &lower, "main")).or_else(|| inner_of(html, &lower, "body")).unwrap_or("");
    let body = drop_blocks(body, &["script", "style", "noscript", "nav", "header", "footer", "aside", "form", "svg", "template", "iframe", "button"]);
    let text = strip_tags(&body);
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| l.chars().count() >= 40)
        .collect();
    let joined = lines.join("\n\n");
    f.body_text = (!joined.is_empty()).then(|| joined.chars().take(60_000).collect());
    f
}

/// A <= 512 px JPEG of an image, its size, and its dominant colour (`#rrggbb`).
pub fn thumbnail(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32, String), String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("can't read this image: {e}"))?;
    let (w, h) = (img.width(), img.height());
    let small = if w > THUMB_PX || h > THUMB_PX { img.thumbnail(THUMB_PX, THUMB_PX) } else { img };
    let rgb = small.to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .encode_image(&rgb)
        .map_err(|e| format!("couldn't make the thumbnail: {e}"))?;
    Ok((out, w, h, dominant(&rgb)))
}

/// Average colour weighted towards saturated pixels, so a product on white reads as its colour.
fn dominant(img: &image::RgbImage) -> String {
    let (mut r, mut g, mut b, mut wsum) = (0f64, 0f64, 0f64, 0f64);
    let step = ((img.width() * img.height()) / 4096).max(1) as usize;
    for p in img.pixels().step_by(step) {
        let [pr, pg, pb] = p.0.map(|c| c as f64);
        let max = pr.max(pg).max(pb);
        let min = pr.min(pg).min(pb);
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        let w = 0.05 + sat * sat;
        r += pr * w;
        g += pg * w;
        b += pb * w;
        wsum += w;
    }
    if wsum == 0.0 {
        return "#808080".into();
    }
    format!("#{:02x}{:02x}{:02x}", (r / wsum) as u8, (g / wsum) as u8, (b / wsum) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_meta_and_text() {
        let html = r#"<html><head><title>Fallback &amp; title</title>
            <meta property="og:title" content="The &quot;Real&quot; Title">
            <meta name="description" content="A page about chairs.">
            <meta property='og:image' content='/img/chair.jpg'>
            <meta property="og:site_name" content="Example">
            </head><body><nav>Home About Shop Contact Login and more menu words here</nav>
            <article><h1>Chairs</h1><p>This is a long paragraph about chairs that should be kept as readable text.</p>
            <script>var x = "a script that is long enough to be kept if not dropped";</script>
            <p>Short.</p></article><footer>Copyright line that is long enough to be kept otherwise</footer></body></html>"#;
        let f = parse_html(html, "https://example.com/a/b.html");
        assert_eq!(f.title.as_deref(), Some("The \"Real\" Title"));
        assert_eq!(f.description.as_deref(), Some("A page about chairs."));
        assert_eq!(f.image_url.as_deref(), Some("https://example.com/img/chair.jpg"));
        assert_eq!(f.site_name.as_deref(), Some("Example"));
        let body = f.body_text.unwrap();
        assert!(body.contains("long paragraph about chairs"));
        assert!(!body.contains("script"));
        assert!(!body.contains("Copyright"));
        let f = parse_html("<title> Only   title </title>", "https://e.com/");
        assert_eq!(f.title.as_deref(), Some("Only title"));
    }

    #[test]
    fn robots_rules() {
        let r = "User-agent: Googlebot\nDisallow: /\n\nUser-agent: *\nDisallow: /private\nAllow: /private/ok\nDisallow: /*.pdf$\n";
        assert!(robots_allows(r, "/blog/post"));
        assert!(!robots_allows(r, "/private/x"));
        assert!(robots_allows(r, "/private/ok/page"));
        assert!(!robots_allows(r, "/file.pdf"));
        assert!(robots_allows(r, "/file.pdf?x"));
        assert!(!robots_allows("User-agent: *\nDisallow: /", "/anything"));
        assert!(robots_allows("", "/x"));
    }

    #[test]
    fn urls_and_entities() {
        assert!(allowed_url("https://example.com/x").is_ok());
        for bad in ["http://localhost:8787/api", "http://127.0.0.1/", "http://192.168.1.4/", "http://[::1]/", "ftp://x.com", "http://10.0.0.1"] {
            assert!(allowed_url(bad).is_err(), "{bad}");
        }
        assert_eq!(absolute("https://e.com/a/b?q", "c.png"), "https://e.com/a/c.png");
        assert_eq!(absolute("https://e.com/a/b", "//cdn.e.com/x.png"), "https://cdn.e.com/x.png");
        assert_eq!(decode_entities("a &amp; b &#8217; &#x41; &bogus"), "a & b \u{2019} A &bogus");
    }

    #[test]
    fn thumbnails_fit_512_and_find_colour() {
        let img = image::RgbImage::from_fn(1200, 600, |x, _| if x < 900 { image::Rgb([20, 90, 220]) } else { image::Rgb([250, 250, 250]) });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let (jpg, w, h, color) = thumbnail(&png).unwrap();
        assert_eq!((w, h), (1200, 600));
        let t = image::load_from_memory(&jpg).unwrap();
        assert_eq!((t.width(), t.height()), (512, 256));
        assert!(color.starts_with('#') && color.len() == 7);
        let b = u8::from_str_radix(&color[5..7], 16).unwrap();
        assert!(b > 150, "{color} should be blue");
        assert!(thumbnail(b"not an image").is_err());
    }
}
