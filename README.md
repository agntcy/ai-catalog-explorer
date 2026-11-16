# AI Catalog Explorer

A small web app for the [AI Catalog](https://github.com/Agent-Card/ai-catalog) specification, built on the released [Rust SDK](https://github.com/Agent-Card/ai-catalog-rust) crates (`ai-catalog`, `-discover`, `-trust`, `-validate`).

- **Explore** (`/`): enter a domain and see how its catalog is discovered (the `Link` header, the HTML `<link>`, the well-known URI), the requests made, its entries and its conformance level. Signatures, the publisher's `did:web` document and artifact digests are verified, so a catalog can reach **Trusted**. Hints name what stands between a catalog and the next level, such as a missing `host`, entries without a digest, or an identifier whose `urn:air` publisher is not the catalog's domain.
- **Publish** (`/publish`): writes a catalog from a short form, checks it as you type, and gives serving configuration for common hosts (nginx, Apache, Caddy, Netlify and Cloudflare Pages, GitHub Pages).
- **Index** (`/catalogs`): a searchable list of known catalogs with their level, how they were found and the kinds of entries they list. Anyone can add a domain. The index keeps only a summary and a link, never the catalog itself, and re-checks each catalog daily.
- **A2A Inspector**: An A2A card entry offers, under Inspect, a ready-to-copy command to test it in the [A2A Inspector](https://github.com/a2aproject/a2a-inspector). The explorer does not run the inspector.
- **MCP Inspector**: an MCP server card with a network remote offers, under Inspect, a ready-to-copy command to test the server in the [MCP Inspector](https://github.com/modelcontextprotocol/inspector).

## Run

```sh
cargo run --release        # http://127.0.0.1:8080
```

| Variable | Default | Purpose |
| --- | --- | --- |
| `BIND` | `127.0.0.1:8080` | Address to listen on. |
| `INDEX_FILE` | `index.json` | Where the index is stored, relative to the current directory. |

The index starts from the domains in [`data/seeds.txt`](data/seeds.txt), which are compiled into the binary, plus every catalog added through the page. Delete `index.json` to start again from the seeds.

## Safety

Requests are limited to public `https` hosts on port 443. Private, loopback and link-local addresses are refused, including after DNS resolution and redirects. Concurrent requests and the number of fetches per lookup are capped, fetched content is shown as text, and the content security policy blocks inline scripts.

The index is a small JSON file store with no per-submitter rate limit and no way to remove a catalog, so the app is not ready to be hosted publicly.

## Verification limits

Trust rests on DNS and TLS for the publisher's domain. Only ES256 is supported, as the `did:web` signer profile requires. A catalog's own signature is checked but never counted as authorized, because the explorer has no list of approved catalog operators. Only the entries of the root catalog are verified: a nested catalog is a separate catalog.

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The pages and scripts in `static/` are embedded in the binary, so rebuild and hard-reload the browser to see a change.
