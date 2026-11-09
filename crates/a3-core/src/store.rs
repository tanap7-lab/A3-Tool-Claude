//! `AttachmentStore` is the boundary around file storage (D5). PostgreSQL today, in 1 MB chunks and
//! streamed, so a move to object storage replaces only `PgAttachmentStore`.

use crate::domain::sha256_hex;
use crate::error::{CoreError, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
const CHUNK: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct BlobInfo {
    pub hash: String,
    pub size: u64,
}

#[async_trait]
pub trait AttachmentStore: Send + Sync {
    /// Stores a file by content hash. Identical content is stored once.
    async fn put_file(&self, path: &Path) -> Result<BlobInfo>;
    async fn put_bytes(&self, data: &[u8]) -> Result<BlobInfo>;
    /// Writes a stored file out, verifying it against its hash.
    async fn get_to_file(&self, hash: &str, dest: &Path) -> Result<()>;
}

pub struct PgAttachmentStore {
    pool: PgPool,
}

impl PgAttachmentStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns true when the bytes are already fully stored.
    async fn begin_blob(&self, hash: &str, size: i64) -> Result<bool> {
        sqlx::query!(
            "INSERT INTO attachment_blob (hash, size_bytes) VALUES ($1, $2) ON CONFLICT (hash) DO NOTHING",
            hash, size
        )
        .execute(&self.pool)
        .await?;
        Ok(sqlx::query_scalar!("SELECT complete FROM attachment_blob WHERE hash = $1", hash)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn put_chunk(&self, hash: &str, seq: i32, data: &[u8]) -> Result<()> {
        sqlx::query!(
            "INSERT INTO attachment_chunk (hash, seq, data) VALUES ($1, $2, $3) ON CONFLICT (hash, seq) DO NOTHING",
            hash, seq, data
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn finish_blob(&self, hash: &str) -> Result<()> {
        sqlx::query!("UPDATE attachment_blob SET complete = true WHERE hash = $1", hash).execute(&self.pool).await?;
        Ok(())
    }
}

async fn hash_file(path: &Path) -> Result<(String, u64)> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > MAX_FILE_BYTES {
            return Err(CoreError::new("TOO_LARGE", "This file is over the 50 MB limit."));
        }
        h.update(&buf[..n]);
    }
    Ok((h.finalize().iter().map(|b| format!("{b:02x}")).collect(), size))
}

#[async_trait]
impl AttachmentStore for PgAttachmentStore {
    async fn put_file(&self, path: &Path) -> Result<BlobInfo> {
        let (hash, size) = hash_file(path).await?;
        if !self.begin_blob(&hash, size as i64).await? {
            let mut f = tokio::fs::File::open(path).await?;
            let mut buf = vec![0u8; CHUNK];
            let mut seq = 0;
            loop {
                let mut filled = 0;
                while filled < CHUNK {
                    let n = f.read(&mut buf[filled..]).await?;
                    if n == 0 {
                        break;
                    }
                    filled += n;
                }
                if filled == 0 {
                    break;
                }
                self.put_chunk(&hash, seq, &buf[..filled]).await?;
                seq += 1;
            }
            self.finish_blob(&hash).await?;
        }
        Ok(BlobInfo { hash, size })
    }

    async fn put_bytes(&self, data: &[u8]) -> Result<BlobInfo> {
        if data.len() as u64 > MAX_FILE_BYTES {
            return Err(CoreError::new("TOO_LARGE", "This file is over the 50 MB limit."));
        }
        let hash = sha256_hex(data);
        if !self.begin_blob(&hash, data.len() as i64).await? {
            for (seq, chunk) in data.chunks(CHUNK).enumerate() {
                self.put_chunk(&hash, seq as i32, chunk).await?;
            }
            self.finish_blob(&hash).await?;
        }
        Ok(BlobInfo { hash, size: data.len() as u64 })
    }

    async fn get_to_file(&self, hash: &str, dest: &Path) -> Result<()> {
        let count = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM attachment_chunk WHERE hash = $1"#, hash)
            .fetch_one(&self.pool)
            .await?;
        let mut out = tokio::fs::File::create(dest).await?;
        let mut h = Sha256::new();
        // One chunk per query, so a large file never sits in memory whole.
        for seq in 0..count as i32 {
            let data = sqlx::query_scalar!("SELECT data FROM attachment_chunk WHERE hash = $1 AND seq = $2", hash, seq)
                .fetch_one(&self.pool)
                .await?;
            h.update(&data);
            out.write_all(&data).await?;
        }
        out.flush().await?;
        drop(out);
        let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != hash {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(CoreError::new("CORRUPT", "This stored file failed its integrity check."));
        }
        Ok(())
    }
}
