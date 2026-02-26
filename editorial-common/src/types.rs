use serde::{Deserialize, Serialize};

/// Output format matching riff-core's expected editorial result.
#[derive(Serialize)]
pub struct EditorialResult {
    pub reviews: Vec<EditorialReview>,
}

/// A single editorial review entry.
#[derive(Serialize)]
pub struct EditorialReview {
    pub source: String,
    pub source_url: String,
    pub excerpt: Option<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<u32>,
    pub reviewer: Option<String>,
    pub review_date: Option<String>,
}

/// Input passed from the server to the plugin.
#[derive(Deserialize)]
pub struct AlbumReviewInput {
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub year: Option<i32>,
}

/// Intermediate result from a site-specific scraper.
pub struct SiteReview {
    pub source_url: String,
    pub excerpt: Option<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<u32>,
    pub reviewer: Option<String>,
    pub review_date: Option<String>,
}

/// Wrap an optional site-specific review into the JSON output format.
pub fn wrap_review(source_name: &str, review: Option<SiteReview>) -> String {
    let mut reviews = Vec::new();

    if let Some(r) = review {
        reviews.push(EditorialReview {
            source: source_name.to_string(),
            source_url: r.source_url,
            excerpt: r.excerpt,
            rating: r.rating,
            rating_count: r.rating_count,
            reviewer: r.reviewer,
            review_date: r.review_date,
        });
    }

    let result = EditorialResult { reviews };
    serde_json::to_string(&result).unwrap_or_else(|_| r#"{"reviews":[]}"#.to_string())
}
