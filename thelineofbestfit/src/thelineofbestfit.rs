use editorial_common::{clean_title, slugify, SiteReview};
use extism_pdk::*;
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "https://www.thelineofbestfit.com";
const LISTING_URL: &str = "https://www.thelineofbestfit.com/albums";
const BATCH_SIZE: u32 = 25;
const MAX_PAGES: u32 = 348;
const CACHE_VAR: &str = "tlobf_cache";

/// Progressive URL cache stored in Extism vars across calls.
/// Stores slugs only (not full URLs) to reduce serialized size by ~60%.
#[derive(Serialize, Deserialize, Default)]
struct UrlCache {
    next_page: u32,
    slugs: Vec<String>,
}

/// JSON-LD structures for MusicAlbum review pages.
#[derive(Deserialize)]
struct JsonLd {
    #[serde(rename = "@type")]
    type_name: Option<String>,
    review: Option<JsonLdReview>,
    #[serde(rename = "datePublished")]
    date_published: Option<String>,
}

#[derive(Deserialize)]
struct JsonLdReview {
    #[serde(rename = "reviewRating")]
    review_rating: Option<JsonLdRating>,
    author: Option<JsonLdAuthor>,
    #[serde(rename = "datePublished")]
    date_published: Option<String>,
    #[serde(rename = "reviewBody")]
    review_body: Option<String>,
}

