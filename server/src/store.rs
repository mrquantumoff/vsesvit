//! The records table: last body wins per `(account, kind, id)`, every write takes the account's
//! next sequence number.
//!
//! Every transaction that writes an account's records locks its `accounts` row first, by
//! updating it. That orders one account's uploads, so they commit in sequence order and a
//! download never passes a number a still-open upload holds, and it gives uploads and deletes one
//! lock order, so they cannot deadlock.

use std::collections::HashMap;

use chrono::Utc;
use sea_orm::sea_query::{Expr, ExprTrait, OnConflict};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbErr, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, TransactionTrait,
};
use sha2::{Digest, Sha256};
use vsesvit_sync_proto::{Page, Record};

use crate::auth::{PendingLogin, Person, random_token};
use crate::entities::{accounts, logins, records, sessions};

/// Rows per INSERT and per `IN` list, under SQLite's and Postgres's bind parameter limits.
const CHUNK: usize = 1000;
/// Rows a download reads at a time while it fills its byte budget.
const DOWNLOAD_CHUNK: u64 = 32;

pub type AccountId = i64;

/// What one account may store. A record counts its body and its id.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Quota {
    pub max_bytes: i64,
    pub max_records: i64,
}

/// A cursor is past the account's sequence number, which only happens when the database went back
/// to an older copy.
const CURSOR_AHEAD: &str = "the server holds fewer writes than this cursor has seen; sync everything again";

