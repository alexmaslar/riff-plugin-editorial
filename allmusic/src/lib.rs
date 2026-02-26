mod allmusic;

use editorial_common::{wrap_review, AlbumReviewInput};
use extism_pdk::*;

#[plugin_fn]
pub fn riff_health_check(_input: String) -> FnResult<String> {
    Ok("ok".to_string())
}

#[plugin_fn]
pub fn riff_get_album_reviews(input: String) -> FnResult<String> {
    let params: AlbumReviewInput = serde_json::from_str(&input)?;
    let review = allmusic::fetch_review(&params.artist, &params.title);
    Ok(wrap_review("allmusic", review))
}
