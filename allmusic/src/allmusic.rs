use editorial_common::{clean_title, slugify, url_encode, SiteReview};
use extism_pdk::*;
use serde::Deserialize;

/// Attempt to fetch an AllMusic review for the given album.
pub fn fetch_review(artist: &str, title: &str) -> Option<SiteReview> {
    let cleaned = clean_title(title);
    let album_url = search_for_album(artist, cleaned)?;

    // Fetch album page for rating from JSON-LD
    let req = HttpRequest::new(&album_url).with_header("Accept", "text/html");
    let resp = http::request::<()>(&req, None).ok()?;
    if resp.status_code() != 200 {
        return None;
    }

    let body = String::from_utf8(resp.body().to_vec()).ok()?;
    let mut review = parse_album_page(&album_url, &body, artist)?;

    // Fetch review text from the AJAX endpoint (requires XHR + Referer headers)
    let review_url = format!("{}/reviewAjax", album_url);
    let req = HttpRequest::new(&review_url)
        .with_header("Accept", "text/html, */*; q=0.01")
        .with_header("X-Requested-With", "XMLHttpRequest")
        .with_header("Referer", &album_url);
    if let Ok(resp) = http::request::<()>(&req, None) {
        if resp.status_code() == 200 {
            if let Ok(html) = String::from_utf8(resp.body().to_vec()) {
                let (excerpt, reviewer) = parse_review_ajax(&html);
                review.excerpt = excerpt;
                if reviewer.is_some() {
                    review.reviewer = reviewer;
                }
            }
        }
    }

    Some(review)
}

/// Search AllMusic and find the album page URL.
fn search_for_album(artist: &str, title: &str) -> Option<String> {
    let title_slug = slugify(title);
    let artist_slug = slugify(artist);

    let query = format!("{} {}", artist, title);
    if let Some(url) = search_and_match(&query, &title_slug, &artist_slug) {
        return Some(url);
    }

    search_and_match(title, &title_slug, &artist_slug)
}

/// Search AllMusic and return the best matching album URL.
fn search_and_match(query: &str, title_slug: &str, artist_slug: &str) -> Option<String> {
    let encoded = url_encode(query);
    let search_url = format!("https://www.allmusic.com/search/albums/{}", encoded);

    let req = HttpRequest::new(&search_url).with_header("Accept", "text/html");
    let resp = http::request::<()>(&req, None).ok()?;
    if resp.status_code() != 200 {
        return None;
    }

    let html = String::from_utf8(resp.body().to_vec()).ok()?;
    find_best_album_match(&html, title_slug, artist_slug)
}

/// Find the best matching album URL from search results HTML.
fn find_best_album_match(html: &str, title_slug: &str, artist_slug: &str) -> Option<String> {
    let album_links = extract_album_links(html);
    let mut first_exact = None;

    // Pass 1: Exact slug match + artist in context (strongest signal)
    for (url, context) in &album_links {
        let url_slug = extract_slug_from_url(url);
        if slug_exact_match(&url_slug, title_slug) {
            let context_slug = slugify(context);
            if context_slug.contains(artist_slug) || artist_slug.is_empty() {
                return Some(url.clone());
            }
            if first_exact.is_none() {
                first_exact = Some(url.clone());
            }
        }
    }

    // Pass 2: Contains slug match + artist in context (e.g. URL-encoded titles)
    for (url, context) in &album_links {
        let url_slug = extract_slug_from_url(url);
        if slug_matches(&url_slug, title_slug) {
            let context_slug = slugify(context);
            if context_slug.contains(artist_slug) || artist_slug.is_empty() {
                return Some(url.clone());
            }
        }
    }

    // Pass 3: Exact slug match without artist context — rely on album page
    // JSON-LD byArtist verification to reject wrong matches.
    first_exact
}

/// Check if a URL slug exactly matches the expected title slug (or its decoded form).
fn slug_exact_match(url_slug: &str, title_slug: &str) -> bool {
    if url_slug == title_slug {
        return true;
    }
    let decoded = simple_url_decode(url_slug);
    let decoded_slug = slugify(&decoded);
    decoded_slug == title_slug
}

/// Check if a URL slug matches the expected title slug (substring with length guard).
fn slug_matches(url_slug: &str, title_slug: &str) -> bool {
    if url_slug.contains(title_slug) && is_close_length(title_slug, url_slug) {
        return true;
    }
    let decoded = simple_url_decode(url_slug);
    let decoded_slug = slugify(&decoded);
    decoded_slug.contains(title_slug) && is_close_length(title_slug, &decoded_slug)
}

/// Require the title slug to be at least 70% of the URL slug length.
/// Blocks false positives like "baby" matching "plays-pretty-for-baby".
fn is_close_length(title_slug: &str, url_slug: &str) -> bool {
    if url_slug.is_empty() {
        return false;
    }
    (title_slug.len() as f64 / url_slug.len() as f64) >= 0.7
}

