use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EventStatus {
    Received,
    Processing,
    Completed,
    FailedRetry,
    Failed,
}

impl EventStatus {
    pub fn can_transition(self, next: EventStatus) -> bool {
        match (self, next) {
            (EventStatus::Received, EventStatus::Processing) => true,
            (EventStatus::Processing, EventStatus::Completed) => true,
            (EventStatus::Processing, EventStatus::FailedRetry) => true,
            (EventStatus::Processing, EventStatus::Failed) => true,
            _ => false,
        }
    }
}

impl Display for EventStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EventStatus::Received => "Received",
            EventStatus::Processing => "Processing",
            EventStatus::Completed => "Completed",
            EventStatus::FailedRetry => "FailedRetry",
            EventStatus::Failed => "Failed",
        };
        write!(f, "{}", s)
    }
}