use crate::application::{ContentService, TickerService};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub content_service: Arc<ContentService>,
    pub ticker_service: Arc<TickerService>,
}
