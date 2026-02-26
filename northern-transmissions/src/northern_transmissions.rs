use editorial_common::{clean_title, slugify, url_encode, SiteReview};
use extism_pdk::*;
use serde::Deserialize;

/// WordPress REST API post structure (relevant fields only).
#[derive(Deserialize)]
struct WpPost {
    slug: String,
    link: String,
    date: Option<String>,
    content: Option<WpContent>,
}

#[derive(Deserialize)]
struct WpContent {
    rendered: Option<String>,
}

/// Attempt to fetch a Northern Transmissions review for the given album.
pub fn fetch_review(artist: &str, title: &str) -> Option<SiteReview> {
    let cleaned = clean_title(title);
    let (review_url, content_html, date) = search_for_review(artist, cleaned)?;

    // Extract excerpt from REST API content (strip HTML tags)
    let excerpt = content_html
        .as_ref()
        .map(|html| strip_html_tags(html))
        .map(|text| {
            let trimmed = text.trim();
            // Truncate to ~2000 chars at a sentence boundary
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
        })
        .filter(|s| !s.is_empty());

    // Fetch the actual page HTML for rating and reviewer (not in REST API)
    let req = HttpRequest::new(&review_url).with_header("Accept", "text/html");
    let resp = http::request::<()>(&req, None).ok()?;
    if resp.status_code() != 200 {
        // Even without the page, we have excerpt + date from the API
        return Some(SiteReview {
            source_url: review_url,
            excerpt,
            rating: None,
            rating_count: None,
            reviewer: None,
            review_date: date,
        });
    }

    let page_html = String::from_utf8(resp.body().to_vec()).ok()?;
    let rating = parse_rating(&page_html);
    let reviewer = parse_reviewer(&page_html);

    if rating.is_none() && excerpt.is_none() {
        return None;
    }

    Some(SiteReview {
        source_url: review_url,
        excerpt,
        rating,
        rating_count: None,
        reviewer,
        review_date: date,
    })
}

/// Search the WordPress REST API for a matching review.
/// Returns (url, content_html, date) on success.
fn search_for_review(artist: &str, title: &str) -> Option<(String, Option<String>, Option<String>)> {
    let title_slug = slugify(title);
    let artist_slug = slugify(artist);

    // Try artist + title first
    let query = format!("{} {}", artist, title);
    if let Some(result) = search_and_match(&query, &title_slug, &artist_slug) {
        return Some(result);
    }

    // Fallback: search with just artist name
    search_and_match(artist, &title_slug, &artist_slug)
}

/// Query the WordPress REST API and match results by slug.
fn search_and_match(
    query: &str,
    title_slug: &str,
    artist_slug: &str,
) -> Option<(String, Option<String>, Option<String>)> {
    let encoded = url_encode(query);
    let search_url = format!(
        "https://northerntransmissions.com/wp-json/wp/v2/posts?categories=15&search={}&per_page=5",
        encoded
    );

    let req = HttpRequest::new(&search_url).with_header("Accept", "application/json");
    let resp = http::request::<()>(&req, None).ok()?;
    if resp.status_code() != 200 {
        return None;
    }

    let body = String::from_utf8(resp.body().to_vec()).ok()?;
    let posts: Vec<WpPost> = serde_json::from_str(&body).ok()?;

    // Find the best matching post by slug
    // Prefer posts whose slug contains both title_slug and artist_slug
    let mut best_match: Option<&WpPost> = None;
    let mut best_has_artist = false;

    for post in &posts {
        if !post.slug.contains(title_slug) {
            continue;
        }

        // Length ratio guard: title_slug should be at least 30% of the full slug
        // (NT slugs combine artist + album, so they're longer)
        if !title_slug.is_empty() && !post.slug.is_empty() {
            let ratio = title_slug.len() as f64 / post.slug.len() as f64;
            if ratio < 0.3 {
                continue;
            }
        }

        let has_artist = !artist_slug.is_empty() && post.slug.contains(artist_slug);

        if has_artist && !best_has_artist {
            best_match = Some(post);
            best_has_artist = true;
        } else if best_match.is_none() {
            best_match = Some(post);
        }
    }

    best_match.map(|post| {
        let content_html = post
            .content
            .as_ref()
            .and_then(|c| c.rendered.clone());
        (post.link.clone(), content_html, post.date.clone())
    })
}

/// Extract a numeric rating (0-10) from the page HTML.
/// The rating appears as a standalone number in `<h2>` or `<span>` tags.
fn parse_rating(html: &str) -> Option<f64> {
    // First pass: scan <h2> tags
    if let Some(rating) = extract_rating_from_tags(html, "<h2>", "</h2>") {
        return Some(rating);
    }

    // Second pass: scan <span> tags
    extract_rating_from_tags(html, "<span>", "</span>")
}

/// Scan for tags and try to parse their text content as a rating.
fn extract_rating_from_tags(html: &str, open_tag: &str, close_tag: &str) -> Option<f64> {
    let mut search_from = 0;

    loop {
        let tag_pos = html[search_from..].find(open_tag)?;
        let abs_start = search_from + tag_pos + open_tag.len();

        let Some(end_offset) = html[abs_start..].find(close_tag) else {
            break;
        };
        let abs_end = abs_start + end_offset;

        let inner = strip_html_tags(&html[abs_start..abs_end]);
        let text = inner.trim();

        if let Some(rating) = try_parse_rating(text) {
            return Some(rating);
        }

        search_from = abs_end + close_tag.len();
        if search_from >= html.len().saturating_sub(50) {
            break;
        }
    }

    None
}

/// Try to parse a text string as a rating value 0-10.
/// Handles formats like "7.5", "8", "7.5/10", "8/10".
fn try_parse_rating(text: &str) -> Option<f64> {
    // Strip optional "/10" suffix
    let text = text.strip_suffix("/10").unwrap_or(text).trim();

    // Must be a short numeric string (avoid matching paragraphs)
    if text.len() > 5 || text.is_empty() {
        return None;
    }

    let val: f64 = text.parse().ok()?;
    if (0.0..=10.0).contains(&val) {
        Some(val)
    } else {
        None
    }
}

/// Extract reviewer name from "Words by {Name}" pattern in page HTML.
fn parse_reviewer(html: &str) -> Option<String> {
    let marker = "Words by ";
    let pos = html.find(marker)?;
    let name_start = pos + marker.len();

    // Find the next HTML tag or newline after the name
    let rest = &html[name_start..];
    let end = rest
        .find(['<', '\n'])
        .unwrap_or(rest.len());

    let name = rest[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
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
