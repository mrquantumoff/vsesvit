# Vsesvit sync server

Stores Vsesvit's bookmarks, history, open tabs, extensions, settings, search engines and site permissions for people who sign in with an OpenID Connect provider of the operator's choice: any provider with discovery and a userinfo endpoint works. Records are stored in SQLite or Postgres.

Browsers talk to this server only. It signs people in with its provider itself, as that provider's client, and gives each browser a session of its own, so Vsesvit needs no client id, secret or provider address: any server works with any copy of the browser.

The server never reads or merges what it stores. It keeps the last upload of each record, and every browser merges on its own side (`crates/vsesvit-core/src/sync.rs`). The API is in `crates/vsesvit-sync-proto`.

## Run it

The image is `ghcr.io/mrquantumoff/vsesvit-sync-server`. With SQLite, the database lives in `/data`:

```bash
docker run -d -p 127.0.0.1:8080:8080 -v vsesvit-sync:/data \
  -e PUBLIC_URL=https://sync.example.com \
  -e OIDC_ISSUER=<issuer URL> -e OIDC_CLIENT_ID=<client id> -e OIDC_CLIENT_SECRET=<client secret> \
  ghcr.io/mrquantumoff/vsesvit-sync-server
```

With Postgres, use `compose.yaml`. It needs a database password; keep it, since the database keeps the first one:

```bash
PUBLIC_URL=https://sync.example.com OIDC_ISSUER=<issuer URL> OIDC_CLIENT_ID=<client id> OIDC_CLIENT_SECRET=<client secret> \
  POSTGRES_PASSWORD=$(openssl rand -hex 24) docker compose -f server/compose.yaml up -d
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
| `PUBLIC_URL` | required | The address browsers reach the server at, such as `https://sync.example.com`; the provider sends people back to `{PUBLIC_URL}/v1/auth/callback`. HTTPS, or HTTP on `localhost` or a loopback IP. |
| `OIDC_ISSUER` | required | The provider's issuer URL, exactly as its `/.well-known/openid-configuration` states it. HTTPS, or HTTP on `localhost` or a loopback IP. |
| `OIDC_CLIENT_ID` | required | The client the server signs people in as. |
| `OIDC_CLIENT_SECRET` | none | The client's secret, for a confidential client. Without it the server is a public client; it uses PKCE either way. |
| `OIDC_SCOPES` | `openid profile` | Must include `openid`. |
| `SESSION_IDLE_DAYS` | `180` | A browser's session ends after this many days unused, 1 to 3650. |
| `MAX_BATCH` | `500` | Records per upload and per download page, 1 to 10000. |
| `MAX_RECORD_BYTES` | `1048576` | One record's body. At most half of `MAX_REQUEST_BYTES`. |
| `MAX_REQUEST_BYTES` | `33554432` | One upload request, and roughly one download page. 65536 to 134217728. |
| `MAX_ACCOUNT_BYTES` | `1073741824` | What one account may store, bodies and ids. An upload that would pass it is refused with 507; one that does not grow the account always passes. |
| `MAX_ACCOUNT_RECORDS` | `1000000` | Records one account may store. |
| `EPOCH` | `0` | Raise it after restoring the database from a backup, so every device syncs everything again. |
| `RUST_LOG` | `info` | |

## The provider

Register the server with the provider as a client:

- **Type.** Confidential with a secret (`OIDC_CLIENT_SECRET`), or public; the server uses PKCE (S256) either way.
- **Grant.** The authorization code grant.
- **Scopes.** The ones in `OIDC_SCOPES`.
- **Redirect URI.** `{PUBLIC_URL}/v1/auth/callback`, such as `https://sync.example.com/v1/auth/callback`.

Set `OIDC_ISSUER` to the provider's issuer URL, and `OIDC_CLIENT_ID` and `OIDC_CLIENT_SECRET` to what it gives you.

## Signing in

Vsesvit signs in to this server, not to the provider. It uses the authorization code flow with PKCE for a native app (RFC 7636, RFC 8252), with the server as the authorization server:

1. The browser opens `/v1/auth/authorize` in a tab, with a loopback `redirect_uri` on whatever port it could open.
2. The server sends the tab on to the provider. The sign-in travels in the `state` it sends there, signed with a key the server makes at startup, so the server stores nothing for a sign-in until the provider vouches for the person, and one under way when the server restarts has to start again.
3. When the provider sends the person back to `/v1/auth/callback`, the server trades the provider's code for an access token, and asks userinfo who it belongs to. The account is the userinfo `sub` at this issuer; an access token's own `sub` need not be the user, and some providers put the granted scopes there.
4. The server sends the tab to the browser's loopback address with a one-time code. Only the browser that started the sign-in listens there, so someone who sends a person their own sign-in link gets nothing.
5. The browser trades the code, with its PKCE verifier, at `/v1/auth/token` for a session: an opaque token the server keeps only as a SHA-256 hash.

The provider sees one request per sign-in and no others: requests for records use the session. A session ends when its browser signs out (`DELETE /v1/auth/session`), or after `SESSION_IDLE_DAYS` unused. Disabling someone at the provider does not end their sessions here; delete their rows from `sessions` to do that.

`DELETE /v1/account` deletes an account's records and ends all its sessions, so every device signs in again and then uploads everything it holds. The account stays, so its sequence numbers keep counting.

The database can be restored from a backup. After restoring one, raise `EPOCH` (by one is enough) and restart the server: every device then uploads everything it holds and downloads everything again, so what the backup lacks comes back from the devices that have it. Without that, the server notices a restore only when a device syncs while its cursor is still past every write the backup holds for its account. It answers that device `409`, before an upload stores anything, and then every device of the account syncs everything again. Once other devices' writes have passed those cursors, nothing shows the restore, and what the backup lacks stays lost.

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
