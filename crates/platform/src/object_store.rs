use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use aws_sdk_s3::Client;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use tokio::io::AsyncRead;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("object store request failed: {0}")]
    Request(String),

    #[error("presigned url was not valid: {0}")]
    InvalidUrl(#[from] url::ParseError),
}

#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub size: u64,
    pub content_type: Option<String>,
}

/// An object's bytes, streamed (a chunk, a source file): never buffered whole in memory.
pub type ObjectReader = Pin<Box<dyn AsyncRead + Send>>;

#[async_trait::async_trait]
pub trait ObjectStore: Send + Sync {
    async fn presign_put(&self, key: &str, ttl: Duration) -> Result<Url, StorageError>;
    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<Url, StorageError>;
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>, StorageError>;
    async fn delete_prefix(&self, prefix: &str) -> Result<u64, StorageError>;
    /// Streams the object at `key`, or `None` if there is none.
    async fn get(&self, key: &str) -> Result<Option<ObjectReader>, StorageError>;
    /// Uploads a local file to `key` (renditions written by the worker). Returns its size.
    async fn put_file(
        &self,
        key: &str,
        path: &Path,
        content_type: &str,
    ) -> Result<u64, StorageError>;
}

pub struct S3ObjectStore {
    client: Client,
    /// Signs presigned URLs. The same as `client` unless browsers reach storage at a different
    /// address than this process does (ADR-0010).
    presign_client: Client,
    bucket: String,
}

impl S3ObjectStore {
    pub fn new(endpoint: &str, bucket: &str, access_key: &str, secret_key: &str) -> Self {
        let client = s3_client(endpoint, access_key, secret_key);
        Self {
            presign_client: client.clone(),
            client,
            bucket: bucket.to_string(),
        }
    }