#[derive(Deserialize)]
struct JsonLdRating {
    #[serde(rename = "ratingValue")]
    rating_value: Option<serde_json::Value>,
    #[serde(rename = "bestRating")]
    best_rating: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct JsonLdAuthor {
    name: Option<String>,
}

/// Fetch a review from The Line of Best Fit for the given album.
pub fn fetch_review(artist: &str, title: &str) -> Option<SiteReview> {
    let review_url = find_review_url(artist, title)?;

    let req = HttpRequest::new(&review_url).with_header("Accept", "text/html");
    let resp = http::request::<()>(&req, None).ok()?;
    if resp.status_code() != 200 {
        return None;
    }

    let html = String::from_utf8(resp.body().to_vec()).ok()?;

    // Get rating, reviewer, date from JSON-LD; full review text from HTML body
    let mut review = parse_json_ld(&html, &review_url)?;
    if let Some(body_text) = extract_article_body(&html) {
        review.excerpt = Some(body_text);
    }
    Some(review)
}

/// Search the progressive URL cache for a matching review URL.
fn find_review_url(artist: &str, title: &str) -> Option<String> {
    let cleaned = clean_title(title);
    let artist_slug = slugify(artist);
    let album_slug = slugify(cleaned);
    let prefix = format!("{}-{}", artist_slug, album_slug);

    if prefix.is_empty() {
        return None;
    }

    let mut cache = load_cache();

    // Extend the cache if incomplete
    if cache.next_page < MAX_PAGES {
        fetch_next_batch(&mut cache);
        save_cache(&cache);
    }

    // Search for a matching URL by slug prefix
    match_url(&cache, &prefix)
}

/// Find a URL in the cache whose slug starts with the given prefix.
fn match_url(cache: &UrlCache, prefix: &str) -> Option<String> {
    let prefix_with_dash = format!("{}-", prefix);
    for slug in &cache.slugs {
        if slug == prefix || slug.starts_with(&prefix_with_dash) {
            return Some(format!("{}/albums/{}", BASE_URL, slug));
        }
    }
    None
}

/// Fetch the next batch of listing pages and add discovered URLs to the cache.
fn fetch_next_batch(cache: &mut UrlCache) {
    let start = cache.next_page + 1;
    let end = (start + BATCH_SIZE).min(MAX_PAGES + 1);

    for page in start..end {
        let url = format!("{}?page={}", LISTING_URL, page);
        let req = HttpRequest::new(&url).with_header("Accept", "text/html");

        let resp = match http::request::<()>(&req, None) {
            Ok(r) => r,
            Err(_) => {
                // Skip failed pages gracefully
                continue;
            }
        };

        if resp.status_code() != 200 {
            continue;
        }

        if let Ok(html) = String::from_utf8(resp.body().to_vec()) {
            let new_slugs = extract_album_slugs(&html);
            for slug in new_slugs {
                // Deduplicate: only add if not already present
                if !cache.slugs.iter().any(|s| s == &slug) {
                    cache.slugs.push(slug);
                }
            }
        }

        cache.next_page = page;
    }
}

/// Extract all album slugs from a listing page HTML.
/// Matches both relative (`/albums/slug`) and absolute (`https://...thelineofbestfit.com/albums/slug`) URLs.
fn extract_album_slugs(html: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Match both relative and absolute album URL patterns
    let patterns: &[&str] = &[
        "href=\"/albums/",
        "href=\"https://www.thelineofbestfit.com/albums/",
    ];

    for pattern in patterns {
        let mut search_from = 0;
        while let Some(pos) = html[search_from..].find(pattern) {
            let abs_pos = search_from + pos;
            let slug_start = abs_pos + pattern.len();

            // Find the closing quote
            if let Some(end_offset) = html[slug_start..].find('"') {
                let slug = &html[slug_start..slug_start + end_offset];

                // Skip empty slugs or slugs with query params/fragments
                if !slug.is_empty() && !slug.contains('?') && !slug.contains('#') {
                    if seen.insert(slug.to_string()) {
                        results.push(slug.to_string());
                    }
                }

                search_from = slug_start + end_offset;
            } else {
                break;
            }
        }
    }

    results
}

/// Extract the full review text from the HTML article body.
/// The review content lives in `<div class="c--article-copy__sections">`.
fn extract_article_body(html: &str) -> Option<String> {
    let marker = "c--article-copy__sections";
    let marker_pos = html.find(marker)?;

    // Find the end of the opening tag
    let content_start = html[marker_pos..].find('>')? + marker_pos + 1;

    // Walk nested divs to find the matching close
    let mut depth: u32 = 1;
    let mut pos = content_start;
    let content_end;

    loop {
        let next_open = html[pos..].find("<div");
        let next_close = html[pos..].find("</div>");

        let close_abs = match next_close {
            Some(c) => pos + c,
            None => return None,
        };

        if let Some(o) = next_open {
            let open_abs = pos + o;
            if open_abs < close_abs {
                depth += 1;
                pos = open_abs + 4;
                continue;
            }
        }

        depth -= 1;
        if depth == 0 {
            content_end = close_abs;
            break;
        }
        pos = close_abs + 6;
    }

    let raw = &html[content_start..content_end];

    // Insert paragraph breaks before block-level closing tags
    let raw = raw
        .replace("</p>", "\n\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");

    // Strip HTML tags
    let text = strip_html_tags(&raw);

    // Decode common HTML entities
    let text = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&apos;", "'")
        .replace("&ndash;", "\u{2013}")
        .replace("&mdash;", "\u{2014}");

    // Collapse runs of whitespace while preserving paragraph breaks (\n\n)
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|p| {
            let mut collapsed = String::with_capacity(p.len());
            let mut prev_ws = false;
            for ch in p.chars() {
                if ch.is_whitespace() {
                    if !prev_ws {
                        collapsed.push(' ');
                    }
                    prev_ws = true;
                } else {
                    collapsed.push(ch);
                    prev_ws = false;
                }
            }
            collapsed.trim().to_string()
        })
        .filter(|p| !p.is_empty())
        .collect();

    if paragraphs.is_empty() {
        return None;
    }

    let trimmed = paragraphs.join("\n\n");

    // Truncate to ~2000 chars at a sentence boundary
    if trimmed.len() > 2000 {
        if let Some(pos) = trimmed[..2000].rfind(". ") {
            Some(trimmed[..=pos].to_string())
        } else {
            let mut s = trimmed[..2000].to_string();
            s.push_str("...");
            Some(s)
        }
    } else {
        Some(trimmed.to_string())
    }
}

