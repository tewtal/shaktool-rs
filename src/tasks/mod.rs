use std::time::Duration;

use async_trait::async_trait;
use poise::serenity_prelude as serenity;
use tracing::{info, warn};

use crate::db::Db;
use crate::Error;

pub mod speedrun;

/// Shared context handed to every background task run, providing access to
/// Discord (via the serenity context) and the persistent database.
pub struct TaskContext {
    pub ctx: serenity::Context,
    pub db: Db,
}

/// A background task that runs on a fixed interval.
///
/// To add a new task: implement this trait, then register the task in [`tasks()`].
/// The first run happens immediately on startup, then every `interval()` after that.
/// Errors are logged and the task keeps running on its schedule.
#[async_trait]
pub trait Task: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn interval(&self) -> Duration;
    async fn run(&self, task_ctx: &TaskContext) -> Result<(), Error>;
}

fn tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(speedrun::SpeedrunMonitor::new()),
    ]
}

/// Spawns all registered background tasks. Called once when the bot is ready.
pub fn start(ctx: serenity::Context, db: Db) {
    for task in tasks() {
        let task_ctx = TaskContext { ctx: ctx.clone(), db: db.clone() };
        tokio::spawn(async move {
            info!("Started background task '{}' (interval: {:?})", task.name(), task.interval());
            let mut interval = tokio::time::interval(task.interval());
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                if let Err(e) = task.run(&task_ctx).await {
                    warn!("Background task '{}' failed: {:?}", task.name(), e);
                }
            }
        });
    }
}