/// Simple percent-decoding for URL path segments.
fn simple_url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                result.push(byte as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Extract album links and surrounding context from search results HTML.
fn extract_album_links(html: &str) -> Vec<(String, String)> {
    let pattern = "href=\"/album/";
    let mut results = Vec::new();
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

        if path.contains("-mw") {
            let full_url = format!("https://www.allmusic.com{}", path);
            let context_end = (path_end + 2000).min(html.len());
            let context = &html[path_end..context_end];
            if !results.iter().any(|(u, _): &(String, String)| u == &full_url) {
                results.push((full_url, context.to_string()));
            }
        }

        search_from = path_end;
        if search_from >= html.len().saturating_sub(50) {
            break;
        }
    }

    results
}

/// Extract the title slug from an AllMusic album URL.
fn extract_slug_from_url(url: &str) -> String {
    let path = url.split("/album/").nth(1).unwrap_or("");
    if let Some(mw_pos) = path.rfind("-mw") {
        path[..mw_pos].to_string()
    } else {
        path.to_string()
    }
}

#[derive(Deserialize)]
struct AlbumJsonLd {
    #[serde(rename = "aggregateRating")]
    aggregate_rating: Option<AggregateRating>,
    #[serde(rename = "byArtist")]
    by_artist: Option<Vec<ByArtist>>,
}

#[derive(Deserialize)]
struct ByArtist {
    name: Option<String>,
}

#[derive(Deserialize)]
struct AggregateRating {
    #[serde(rename = "ratingValue")]
    rating_value: Option<String>,
    #[serde(rename = "ratingCount")]
    rating_count: Option<u32>,
    #[serde(rename = "bestRating")]
    best_rating: Option<String>,
}

/// Parse the reviewAjax HTML for review text and reviewer name.
/// Format: <h3>Album Review by Reviewer Name</h3> <p>Review text...</p>
fn parse_review_ajax(html: &str) -> (Option<String>, Option<String>) {
    let reviewer = html
        .find("<h3>")
        .and_then(|start| {
            let inner_start = start + 4;
            let inner_end = html[inner_start..].find("</h3>")? + inner_start;
            let h3_text = strip_html_tags(&html[inner_start..inner_end]);
            // Format: "Album Review by Reviewer Name"
            h3_text
                .find(" Review by ")
                .map(|pos| h3_text[pos + " Review by ".len()..].trim().to_string())
        });

    let excerpt = html
        .find("<p>")
        .and_then(|start| {
            let inner_start = start + 3;
            let inner_end = html[inner_start..].find("</p>")? + inner_start;
            let text = strip_html_tags(&html[inner_start..inner_end]);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

    (excerpt, reviewer)
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

/// Parse an AllMusic album page for rating data from JSON-LD.
/// Verifies that the page's byArtist matches the expected artist.
fn parse_album_page(url: &str, html: &str, artist: &str) -> Option<SiteReview> {
    let json_ld = extract_album_json_ld(html)?;
    let album: AlbumJsonLd = serde_json::from_str(&json_ld).ok()?;

    // Verify artist from JSON-LD structured data
    let artist_slug = slugify(artist);
    if !artist_slug.is_empty() {
        let artist_ok = album.by_artist.as_ref().map_or(false, |artists| {
            artists.iter().any(|a| {
                a.name
                    .as_ref()
                    .map_or(false, |n| slugify(n).contains(&artist_slug))
            })
        });
        if !artist_ok {
            return None;
        }
    }

    let agg = album.aggregate_rating?;

    let rating_value: f64 = agg.rating_value.as_deref()?.parse().ok()?;
    let best: f64 = agg
        .best_rating
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10.0);

    let rating = if best > 0.0 {
        (rating_value / best) * 10.0
    } else {
        return None;
    };

    if !(0.0..=10.0).contains(&rating) {
        return None;
    }

    Some(SiteReview {
        source_url: url.to_string(),
        excerpt: None,
        rating: Some(rating),
        rating_count: agg.rating_count,
        reviewer: None,
        review_date: None,
    })
}

/// Extract the JSON-LD block containing MusicAlbum schema from HTML.
fn extract_album_json_ld(html: &str) -> Option<String> {
    let marker = "application/ld+json";
    let mut search_from = 0;

    loop {
        let tag_pos = html[search_from..].find(marker)?;
        let abs_pos = search_from + tag_pos;

        let content_start = html[abs_pos..].find('>')? + abs_pos + 1;
        let content_end = html[content_start..].find("</script>")? + content_start;
        let json_str = html[content_start..content_end].trim();

        if json_str.contains("\"MusicAlbum\"") {
            if json_str.starts_with('[') {
                if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
                    for item in &arr {
                        let s = item.to_string();
                        if s.contains("\"MusicAlbum\"") {
                            return Some(s);
                        }
                    }
                }
            }
            return Some(json_str.to_string());
        }

        search_from = content_end;
        if search_from >= html.len().saturating_sub(50) {
            break;
        }
    }

    None
}
