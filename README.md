# Riff Editorial Plugins

WASM plugins that fetch album reviews and ratings from editorial sources for the [Riff](https://github.com/alexmaslar/riff) music server.

## Plugins

| Plugin | Source | Data |
|---|---|---|
| **allmusic** | [AllMusic](https://www.allmusic.com) | Ratings, review excerpts, reviewer attribution |
| **pitchfork** | [Pitchfork](https://pitchfork.com) | Ratings, review excerpts, reviewer attribution |

## Build

Requires Rust with the `wasm32-wasip1` target:

```sh
rustup target add wasm32-wasip1
cargo build --release --target wasm32-wasip1
```

Compiled WASM binaries are in `target/wasm32-wasip1/release/`.

## Local Development

Copy the plugin WASM and manifest to the dev plugins directory. Riff hot-reloads from here on startup:

```sh
mkdir -p /tmp/riff-dev-plugins/allmusic
cp target/wasm32-wasip1/release/riff_plugin_allmusic.wasm /tmp/riff-dev-plugins/allmusic/plugin.wasm
cp allmusic/manifest.json /tmp/riff-dev-plugins/allmusic/

mkdir -p /tmp/riff-dev-plugins/pitchfork
cp target/wasm32-wasip1/release/riff_plugin_pitchfork.wasm /tmp/riff-dev-plugins/pitchfork/plugin.wasm
cp pitchfork/manifest.json /tmp/riff-dev-plugins/pitchfork/
```

## Project Structure

```
editorial-common/     Shared library (slugify, HTML parsing, types)
allmusic/
  src/allmusic.rs     Search + match + JSON-LD rating extraction
  manifest.json       Plugin manifest (id, capabilities, HTTP permissions)
pitchfork/
  src/pitchfork.rs    Search + match + JSON-LD rating extraction
  manifest.json
```

## How It Works

Each plugin implements `riff_get_album_reviews(input) -> EditorialResult`:

1. Search the source site for the album (artist + title query)
2. Match the correct album from search results via slug comparison
3. Fetch the album page and extract structured data (JSON-LD)
4. Return rating, review excerpt, and reviewer attribution

AllMusic includes additional false-positive protection for short/common titles:
- Length ratio guard on substring slug matching
- Exact slug fallback with JSON-LD `byArtist` artist verification

## Plugin Guide

See the [Plugin Development Guide](https://github.com/alexmaslar/riff-plugins/blob/main/PLUGINS.md) for the full WASM plugin API reference.