#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error(transparent)]
    Db(#[from] DbErr),
    #[error("the account's storage quota is full")]
    OverQuota,
    #[error("{CURSOR_AHEAD}")]
    CursorAhead,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Db(#[from] DbErr),
    #[error("{CURSOR_AHEAD}")]
    CursorAhead,
}

/// The person's account, made the first time they sign in. `None` when they have none and the
/// server has `max_accounts` already; a few signing up at once may pass it by one each.
pub async fn account(db: &DatabaseConnection, issuer: &str, subject: &str, max_accounts: Option<u64>) -> Result<Option<AccountId>, DbErr> {
    if let Some(id) = find_account(db, issuer, subject).await? {
        return Ok(Some(id));
    }
    if let Some(max) = max_accounts
        && accounts::Entity::find().count(db).await? >= max
    {
        return Ok(None);
    }
    let row = accounts::ActiveModel {
        issuer: Set(issuer.to_owned()),
        subject: Set(subject.to_owned()),
        seq: Set(0),
        stored_bytes: Set(0),
        record_count: Set(0),
        created_at: Set(Utc::now()),
        ..Default::default()
    };
    let conflict = OnConflict::columns([accounts::Column::Issuer, accounts::Column::Subject]).do_nothing().to_owned();
    // A concurrent first request may have inserted it; either way the row exists now.
    let _ = accounts::Entity::insert(row).on_conflict(conflict).exec_without_returning(db).await?;
    find_account(db, issuer, subject).await?.ok_or_else(|| DbErr::RecordNotFound("the account just inserted".to_owned())).map(Some)
}

async fn find_account(db: &impl ConnectionTrait, issuer: &str, subject: &str) -> Result<Option<AccountId>, DbErr> {
    let found = accounts::Entity::find()
        .filter(accounts::Column::Issuer.eq(issuer))
        .filter(accounts::Column::Subject.eq(subject))
        .one(db)
        .await?;
    Ok(found.map(|a| a.id))
}

fn id_hash(id: &str) -> Vec<u8> {
    Sha256::digest(id.as_bytes()).to_vec()
}

fn size(record: &Record) -> i64 {
    (record.body.len() + record.id.len()) as i64
}

/// Locks the account's row and moves its counters. Returns the row as it is afterwards.
async fn bump(txn: &DatabaseTransaction, account: AccountId, seq: i64, bytes: i64, count: i64) -> Result<accounts::Model, DbErr> {
    let updated = accounts::Entity::update_many()
        .col_expr(accounts::Column::Seq, Expr::col(accounts::Column::Seq).add(seq))
        .col_expr(accounts::Column::StoredBytes, Expr::col(accounts::Column::StoredBytes).add(bytes))
        .col_expr(accounts::Column::RecordCount, Expr::col(accounts::Column::RecordCount).add(count))
        .filter(accounts::Column::Id.eq(account))
        .exec_with_returning(txn)
        .await?;
    updated.into_iter().next().ok_or_else(|| DbErr::RecordNotFound(format!("account {account}")))
}

/// Starts a new epoch for the account: a cursor past its sequence number showed that it went back,
/// and every device of it is to sync everything again.
async fn went_back(db: &DatabaseConnection, account: AccountId) -> Result<(), DbErr> {
    tracing::warn!(account, "a cursor is past every write of the account, as after a restore from a backup; its devices sync everything again");
    accounts::Entity::update_many()
        .col_expr(accounts::Column::Epoch, Expr::col(accounts::Column::Epoch).add(1))
        .filter(accounts::Column::Id.eq(account))
        .exec(db)
        .await
        .map(drop)
}

/// Stores each record's body over what was there. When one upload holds the same record twice,
/// the later one wins. An upload that would take the account over `quota` stores nothing; one
/// that does not grow it is always stored. One whose `download_cursor` is past every write of the
/// account stores nothing either. Returns how many records were stored.
pub async fn upload(
    db: &DatabaseConnection,
    account: AccountId,
    uploaded: Vec<Record>,
    download_cursor: u64,
    quota: Quota,
) -> Result<u32, UploadError> {
    let mut last: HashMap<(u8, &str), usize> = HashMap::new();
    for (i, r) in uploaded.iter().enumerate() {
        last.insert((r.kind, &r.id), i);
    }
    let keep: Vec<bool> = uploaded.iter().enumerate().map(|(i, r)| last[&(r.kind, r.id.as_str())] == i).collect();
    let records: Vec<(Vec<u8>, Record)> =
        uploaded.into_iter().zip(keep).filter_map(|(r, k)| k.then(|| (id_hash(&r.id), r))).collect();
    let count = records.len() as i64;
    if count == 0 {
        return Ok(0);
    }
    let txn = db.begin().await?;
    let locked = bump(&txn, account, count, 0, 0).await?;
    if i64::try_from(download_cursor).unwrap_or(i64::MAX) > locked.seq - count {
        txn.rollback().await?;
        went_back(db, account).await?;
        return Err(UploadError::CursorAhead);
    }

    let mut stored: HashMap<(i16, Vec<u8>), i64> = HashMap::new();
    for chunk in records.chunks(CHUNK) {
        let hashes: Vec<Vec<u8>> = chunk.iter().map(|(h, _)| h.clone()).collect();
        let rows: Vec<(i16, Vec<u8>, i64)> = records::Entity::find()
            .select_only()
            .columns([records::Column::Kind, records::Column::IdHash, records::Column::Size])
            .filter(records::Column::AccountId.eq(account))
            .filter(records::Column::IdHash.is_in(hashes))
            .into_tuple()
            .all(&txn)
            .await?;
        stored.extend(rows.into_iter().map(|(kind, hash, size)| ((kind, hash), size)));
    }
    let (mut bytes, mut added) = (0i64, 0i64);
    for (hash, r) in &records {
        match stored.get(&(i16::from(r.kind), hash.clone())) {
            Some(old) => bytes += size(r) - old,
            None => {
                bytes += size(r);
                added += 1;
            }
        }
    }
    let grows = bytes > 0 || added > 0;
    if grows && (locked.stored_bytes + bytes > quota.max_bytes || locked.record_count + added > quota.max_records) {
        txn.rollback().await?;
        return Err(UploadError::OverQuota);
    }

    let now = Utc::now();
    let rows: Vec<records::ActiveModel> = records
        .into_iter()
        .zip(locked.seq - count + 1..)
        .map(|((hash, r), seq)| records::ActiveModel {
            account_id: Set(account),
            kind: Set(i16::from(r.kind)),
            id_hash: Set(hash),
            size: Set(size(&r)),
            record_id: Set(r.id),
            body: Set(r.body),
            seq: Set(seq),
            updated_at: Set(now),
        })
        .collect();
    let mut rows = rows.into_iter().peekable();
    while rows.peek().is_some() {
        let chunk: Vec<_> = rows.by_ref().take(CHUNK).collect();
        let conflict = OnConflict::columns([records::Column::AccountId, records::Column::Kind, records::Column::IdHash])
            .update_columns([
                records::Column::RecordId,
                records::Column::Body,
                records::Column::Size,
                records::Column::Seq,
                records::Column::UpdatedAt,
            ])
            .to_owned();
        records::Entity::insert_many(chunk).on_conflict(conflict).exec_without_returning(&txn).await?;
    }
    bump(&txn, account, 0, bytes, added).await?;
    txn.commit().await?;
    Ok(count as u32)
}

/// Records written after `since`, oldest write first: at most `limit`, and no more than
/// `max_bytes` of JSON unless one record alone is larger. Rows are read a few at a time, so a page
/// holds about `max_bytes` in memory however large the records are. The page has the account's
/// epoch.
pub async fn download(db: &DatabaseConnection, account: AccountId, since: u64, limit: u32, max_bytes: usize) -> Result<Page, DownloadError> {
    let since = i64::try_from(since).unwrap_or(i64::MAX);
    let row = accounts::Entity::find_by_id(account).one(db).await?;
    let (seq, epoch) = row.map_or((0, 0), |a| (a.seq, a.epoch as u64));
    if since > seq {
        went_back(db, account).await?;
        return Err(DownloadError::CursorAhead);
    }
    let mut cursor = since;
    let mut records = Vec::new();
    let mut bytes = 0;
    loop {
        let rows = records::Entity::find()
            .filter(records::Column::AccountId.eq(account))
            .filter(records::Column::Seq.gt(cursor))
            .order_by_asc(records::Column::Seq)
            .limit(DOWNLOAD_CHUNK)
            .all(db)
            .await?;
        let exhausted = rows.len() < DOWNLOAD_CHUNK as usize;
        for row in rows {
            // base64 grows a body by a third; the rest is the JSON around it.
            let json = row.body.len().div_ceil(3) * 4 + row.record_id.len() + 64;
            if records.len() == limit as usize || (!records.is_empty() && bytes + json > max_bytes) {
                return Ok(Page { records, cursor: cursor as u64, more: true, epoch });
            }
            bytes += json;
            cursor = row.seq;
            if let Ok(kind) = u8::try_from(row.kind) {
                records.push(Record { kind, id: row.record_id, body: row.body });
            }
        }
        if exhausted {
            return Ok(Page { records, cursor: cursor as u64, more: false, epoch });
        }
    }
}

/// Deletes every record of the account, and signs out every device: one that signs in again starts
/// its cursors over, so it uploads everything it holds. The account stays, with its sequence
/// number, so a cursor held from before still sees what is uploaded afterwards.
pub async fn delete_records(db: &DatabaseConnection, account: AccountId) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    let locked = bump(&txn, account, 0, 0, 0).await?;
    records::Entity::delete_many().filter(records::Column::AccountId.eq(account)).exec(&txn).await?;
    sessions::Entity::delete_many().filter(sessions::Column::AccountId.eq(account)).exec(&txn).await?;
    bump(&txn, account, 0, -locked.stored_bytes, -locked.record_count).await?;
    txn.commit().await
}

