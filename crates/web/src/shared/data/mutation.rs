#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MutationStatus {
    Idle,
    Pending,
    Success,
    Failed(String),
}

impl MutationStatus {
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }
}
