# End-to-end encrypted sync

Sync records are encrypted on the device with keys only the person's devices hold, so the sync server stores ciphertext
under opaque ids. The server is unchanged: it never read bodies, and the opaque ids and two new kind codes fit its
protocol as it is. The formats are in `crates/vsesvit-sync/src/crypto.rs`, the rounds and the passphrase steps in
`engine.rs`; core's records, merge and `SyncStore` are untouched. Encryption is not optional: a device syncs nothing
until the account's sync passphrase is set or entered on it, and never uploads plaintext. An adversarial review of a
first draft shaped this design; what it changed is under "Alternatives not taken".

## Keys

A **sync passphrase** protects a **keyring** of random keys; nothing is derived from the passphrase but the key that
wraps the keyring.

- `id_key`, 32 random bytes, made once when the passphrase is first set and kept through every change. It turns a
  record's kind and id into the id the server sees.
- `current`, the record key: a random 16-byte key id and a random 32-byte XChaCha20-Poly1305 key. Records are sealed
  with it, and only records sealed with it are opened.
- `replaced`, one entry per passphrase change: a replaced key's id and a SHA-256 hash of it. The ids let a device tell
  a stale record from a foreign one; the hashes let a later keyring prove it descends from this one. Neither opens
  anything, so the current passphrase does not open records sealed before a change.

The keyring's generation is the number of keys it replaced. On each device it is sealed by the profile vault (DPAPI on
Windows, the Secret Service on Linux, as the sync session is), under `sync_secrets` key `account.keyring`. Signing out
forgets it, so signing in again asks for the passphrase again. The passphrase is at least 8 characters, taken in
Unicode NFC (RFC 8265) so that it derives the same key however a keyboard composed it. It never leaves the device and is
not stored.

## The key record

Wire kind 200, id `keys`. Its body is JSON the server can read:

```json
{ "version": 1, "generation": 2, "key_id": "<base64, the current key's id>",
  "kdf": { "memory_kib": 65536, "iterations": 3, "parallelism": 1, "salt": "<base64, 16 random bytes>" },
  "nonce": "<base64, 24 random bytes>", "keyring": "<base64, ciphertext>" }
```

`KEK = Argon2id(passphrase, salt, 64 MiB, 3 iterations, 1 lane)`, 32 bytes. `keyring` is the keyring's JSON sealed with
XChaCha20-Poly1305 under the KEK, with associated data
`"vsesvit-sync keys\0" ‖ version ‖ generation (u32 LE) ‖ key_id ‖ memory_kib ‖ iterations ‖ parallelism (u32 LE each) ‖ salt`.
Opening it is the **key check**: it succeeds only with the right passphrase and an unaltered record, and the keyring
inside must then be of the record's generation and key id. Format 1 takes exactly these parameters (RFC 9106's second
recommendation, one lane because the derivation runs on one thread), so a server cannot make a device spend gigabytes
on a derivation, or weaken it.

## Sealed records

Every record a device uploads is wire kind 201, with:

- **id**: `base64url(HMAC-SHA256(id_key, "vsesvit-sync id\0" ‖ kind ‖ id))`, 43 characters. The same record always
  lands in the same server slot, as the server's last-write-wins per slot needs, and the server learns neither the kind
  nor the id (history ids are URLs).
- **body**: `[format 1][16: key id][24: random nonce][XChaCha20-Poly1305 ciphertext and tag]`. The plaintext is
  `kind (1) ‖ id length (u32 LE) ‖ id ‖ body length (u32 LE) ‖ body`, zero-padded to the Padmé length of the whole,
  which leaks O(log log n) bits of its size for at most 12% more bytes. The associated data is
  `"vsesvit-sync record\0" ‖ format ‖ key id ‖ server id`, so a ciphertext moved to another slot does not open.

Opening checks the format, that the key id is the current key's (a replaced key's record is skipped as stale, any other
as foreign), the tag, the lengths and zero padding, and that the HMAC of the kind and id inside is the server id. A
record that fails is skipped and logged, and nothing of it is applied; core then validates the body as it always has.
A record that would be over the server's size limit once sealed is not uploaded.

**Nonces.** Every seal takes 24 fresh bytes from the OS random source. Nothing counts nonces, so a restored backup, a
copied profile or two devices sharing a key cannot repeat one; among 2^40 seals under one key a repeat has odds of
about 2^-112. XChaCha20 rather than AES-GCM because GCM's 12-byte random nonces allow only about 2^32 seals per key, and
history re-uploads every page on each passphrase change, server restore and new device.