    /// Presigned URLs are signed for `public_endpoint` (the address browsers use, e.g. a CDN
    /// host, or in dev the HTTPS dev server that proxies to MinIO). SigV4 covers the host, so
    /// the URL must be signed for the host the request will carry. Everything else keeps using
    /// the internal endpoint.
    pub fn with_public_endpoint(
        mut self,
        public_endpoint: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Self {
        self.presign_client = s3_client(public_endpoint, access_key, secret_key);
        self
    }
}

fn s3_client(endpoint: &str, access_key: &str, secret_key: &str) -> Client {
    let credentials = Credentials::new(access_key, secret_key, None, None, "static");
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .endpoint_url(endpoint)
        .credentials_provider(credentials)
        .force_path_style(true)
        .build();
    Client::from_conf(config)
}

#[async_trait::async_trait]
impl ObjectStore for S3ObjectStore {
    async fn presign_put(&self, key: &str, ttl: Duration) -> Result<Url, StorageError> {
        let presigning =
            PresigningConfig::expires_in(ttl).map_err(|e| StorageError::Request(e.to_string()))?;
        let request = self
            .presign_client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning)
            .await
            .map_err(|e| StorageError::Request(e.to_string()))?;
        Ok(Url::parse(request.uri())?)
    }

    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<Url, StorageError> {
        let presigning =
            PresigningConfig::expires_in(ttl).map_err(|e| StorageError::Request(e.to_string()))?;
        let request = self
            .presign_client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning)
            .await
            .map_err(|e| StorageError::Request(e.to_string()))?;
        Ok(Url::parse(request.uri())?)
    }

    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>, StorageError> {
        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        match result {
            Ok(output) => Ok(Some(ObjectMeta {
                size: output.content_length().unwrap_or_default().max(0) as u64,
                content_type: output.content_type().map(str::to_string),
            })),
            Err(SdkError::ServiceError(service_error)) if service_error.err().is_not_found() => {
                Ok(None)
            }
            Err(error) => Err(StorageError::Request(error.to_string())),
        }
    }

    async fn get(&self, key: &str) -> Result<Option<ObjectReader>, StorageError> {
        let result = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;
        match result {
            Ok(output) => Ok(Some(Box::pin(output.body.into_async_read()))),
            Err(SdkError::ServiceError(service_error)) if service_error.err().is_no_such_key() => {
                Ok(None)
            }
            Err(error) => Err(StorageError::Request(error.to_string())),
        }
    }

    async fn put_file(
        &self,
        key: &str,
        path: &Path,
        content_type: &str,
    ) -> Result<u64, StorageError> {
        let size = tokio::fs::metadata(path)
            .await
            .map_err(|e| StorageError::Request(format!("{}: {e}", path.display())))?
            .len();
        let body = ByteStream::from_path(path)
            .await
            .map_err(|e| StorageError::Request(e.to_string()))?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(body)
            .send()
            .await
            .map_err(|e| StorageError::Request(e.to_string()))?;
        Ok(size)
    }

    async fn delete_prefix(&self, prefix: &str) -> Result<u64, StorageError> {
        let mut deleted = 0u64;
        let mut continuation_token = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);
            if let Some(token) = continuation_token.take() {
                request = request.continuation_token(token);
            }
            let output = request
                .send()
                .await
                .map_err(|e| StorageError::Request(e.to_string()))?;

            for object in output.contents() {
                if let Some(key) = object.key() {
                    self.client
                        .delete_object()
                        .bucket(&self.bucket)
                        .key(key)
                        .send()
                        .await
                        .map_err(|e| StorageError::Request(e.to_string()))?;
                    deleted += 1;
                }
            }

            if output.is_truncated().unwrap_or(false) {
                continuation_token = output.next_continuation_token().map(str::to_string);
            } else {
                break;
            }
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> S3ObjectStore {
        S3ObjectStore::new(
            &std::env::var("S3_ENDPOINT").expect("S3_ENDPOINT set"),
            &std::env::var("S3_BUCKET").expect("S3_BUCKET set"),
            &std::env::var("S3_ACCESS_KEY").expect("S3_ACCESS_KEY set"),
            &std::env::var("S3_SECRET_KEY").expect("S3_SECRET_KEY set"),
        )
    }

    #[tokio::test]
    async fn put_file_then_get_streams_it_back() {
        use tokio::io::AsyncReadExt;

        let store = test_store();
        let prefix = format!("test/put-file-{}/", uuid::Uuid::now_v7());
        let key = format!("{prefix}source.webm");
        let path = std::env::temp_dir().join(format!("sintade-put-file-{}", uuid::Uuid::now_v7()));
        let body: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
        tokio::fs::write(&path, &body)
            .await
            .expect("write temp file");

        let size = store
            .put_file(&key, &path, "video/webm")
            .await
            .expect("put_file succeeds");
        assert_eq!(size, body.len() as u64);
        let meta = store.head(&key).await.expect("head").expect("exists");
        assert_eq!(meta.content_type.as_deref(), Some("video/webm"));

        let mut reader = store.get(&key).await.expect("get").expect("exists");
        let mut downloaded = Vec::new();
        reader.read_to_end(&mut downloaded).await.expect("read");
        assert_eq!(downloaded, body);
        assert!(
            store
                .get(&format!("{prefix}missing"))
                .await
                .expect("get")
                .is_none()
        );

        store.delete_prefix(&prefix).await.expect("tidy");
        tokio::fs::remove_file(&path).await.expect("tidy");
    }

    #[tokio::test]
    async fn uploads_and_downloads_through_presigned_urls_then_deletes() {
        let store = test_store();
        let key = "test/day5-lifecycle/object.txt";
        let body = b"hello from the day 5 integration test";
        let http = reqwest::Client::new();

        let put_url = store
            .presign_put(key, Duration::from_secs(60))
            .await
            .expect("presign_put succeeds");
        let put_response = http
            .put(put_url)
            .body(body.to_vec())
            .send()
            .await
            .expect("PUT to presigned url succeeds");
        assert!(put_response.status().is_success());

        let meta = store
            .head(key)
            .await
            .expect("head succeeds")
            .expect("object exists after upload");
        assert_eq!(meta.size, body.len() as u64);

        let get_url = store
            .presign_get(key, Duration::from_secs(60))
            .await
            .expect("presign_get succeeds");
        let get_response = http
            .get(get_url)
            .send()
            .await
            .expect("GET from presigned url succeeds");
        assert!(get_response.status().is_success());
        let downloaded = get_response.bytes().await.expect("read body");
        assert_eq!(&downloaded[..], body);

        let deleted = store
            .delete_prefix("test/day5-lifecycle/")
            .await
            .expect("delete_prefix succeeds");
        assert_eq!(deleted, 1);

        let meta_after_delete = store.head(key).await.expect("head succeeds");
        assert!(meta_after_delete.is_none());
    }
}
