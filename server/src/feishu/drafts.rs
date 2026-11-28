use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const MAX_DRAFTS: usize = 5;
pub const DRAFT_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone)]
pub struct Draft {
    pub id: String,
    pub text: String,
    pub image: Option<Vec<u8>>,
    pub created_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    AlreadySent,
    ExpiredOrMissing,
}

struct SentMarker {
    expires_at: Instant,
}

pub struct DraftStore {
    pending: Vec<Draft>,
    sent: HashMap<String, SentMarker>,
}

impl DraftStore {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            sent: HashMap::new(),
        }
    }

    pub fn insert(&mut self, draft: Draft, now: Instant) -> Option<String> {
        self.gc_sent(now);
        let mut evicted = None;
        if self.pending.len() >= MAX_DRAFTS {
            evicted = self.pending.first().map(|draft| draft.id.clone());
            self.pending.remove(0);
        }
        self.pending.push(draft);
        evicted
    }

    pub fn take_for_publish(&mut self, id: &str, now: Instant) -> Result<Draft, DraftError> {
        self.gc_sent(now);
        if self.sent.contains_key(id) {
            return Err(DraftError::AlreadySent);
        }
        let position = self.pending.iter().position(|draft| draft.id == id);
        let Some(position) = position else {
            return Err(DraftError::ExpiredOrMissing);
        };
        if now.duration_since(self.pending[position].created_at) >= DRAFT_TTL {
            self.pending.remove(position);
            return Err(DraftError::ExpiredOrMissing);
        }
        let draft = self.pending.remove(position);
        self.sent.insert(
            draft.id.clone(),
            SentMarker {
                expires_at: draft.created_at + DRAFT_TTL,
            },
        );
        Ok(draft)
    }

    fn gc_sent(&mut self, now: Instant) {
        self.sent.retain(|_, marker| marker.expires_at > now);
        self.pending
            .retain(|draft| now.duration_since(draft.created_at) < DRAFT_TTL);
    }
}

impl Default for DraftStore {
    fn default() -> Self {
        Self::new()
    }
}