## What a device does

`Account` keeps, besides the keyring: `own_keys`, its keyring as wrapped in the key record; `server_keys`, what it last
saw in the key slot (`Unknown`, `Missing` or `Found`); and four flags. What Settings asks for follows:

| keyring | key slot | Settings | the device |
|---|---|---|---|
| none | not looked through yet | "Checking…" | looks |
| none | a look found none | Set a passphrase | looks |
| none | a record | Enter the passphrase | looks |
| held | its own record, or an older one | Change the passphrase | syncs |
| held | a newer record, or another of its generation | Enter the new passphrase | looks |

A device that **looks** uploads nothing and applies nothing; it downloads pages only to note the newest key record.
Without keys it takes only a record at least as new as the last it saw; with keys it takes any record that would replace
them, and syncs again if its own record comes back. An account saved by an older build lacks the two kind codes in
`known_kinds`, so its first round starts the download over from 0, and the look covers the whole account.

A device that **syncs** seals what it gathered on the worker thread, uploads it, downloads a page and opens it there;
the UI thread applies what opened. A key record in the page that is its own needs nothing. An older one, or one altered
under its own generation and key id, is what a server restored from a backup (or a malicious one) holds, and the device
sends its own again (`upload_keys`). Any other means another device changed the passphrase: the whole page is dropped,
the cursor stays, and the device looks until the new passphrase is entered. Plaintext records are never applied.

**Setting** (no keyring, no key record found): make a keyring, wrap it, and start over (upload cursors cleared, download
cursor 0) with `pending` set. The key record goes up with the first round, or, on an account this profile synced before
encryption (`plaintext_trusted`, true only for an account an older build saved), after the download has passed through
the whole account once (`converting`): every non-empty plaintext record on the way is sealed into its slot as it is,
before the key record exists, so what gets sealed is only what the account held before it was encrypted. If the upload
of those copies fails the round fails and the page comes again. A record too large once sealed stays plaintext. While
`pending`, any key record other than the device's own means another device set a passphrase first: the device drops its
keys and asks for that passphrase; its own record coming back ends `pending`. The copies it made under the dropped key
are left on the server unread, and nothing was deleted, so nothing is lost.

**Entering**: open the found key record with the passphrase (on a worker; Argon2id). A device that holds keys takes the
new keyring only if it descends from them: the same `id_key`, its replaced keys in the same order, and the device's
current key next among them by id and hash. Then it stores it, starts over, and uploads nothing until its download has
reached the end (`joining`), so its older copies do not overwrite what other devices sealed since; after that it uploads
everything it holds. A wrong passphrase says so and changes nothing.

**Changing** (syncing): append a key, wrap the keyring under a new salt, store it, start over, and send the key record
with the next round. Everything the device holds is sealed again into the same slots, overwriting the old ciphertext.
Each other device stops at the new key record and asks for the new passphrase, then does the same.

**Restore.** When the server went back to an older copy of the account (the existing epoch and `409` paths), the device
starts over and sends its key record again, so a key record the backup lacks comes back.

A round or a passphrase step that outlives a change of the stored keys saves nothing: a round compares the stored
keyring, a passphrase step the stored keyring and key slot, with the ones it started from. The account's JSON is saved
before the keyring, so a device stopped between the two lacks the keyring its JSON names and asks for the passphrase,
rather than syncing with keys its cursors were not started over for.

## What a malicious server can still do

The server, or whoever controls it or its database, **cannot** read the contents or ids of sealed records, change one
without the change being refused, move one to another slot, or have a device apply plaintext once that device syncs. It
**can**:

- **See metadata.** How many records an account has, their padded sizes, which slots change and when, and which session
  uploads them; the key record's generation, so how many times and when the passphrase changed; the account's identity
  at the OpenID provider.
- **Guess the passphrase offline.** The key record lets it test guesses (sealed records do not: their keys are random).
  Argon2id at 64 MiB per guess makes that slow, not impossible. A long passphrase is the defense.
- **Withhold, delay, reorder, delete or replay.** It can serve an older ciphertext of a record, drop records, or serve
  nothing. A device that has a newer version keeps it, as the merge keeps the greater value, and sends it again in its
  next sync; a deletion stays deleted. A new device can still be shown an old copy of the account, or an empty one.
