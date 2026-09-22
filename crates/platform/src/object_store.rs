use std::time::Duration;

use aws_sdk_s3::Client;
use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::presigning::PresigningConfig;
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

#[async_trait::async_trait]
pub trait ObjectStore: Send + Sync {
    async fn presign_put(&self, key: &str, ttl: Duration) -> Result<Url, StorageError>;
    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<Url, StorageError>;
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>, StorageError>;
    async fn delete_prefix(&self, prefix: &str) -> Result<u64, StorageError>;
}

pub struct S3ObjectStore {
    client: Client,
    bucket: String,
}

impl S3ObjectStore {
    pub fn new(endpoint: &str, bucket: &str, access_key: &str, secret_key: &str) -> Self {
        let credentials = Credentials::new(access_key, secret_key, None, None, "static");
        let config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .endpoint_url(endpoint)
            .credentials_provider(credentials)
            .force_path_style(true)
            .build();

        Self {
            client: Client::from_conf(config),
            bucket: bucket.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl ObjectStore for S3ObjectStore {
    async fn presign_put(&self, key: &str, ttl: Duration) -> Result<Url, StorageError> {
        let presigning =
            PresigningConfig::expires_in(ttl).map_err(|e| StorageError::Request(e.to_string()))?;
        let request = self
            .client
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
            .client
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
