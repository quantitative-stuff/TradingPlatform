use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{Event, Subscriber};
use tracing_subscriber::{layer::Context, Layer};
use super::QuestDBClient;

/// A tracing layer that captures all log messages and sends them to QuestDB
pub struct QuestDBLogLayer {
    questdb: Arc<RwLock<QuestDBClient>>,
}

impl QuestDBLogLayer {
    pub fn new(questdb: Arc<RwLock<QuestDBClient>>) -> Self {
        Self { questdb }
    }
}

impl<S> Layer<S> for QuestDBLogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // Extract log information
        let level = event.metadata().level().to_string();
        let target = event.metadata().target().to_string();
        
        // Get current thread info
        let thread = std::thread::current();
        let thread_id = format!("{:?}", thread.id());
        
        // Extract the log message
        let mut visitor = MessageVisitor::new();
        event.record(&mut visitor);
        let message = visitor.message;
        
        // Send to QuestDB asynchronously without blocking
        let questdb = self.questdb.clone();
        let level_clone = level.clone();
        let target_clone = target.clone();
        let thread_id_clone = thread_id.clone();
        let message_clone = message.clone();
        
        tokio::spawn(async move {
            let questdb = questdb.write().await;
            if let Err(e) = questdb.write_full_log(&level_clone, &target_clone, &thread_id_clone, &message_clone).await {
                // Don't spam console with QuestDB errors - just log critical ones
                if e.to_string().contains("10040") {
                    eprintln!("QuestDB: Message too long error (10040) - message truncated");
                } else if !e.to_string().contains("connection") {
                    eprintln!("QuestDB logging error: {}", e);
                }
            }
        });
    }
}

/// Visitor to extract the message from tracing events
struct MessageVisitor {
    message: String,
}

impl MessageVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
        }
    }
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
            // Remove the quotes that Debug formatting adds
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len()-1].to_string();
            }
        }
    }
    
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}