- **Leave a slot holding a record nobody opens.** A ciphertext moved into a slot, or one sealed with a key that was
  replaced, is skipped, and no device notices that it should send its own copy again, so a device signing in later lacks
  that record until some device changes it.
- **Hide the key record.** A new device then offers to set a passphrase. Setting one makes a second key. While the
  account has never changed its passphrase, the new record replaces the account's at the same generation, and the other
  devices stop and ask for the passphrase, and take the new key only after signing out and in again; otherwise they put
  their own record back, and the new device drops its key and asks for theirs. Nothing is read by the server, and a new device never seals plaintext it is shown (only a profile
  that synced the account before encryption does), so the server gains nothing but a split. The Set dialog tells
  people who set a passphrase elsewhere not to set another.
- **Copy an account.** Nothing binds a record to an account, so a server can copy one account's records and key record
  into another account of the same server; a device of the second opens them only with the first one's passphrase.
- **Deny service**: answer errors, show a device a forged key record so that it stops and asks for a passphrase that
  does not open it, or flip between key records.
- **Keep what it was given, and use an old passphrase.** Changing the passphrase does not take back ciphertext the
  server copied before, which the old passphrase still opens; the Change dialog says so. Whoever learns an old
  passphrase also holds `id_key`, so with the server they can test guesses of record ids (whether a URL is in the
  history) and watch when such a slot changes, for as long as the account lives, since `id_key` survives changes so
  that re-sealed records overwrite the old ones. They cannot open records sealed with a newer key, but a device that has
  not yet seen the change keeps sealing with the old key, and the server can keep hiding the change from it. They can
  also wrap a keyring of their own that descends from the one they opened; a device still at that generation takes it
  if its user types the old passphrase when asked for the new one (the dialog says never to), and a new device takes
  whatever key the passphrase typed opens.
- **Read the past.** Plaintext records uploaded before the account was encrypted, and anything an older Vsesvit uploads
  after, reached the server unencrypted and stay there, under their real ids, since the server has no way to delete one
  record and no device blanks them (a device that did could destroy data no other device has). "Delete Data on Server"
  removes them, with everything else: every device signs out, and once a passphrase is set again each uploads what it
  holds, sealed. Every device should run a Vsesvit with encryption, since an older one keeps uploading plaintext.
- **Have what it fed a device sealed.** A profile that synced the account before encryption trusted the server then,
  so what the server held when the passphrase was set is sealed as the account's. Any device also uploads, sealed,
  whatever it applied before it joined.

On the device, the keyring is as safe as the profile vault: with no Secret Service on Linux the vault key sits in the
profile database, and in effect so does the keyring.

## Alternatives not taken

- **Optional encryption, plaintext by default.** Chrome encrypts with a Google-held key unless a passphrase is set.
  Here the server is anyone's, so a default it can read is the case to avoid, and two modes double every state above.
- **Deriving record keys from the passphrase.** A weak passphrase would then weaken every record key rather than only
  the wrapping, and a change could not prove descent.
- **Keeping replaced keys usable.** Opening records sealed with a replaced key would let whoever learned the old
  passphrase forge records (an extension install, a bookmark) that devices still accept, and would let the current
  passphrase open what the server kept from before. Refusing them costs only records no device holds any more.
- **A new `id_key` on each change.** The server has no per-record delete, so re-sealed records would land in new slots
  and the old ciphertext would stay, readable with the old passphrase.
- **Blanking plaintext records after sealing them** (the first draft). The review found it loses data: a device that
  blanks a record another device has not sealed yet destroys what only the server held, and a device that seals under a
  key it then drops loses its copies. Leaving plaintext in place loses nothing, and the server had read it already.
- **Converting after the key record goes up** (the first draft). The server would then decide, page by page, what the
  converting device seals as genuine after the account is encrypted. Converting first, and only on a profile that
  trusted the server before, makes conversion no more trusting than the account already was.
- **Wider KDF bounds** (the first draft accepted 19 MiB to 1 GiB). A forged record could make every passphrase entry
  take tens of seconds or abort on a failed allocation, and genuine records only ever use one setting.
- **Reverting a lost change race.** When two devices change the passphrase at once, the server keeps one record, and
  the other device cannot take it (its own new key is not in the winner's keyring) until it signs out and in again.
  Keeping its previous keyring to fall back to would avoid that, at the cost of another stored secret for a rare race.
