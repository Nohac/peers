use anyhow::Result;
use thiserror::Error;

use crate::review_provider::ReviewProvider;

pub use crate::realtime::ReviewUpdate;
pub use crate::review_provider::{
    CommentRequest, CreateThreadRequest, EditCommentRequest, ReviewComment, ReviewCommit,
    ReviewProjection, ReviewThread, ReviewThreadAnchor, ThreadBodyRequest, ThreadRequest,
};

#[derive(Debug, Error)]
enum ReviewApiError {
    #[error("invalid review session token")]
    InvalidToken,
}

#[vox::service]
pub trait PeersReview {
    async fn get_review(&self, token: String) -> std::result::Result<ReviewProjection, String>;
    async fn subscribe_updates(
        &self,
        token: String,
        updates: vox::Tx<ReviewUpdate>,
    ) -> std::result::Result<(), String>;
    async fn refresh_diff(&self, token: String) -> std::result::Result<ReviewProjection, String>;
    async fn create_thread(
        &self,
        token: String,
        request: CreateThreadRequest,
    ) -> std::result::Result<ReviewProjection, String>;
    async fn reply_to_thread(
        &self,
        token: String,
        request: ThreadBodyRequest,
    ) -> std::result::Result<ReviewProjection, String>;
    async fn edit_comment(
        &self,
        token: String,
        request: EditCommentRequest,
    ) -> std::result::Result<ReviewProjection, String>;
    async fn delete_comment(
        &self,
        token: String,
        request: CommentRequest,
    ) -> std::result::Result<ReviewProjection, String>;
    async fn delete_thread(
        &self,
        token: String,
        request: ThreadRequest,
    ) -> std::result::Result<ReviewProjection, String>;
    async fn resolve_thread(
        &self,
        token: String,
        request: ThreadRequest,
    ) -> std::result::Result<ReviewProjection, String>;
    async fn reopen_thread(
        &self,
        token: String,
        request: ThreadRequest,
    ) -> std::result::Result<ReviewProjection, String>;
    async fn toggle_thread_collapsed(
        &self,
        token: String,
        request: ThreadRequest,
    ) -> std::result::Result<ReviewProjection, String>;
}

#[derive(Clone)]
pub struct ReviewApi {
    provider: ReviewProvider,
    token: String,
}

impl ReviewApi {
    pub fn new(provider: ReviewProvider, token: String) -> Self {
        Self { provider, token }
    }

    fn check_token(&self, token: &str) -> Result<()> {
        if token != self.token {
            return Err(ReviewApiError::InvalidToken.into());
        }
        Ok(())
    }
}

impl PeersReview for ReviewApi {
    async fn get_review(&self, token: String) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider.get_review().await.map_err(format_error)
    }

    async fn subscribe_updates(
        &self,
        token: String,
        updates: vox::Tx<ReviewUpdate>,
    ) -> std::result::Result<(), String> {
        self.check_token(&token).map_err(format_error)?;
        let mut receiver = self.provider.updates().subscribe();
        tokio::spawn(async move {
            while let Ok(update) = receiver.recv().await {
                if updates.send(update).await.is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    async fn refresh_diff(&self, token: String) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider.refresh_diff().await.map_err(format_error)
    }

    async fn create_thread(
        &self,
        token: String,
        request: CreateThreadRequest,
    ) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider
            .create_thread(request)
            .await
            .map_err(format_error)
    }

    async fn reply_to_thread(
        &self,
        token: String,
        request: ThreadBodyRequest,
    ) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider
            .reply_to_thread(request)
            .await
            .map_err(format_error)
    }

    async fn edit_comment(
        &self,
        token: String,
        request: EditCommentRequest,
    ) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider
            .edit_comment(request)
            .await
            .map_err(format_error)
    }

    async fn delete_comment(
        &self,
        token: String,
        request: CommentRequest,
    ) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider
            .delete_comment(request)
            .await
            .map_err(format_error)
    }

    async fn delete_thread(
        &self,
        token: String,
        request: ThreadRequest,
    ) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider
            .delete_thread(request)
            .await
            .map_err(format_error)
    }

    async fn resolve_thread(
        &self,
        token: String,
        request: ThreadRequest,
    ) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider
            .resolve_thread(request)
            .await
            .map_err(format_error)
    }

    async fn reopen_thread(
        &self,
        token: String,
        request: ThreadRequest,
    ) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider
            .reopen_thread(request)
            .await
            .map_err(format_error)
    }

    async fn toggle_thread_collapsed(
        &self,
        token: String,
        request: ThreadRequest,
    ) -> std::result::Result<ReviewProjection, String> {
        self.check_token(&token).map_err(format_error)?;
        self.provider
            .toggle_thread_collapsed(request)
            .await
            .map_err(format_error)
    }
}

fn format_error(error: anyhow::Error) -> String {
    format!("{error:#}")
}
//test
// peers realtime manual rpc