/// Strip HTML tags from a string, keeping only text content.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Parse JSON-LD blocks from a review page to extract review data.
fn parse_json_ld(html: &str, review_url: &str) -> Option<SiteReview> {
    let marker = "application/ld+json";
    let mut search_from = 0;

    loop {
        let tag_pos = match html[search_from..].find(marker) {
            Some(p) => p,
            None => break,
        };
        let abs_pos = search_from + tag_pos;

        let content_start = match html[abs_pos..].find('>') {
            Some(p) => abs_pos + p + 1,
            None => break,
        };
        let content_end = match html[content_start..].find("</script>") {
            Some(p) => content_start + p,
            None => break,
        };

        let json_str = html[content_start..content_end].trim();

        // Try parsing as a single object
        if let Ok(ld) = serde_json::from_str::<JsonLd>(json_str) {
            if ld.type_name.as_deref() == Some("MusicAlbum") {
                if let Some(review) = extract_review_from_ld(&ld, review_url) {
                    return Some(review);
                }
            }
        }

        // Try parsing as an array
        if let Ok(arr) = serde_json::from_str::<Vec<JsonLd>>(json_str) {
            for ld in &arr {
                if ld.type_name.as_deref() == Some("MusicAlbum") {
                    if let Some(review) = extract_review_from_ld(ld, review_url) {
                        return Some(review);
                    }
                }
            }
        }

        search_from = content_end;
        if search_from >= html.len().saturating_sub(50) {
            break;
        }
    }

    None
}

/// Extract a SiteReview from a parsed MusicAlbum JSON-LD block.
fn extract_review_from_ld(ld: &JsonLd, review_url: &str) -> Option<SiteReview> {
    let review = ld.review.as_ref()?;

    let rating = review.review_rating.as_ref().and_then(|r| {
        let value = parse_numeric_value(r.rating_value.as_ref()?)?;
        let best = r
            .best_rating
            .as_ref()
            .and_then(|b| parse_numeric_value(b))
            .unwrap_or(10.0);

        if best > 0.0 && best != 10.0 {
            Some((value / best) * 10.0)
        } else {
            Some(value)
        }
    });

    let reviewer = review.author.as_ref().and_then(|a| a.name.clone());

    // Prefer review-level date, fall back to top-level
    let review_date = review
        .date_published
        .clone()
        .or_else(|| ld.date_published.clone());

    let excerpt = review.review_body.as_ref().map(|body| {
        let cleaned = clean_review_body(body);
        let trimmed = cleaned.trim();
        if trimmed.len() > 2000 {
            if let Some(pos) = trimmed[..2000].rfind(". ") {
                trimmed[..=pos].to_string()
            } else {
                let mut s = trimmed[..2000].to_string();
                s.push_str("...");
                s
            }
        } else {
            trimmed.to_string()
        }
    });

    if rating.is_none() && excerpt.is_none() {
        return None;
    }

    Some(SiteReview {
        source_url: review_url.to_string(),
        excerpt,
        rating,
        rating_count: None,
        reviewer,
        review_date,
    })
}

/// Clean a review body from JSON-LD: strip CDATA wrapper, decode HTML entities, strip HTML tags.
fn clean_review_body(body: &str) -> String {
    let mut s = body.to_string();

    // Strip CDATA wrapper
    if let Some(inner) = s.strip_prefix("<![CDATA[") {
        if let Some(inner) = inner.strip_suffix("]]>") {
            s = inner.to_string();
        }
    }

    // Decode HTML entities
    s = s
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&ndash;", "\u{2013}")
        .replace("&mdash;", "\u{2014}")
        .replace("&amp;", "&");

    // Strip HTML tags
    let text = strip_html_tags(&s);

    // Collapse multiple whitespace/newlines into single spaces
    let mut collapsed = String::with_capacity(text.len());
    let mut prev_ws = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                collapsed.push(' ');
            }
            prev_ws = true;
        } else {
            collapsed.push(ch);
            prev_ws = false;
        }
    }

    collapsed.trim().to_string()
}

/// Parse a JSON value (string or number) as f64.
fn parse_numeric_value(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Load the URL cache from an Extism var, or return an empty cache.
fn load_cache() -> UrlCache {
    let bytes: Option<Vec<u8>> = var::get(CACHE_VAR).ok().flatten();
    bytes
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Save the URL cache to an Extism var.
fn save_cache(cache: &UrlCache) {
    if let Ok(bytes) = serde_json::to_vec(cache) {
        let _ = var::set(CACHE_VAR, &bytes);
    }
}
