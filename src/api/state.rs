use crate::application::ContentService;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub content_service: Arc<ContentService>,
}
