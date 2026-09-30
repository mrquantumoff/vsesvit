# Vsesvit sync server

Stores Vsesvit's bookmarks, history, open tabs, extensions, settings, search engines and site permissions for people who sign in with an OpenID Connect provider of the operator's choice: any provider with discovery and a userinfo endpoint works. Records are stored in SQLite or Postgres.

The server never reads or merges what it stores. It keeps the last upload of each record, and every browser merges on its own side (`crates/vsesvit-core/src/sync.rs`). The API is in `crates/vsesvit-sync-proto`.

## Run it

The image is `ghcr.io/mrquantumoff/vsesvit-sync-server`. With SQLite, the database lives in `/data`:

```bash
docker run -d -p 127.0.0.1:8080:8080 -v vsesvit-sync:/data \
  -e OIDC_ISSUER=<issuer URL> -e OIDC_CLIENT_ID=<client id> \
  ghcr.io/mrquantumoff/vsesvit-sync-server
```

With Postgres, use `compose.yaml`. It needs a database password; keep it, since the database keeps the first one:

```bash
OIDC_ISSUER=<issuer URL> OIDC_CLIENT_ID=<client id> POSTGRES_PASSWORD=$(openssl rand -hex 24) docker compose -f server/compose.yaml up -d
```

The image is Alpine with a statically linked server and nothing else but CA certificates, and runs as an unprivileged user.

To build the image yourself, run this from the repository root:

```bash
docker build -f server/Dockerfile -t vsesvit-sync-server .
```

Put the server behind HTTPS. Vsesvit only talks plain HTTP to a server on `localhost`. `nginx.conf` is a reverse proxy for that: TLS, the request size limit, and a rate limit per address. Both commands above publish the server on `127.0.0.1` only, so the proxy is the one way in.

Then, in Vsesvit, go to Settings, then Sync, enter the server's address and sign in.

## Configuration

Settings come from environment variables, or from a `.env` file in the working directory. They are checked at startup: a value that could not work stops the server with a message naming it.

| Variable | Default | |
|---|---|---|
| `DATABASE_URL` | `sqlite://vsesvit-sync.db?mode=rwc` (in the image, `/data/vsesvit-sync.db`) | `sqlite://…` or `postgres://…`. |
| `DATABASE_MAX_CONNECTIONS` | `10` | Connections to the database the server opens at most, 1 to 1000. Keep it under the limit of a Postgres shared with other services, or of the server's role. |
| `RUN_MIGRATIONS` | `true` | At startup, apply the migrations the database lacks. With `false`, the server refuses to start while any are pending, for databases migrated separately with the migration CLI. |
| `BIND_ADDRESS` | `0.0.0.0:8080` | |
| `OIDC_ISSUER` | required | The provider's issuer URL, exactly as its `/.well-known/openid-configuration` states it. HTTPS, or HTTP on `localhost` or a loopback IP. |
| `OIDC_CLIENT_ID` | required | The public client that Vsesvit signs in as. |
| `OIDC_SCOPES` | `openid profile` | Must include `openid`. |
| `OIDC_REDIRECT_URIS` | `http://127.0.0.1:47801/callback` … `47805` | Loopback redirect URIs registered for the client: `http`, a loopback IP literal and a port. Vsesvit listens on the first free one. |
| `OIDC_ALLOWED_CLIENT_IDS` | `OIDC_CLIENT_ID` | The clients an access token may name. `*` accepts any token, for providers whose access tokens are opaque. |
| `MAX_BATCH` | `500` | Records per upload and per download page, 1 to 10000. |
| `MAX_RECORD_BYTES` | `1048576` | One record's body. At most half of `MAX_REQUEST_BYTES`. |
| `MAX_REQUEST_BYTES` | `33554432` | One upload request, and roughly one download page. At least 65536. |
| `MAX_ACCOUNT_BYTES` | `1073741824` | What one account may store, bodies and ids. An upload that would pass it is refused with 507; one that does not grow the account always passes. |
| `MAX_ACCOUNT_RECORDS` | `1000000` | Records one account may store. |
| `RUST_LOG` | `info` | |

## The provider

Register Vsesvit with the provider as a public client (no secret). It needs these settings:

- The authorization code grant with PKCE (S256), plus refresh tokens.
- The scopes from `OIDC_SCOPES`.
- Every URI in `OIDC_REDIRECT_URIS` as a redirect URI.

Set `OIDC_ISSUER` to the provider's issuer URL and `OIDC_CLIENT_ID` to the client id it gives you.

The server identifies each user by asking the provider's userinfo endpoint about their token, and uses the `sub` it returns. That check works for every provider and catches revoked tokens, where a local JWT check could not: an access token's `sub` need not be the user, and some providers put the granted scopes there. An accepted token is cached for up to five minutes, a refused one for one minute, and at most 32 calls to the provider run at once.

Userinfo does not say which app a token was issued to, so the token must: a JWT access token naming its client in `client_id` (RFC 9068) or `azp`. Every client it names must be in `OIDC_ALLOWED_CLIENT_IDS`, and a token that names none is refused. Without that check, another app the user signed in to could read their browser data with its own token. For a provider whose access tokens are opaque, set `OIDC_ALLOWED_CLIENT_IDS=*` and accept that risk.

`DELETE /v1/account` deletes an account's records but keeps the account, so its sequence numbers keep counting and other devices' cursors stay valid.

## Develop

```bash
cargo test --manifest-path server/Cargo.toml
```

To also run the tests against Postgres, point them at an empty database. They drop and recreate its tables, and must run one at a time:

```bash
VSESVIT_SYNC_TEST_POSTGRES=postgres://postgres:test@localhost:5432/postgres cargo test --manifest-path server/Cargo.toml -- --test-threads=1
```

`scripts/sync-e2e.sh` builds the server and syncs real browser profiles through it. `cargo run -p migration --manifest-path server/Cargo.toml -- --help` runs SeaORM's migration CLI.

The server is its own Cargo workspace. Its sqlx and the browser's rusqlite link different `libsqlite3-sys` versions, and one Cargo graph cannot hold both.
