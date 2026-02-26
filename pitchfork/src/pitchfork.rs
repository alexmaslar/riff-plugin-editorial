use editorial_common::{clean_title, extract_json_ld, slugify, url_encode, SiteReview};
use extism_pdk::*;
use serde::Deserialize;

/// Attempt to fetch a Pitchfork review for the given album.
pub fn fetch_review(artist: &str, title: &str) -> Option<SiteReview> {
    let review_url = search_for_review(artist, title)?;

    let req = HttpRequest::new(&review_url).with_header("Accept", "text/html");
    let resp = http::request::<()>(&req, None).ok()?;
    if resp.status_code() != 200 {
        return None;
    }

    let body = String::from_utf8(resp.body().to_vec()).ok()?;
    parse_review_page(&review_url, &body)
}

/// Search Pitchfork to find the review URL for an album.
/// Tries artist+title first, then falls back to artist-only with slug matching.
fn search_for_review(artist: &str, title: &str) -> Option<String> {
    let cleaned = clean_title(title);
    let title_slug = slugify(cleaned);

    // Try artist+title first (works for most albums)
    let query = format!("{} {}", artist, cleaned);
    if let Some(url) = search_and_match(&query, &title_slug) {
        return Some(url);
    }

    // Fall back to artist-only (Pitchfork search chokes on some album titles)
    search_and_match(artist, &title_slug)
}

/// Search Pitchfork and return the review URL whose slug best matches title_slug.
fn search_and_match(query: &str, title_slug: &str) -> Option<String> {
    let encoded = url_encode(query);
    let search_url = format!("https://pitchfork.com/search/?q={}", encoded);

    let req = HttpRequest::new(&search_url).with_header("Accept", "text/html");
    let resp = http::request::<()>(&req, None).ok()?;
    if resp.status_code() != 200 {
        return None;
    }

    let html = String::from_utf8(resp.body().to_vec()).ok()?;
    let urls = extract_review_urls(&html);

    // Find the URL whose slug contains the title slug
    urls.into_iter().find(|url| {
        if let Some(slug_part) = url.split("/reviews/albums/").nth(1) {
            let slug = slug_part.trim_end_matches('/');
            // Strip optional numeric prefix (e.g. "17253-")
            let slug = if let Some(pos) = slug.find('-') {
                if slug[..pos].chars().all(|c| c.is_ascii_digit()) {
                    &slug[pos + 1..]
                } else {
                    slug
                }
            } else {
                slug
            };
            slug.contains(title_slug)
        } else {
            false
        }
    })
}

/// Extract all review album URLs from Pitchfork search HTML.
fn extract_review_urls(html: &str) -> Vec<String> {
    let pattern = "href=\"/reviews/albums/";
    let mut urls = Vec::new();
    let mut search_from = 0;

    loop {
        let Some(pos) = html[search_from..].find(pattern) else {
            break;
        };
        let abs_pos = search_from + pos;
        let path_start = abs_pos + "href=\"".len();
        let Some(end_offset) = html[path_start..].find('"') else {
            break;
        };
        let path_end = path_start + end_offset;
        let path = &html[path_start..path_end];

        if path != "/reviews/albums/" && path.len() > "/reviews/albums/".len() {
            let full_url = format!("https://pitchfork.com{}", path);
            if !urls.contains(&full_url) {
                urls.push(full_url);
            }
        }

        search_from = path_end;
        if search_from >= html.len().saturating_sub(50) {
            break;
        }
    }

    urls
}

/// JSON-LD schema for Pitchfork review pages.
#[derive(Deserialize)]
struct JsonLdReview {
    #[serde(rename = "reviewBody")]
    review_body: Option<String>,
    author: Option<serde_json::Value>,
    #[serde(rename = "datePublished")]
    date_published: Option<String>,
}

/// Parse a Pitchfork review page for rating (from __PRELOADED_STATE__) and
/// review text/author/date (from JSON-LD).
fn parse_review_page(url: &str, html: &str) -> Option<SiteReview> {
    let rating = extract_rating_from_preloaded(html);

    let json_ld = extract_json_ld(html);
    let (excerpt, reviewer, review_date) = if let Some(ref ld_str) = json_ld {
        if let Ok(review) = serde_json::from_str::<JsonLdReview>(ld_str) {
            let excerpt = review.review_body;

            let reviewer = review.author.and_then(|a| match a {
                serde_json::Value::Array(arr) => arr
                    .first()
                    .and_then(|v| v.get("name"))
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string()),
                serde_json::Value::Object(obj) => {
                    obj.get("name").and_then(|n| n.as_str()).map(|s| s.to_string())
                }
                _ => None,
            });

            let review_date = review.date_published;

            (excerpt, reviewer, review_date)
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    if rating.is_none() && excerpt.is_none() {
        return None;
    }

    Some(SiteReview {
        source_url: url.to_string(),
        excerpt,
        rating,
        rating_count: None,
        reviewer,
        review_date,
    })
}

/// Extract the numeric rating from Pitchfork's __PRELOADED_STATE__ JSON.
fn extract_rating_from_preloaded(html: &str) -> Option<f64> {
    let state_marker = "__PRELOADED_STATE__";
    let state_pos = html.find(state_marker)?;
    let state_region = &html[state_pos..];

    let pattern = "\"rating\":";
    let mut search_from = 0;

    while let Some(pos) = state_region[search_from..].find(pattern) {
        let abs_pos = search_from + pos;
        let value_start = abs_pos + pattern.len();

        // Skip if preceded by another letter (like "bestRating")
        if abs_pos > 0 {
            let before = state_region.as_bytes().get(abs_pos - 1).copied().unwrap_or(b'"');
            if before.is_ascii_alphabetic() {
                search_from = value_start;
                continue;
            }
        }

        let rest = &state_region[value_start..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(rest.len());
        let num_str = &rest[..end];

        if let Ok(val) = num_str.parse::<f64>() {
            if (0.0..=10.0).contains(&val) {
                return Some(val);
            }
        }

        search_from = value_start;
    }

    None
}