/// How long a sign-in may take at the provider, and how long its one-time code stays valid.
pub const LOGIN_LIFETIME: chrono::Duration = chrono::Duration::minutes(10);
pub const CODE_LIFETIME: chrono::Duration = chrono::Duration::minutes(2);
/// A session's last use is written at most this often, not on every request.
const TOUCH_EVERY: chrono::Duration = chrono::Duration::hours(1);

pub fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// The provider vouched for the person: the sign-in is stored, waiting for the browser to trade
/// the one-time code whose hash this stores. Sign-ins whose code has expired are cleared first: one
/// that started before `LOGIN_LIFETIME + CODE_LIFETIME` was authorized before `CODE_LIFETIME`.
pub async fn authorize_login(db: &DatabaseConnection, pending: PendingLogin, account: AccountId, person: &Person, code_hash: Vec<u8>) -> Result<(), DbErr> {
    let now = Utc::now();
    logins::Entity::delete_many().filter(logins::Column::CreatedAt.lt(now - LOGIN_LIFETIME - CODE_LIFETIME)).exec(db).await?;
    let row = logins::ActiveModel {
        id: Set(random_token()),
        client_redirect: Set(pending.login.client_redirect),
        client_state: Set(pending.login.client_state),
        client_challenge: Set(pending.login.client_challenge),
        upstream_verifier: Set(pending.upstream_verifier),
        code_hash: Set(Some(code_hash)),
        account_id: Set(Some(account)),
        name: Set(person.name.clone()),
        claims: Set(Some(serde_json::to_string(&person.claims).expect("the claims serialize"))),
        created_at: Set(pending.started),
        authorized_at: Set(Some(now)),
    };
    logins::Entity::insert(row).exec_without_returning(db).await.map(drop)
}

