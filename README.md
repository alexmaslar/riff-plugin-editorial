# Riff Editorial Plugin

A consolidated WASM plugin that fetches album reviews and ratings from multiple editorial sources for the [Riff](https://github.com/alexmaslar/riff) music server.

## Sources

| Source | Data |
|---|---|
| [AllMusic](https://www.allmusic.com) | Ratings, review excerpts, reviewer attribution |
| [Northern Transmissions](https://northerntransmissions.com) | Ratings (0-10), review excerpts, reviewer attribution |
| [Pitchfork](https://pitchfork.com) | Ratings, review excerpts, reviewer attribution |
| [The Line of Best Fit](https://www.thelineofbestfit.com) | Ratings (0-10), full review text, reviewer attribution |

## Build

Requires Rust with the `wasm32-wasip1` target:

```sh
rustup target add wasm32-wasip1
cargo build --release --target wasm32-wasip1
```

This builds all source modules in a single workspace. Compiled WASM binaries are in `target/wasm32-wasip1/release/`.

## Local Development

Copy the plugin WASM and manifest to the dev plugins directory. Riff hot-reloads from here on startup:

```sh
mkdir -p /tmp/riff-dev-plugins/editorial
cp target/wasm32-wasip1/release/riff_plugin_editorial.wasm /tmp/riff-dev-plugins/editorial/plugin.wasm
cp editorial-manifest.json /tmp/riff-dev-plugins/editorial/manifest.json
```

## Project Structure

```
editorial-common/                  Shared library (slugify, HTML parsing, types)
  src/
    lib.rs                         Re-exports
    html.rs                        HTML parsing helpers
    types.rs                       Shared types (EditorialResult, etc.)
    util.rs                        Utility functions
allmusic/
  src/allmusic.rs                  Search + match + JSON-LD rating extraction
  manifest.json                    Per-source manifest
northern-transmissions/
  src/northern_transmissions.rs    WP REST API search + HTML rating extraction
  manifest.json
pitchfork/
  src/pitchfork.rs                 Search + match + JSON-LD rating extraction
  manifest.json
thelineofbestfit/
  src/thelineofbestfit.rs          Progressive listing crawl + JSON-LD + full review extraction
  manifest.json
```

Each source is a separate Rust crate in the workspace, sharing `editorial-common` for types and utilities.

## How It Works

Each source module implements `riff_get_album_reviews(input) -> EditorialResult`:

1. Search the source site for the album (artist + title query)
2. Match the correct album from search results via slug comparison
3. Fetch the album page and extract structured data (JSON-LD, HTML parsing, or REST API)
4. Return rating, review excerpt, and reviewer attribution

### AllMusic

Includes false-positive protection for short/common titles:
- Length ratio guard on substring slug matching
- Exact slug fallback with JSON-LD `byArtist` artist verification

### Northern Transmissions

Uses a hybrid approach:
- WordPress REST API for search, review text, and date
- Page HTML scraping for rating (0-10 in `<h2>`/`<span>` tags) and reviewer ("Words by" pattern)

### The Line of Best Fit

Uses progressive listing crawl (no search API):
- Crawls `/albums?page=N` listing pages in batches of 25, caching slugs in Extism vars across calls
- Matches albums by slug prefix (`artist-slug-album-slug`)
- Extracts rating and metadata from JSON-LD, full review text from `c--article-copy__sections` div

## Plugin Guide

See the [Plugin Development Guide](https://github.com/alexmaslar/riff-plugins/blob/main/PLUGINS.md) for the full WASM plugin API reference.
