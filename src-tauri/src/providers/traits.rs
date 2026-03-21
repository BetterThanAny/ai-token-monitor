use super::types::AllStats;

pub trait TokenProvider: Send + Sync {
    fn name(&self) -> &str;
    fn fetch_stats(&self) -> Result<AllStats, String>;
    fn is_available(&self) -> bool;
}