/// Takes the sign-in a one-time code belongs to: it is deleted, so the code works once. `None` when
/// no sign-in has it, or it has expired.
pub async fn take_login(db: &DatabaseConnection, code_hash: &[u8]) -> Result<Option<logins::Model>, DbErr> {
    let txn = db.begin().await?;
    let found = logins::Entity::find().filter(logins::Column::CodeHash.eq(code_hash.to_vec())).one(&txn).await?;
    let Some(login) = found else {
        txn.commit().await?;
        return Ok(None);
    };
    let deleted = logins::Entity::delete_by_id(login.id.clone()).exec(&txn).await?.rows_affected;
    txn.commit().await?;
    let fresh = login.authorized_at.is_some_and(|at| at > Utc::now() - CODE_LIFETIME);
    Ok((deleted == 1 && fresh).then_some(login))
}

/// Sessions unused for longer than `idle` are cleared first, as the devices that held them may
/// never come back to end them.
pub async fn start_session(db: &DatabaseConnection, account: AccountId, token_hash: Vec<u8>, idle: chrono::Duration) -> Result<(), DbErr> {
    let now = Utc::now();
    sessions::Entity::delete_many().filter(sessions::Column::LastUsedAt.lt(now - idle)).exec(db).await?;
    let row = sessions::ActiveModel { token_hash: Set(token_hash), account_id: Set(account), created_at: Set(now), last_used_at: Set(now) };
    sessions::Entity::insert(row).exec_without_returning(db).await.map(drop)
}

/// The account a session token belongs to. A session unused for longer than `idle` ends here.
pub async fn session_account(db: &DatabaseConnection, token_hash: &[u8], idle: chrono::Duration) -> Result<Option<AccountId>, DbErr> {
    let Some(session) = sessions::Entity::find_by_id(token_hash.to_vec()).one(db).await? else {
        return Ok(None);
    };
    let now = Utc::now();
    if session.last_used_at < now - idle {
        sessions::Entity::delete_by_id(token_hash.to_vec()).exec(db).await?;
        return Ok(None);
    }
    if session.last_used_at < now - TOUCH_EVERY {
        sessions::Entity::update_many()
            .col_expr(sessions::Column::LastUsedAt, Expr::value(now))
            .filter(sessions::Column::TokenHash.eq(token_hash.to_vec()))
            .exec(db)
            .await?;
    }
    Ok(Some(session.account_id))
}

pub async fn end_session(db: &DatabaseConnection, token_hash: &[u8]) -> Result<(), DbErr> {
    sessions::Entity::delete_by_id(token_hash.to_vec()).exec(db).await.map(drop)
}
