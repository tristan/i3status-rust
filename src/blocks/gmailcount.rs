use std::time::Duration;
use crossbeam_channel::Sender;
use std::process::Command;

use crate::blocks::{Block, ConfigBlock, Update};
use crate::config::Config;
use crate::de::deserialize_duration;
use crate::errors::*;
use crate::widgets::text::TextWidget;
use crate::widget::{I3BarWidget, State};
use crate::input::I3BarEvent;
use crate::scheduler::Task;

use serde::Deserialize;
use uuid::Uuid;

pub struct GmailCount {
    text: TextWidget,
    id: String,
    update_interval: Duration,
    auth_base64: String,
    threshold_warning: usize,
    threshold_critical: usize,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct GmailCountConfig {
    /// Update interval in seconds
    #[serde(default = "GmailCountConfig::default_interval", deserialize_with = "deserialize_duration")]
    pub interval: Duration,
    pub auth_base64: String,
    #[serde(default = "GmailCountConfig::default_threshold_warning")]
    pub threshold_warning: usize,
    #[serde(default = "GmailCountConfig::default_threshold_critical")]
    pub threshold_critical: usize,
}

impl GmailCountConfig {
    fn default_interval() -> Duration {
        Duration::from_secs(60)
    }
    fn default_threshold_warning() -> usize {
        1 as usize
    }
    fn default_threshold_critical() -> usize {
        10 as usize
    }
}

impl ConfigBlock for GmailCount {
    type Config = GmailCountConfig;

    fn new(block_config: Self::Config, config: Config, _tx_update_request: Sender<Task>) -> Result<Self> {
        Ok(GmailCount {
            id: Uuid::new_v4().to_simple().to_string(),
            update_interval: block_config.interval,
            text: TextWidget::new(config.clone())
                .with_icon("mail")
                .with_text(""),
            auth_base64: block_config.auth_base64,
            threshold_warning: block_config.threshold_warning,
            threshold_critical: block_config.threshold_critical,
        })
    }
}

impl Block for GmailCount {
    fn update(&mut self) -> Result<Option<Update>> {
        if let Ok(output) = Command::new("curl")
            .args(&["-H", &["Authorization: Basic", &self.auth_base64].join(" "),
                    "https://mail.google.com/mail/feed/atom"])
            .output() {
                if output.status.success() {
                    if let Ok(data) = String::from_utf8(output.stdout) {
                        if let Some(idx_start) = data.find("<fullcount>") {
                            if let Some(idx_end) = data.find("</fullcount>") {
                                if let Ok(newmails) = data[idx_start+11..idx_end].parse::<usize>() {
                                    let state = {
                                        if newmails >= self.threshold_critical {
                                            State::Critical
                                        } else if newmails >= self.threshold_warning {
                                            State::Warning
                                        } else {
                                            State::Idle
                                        }
                                    };
                                    self.text.set_state(state);
                                    self.text.set_text(format!("{}", newmails));
                                }
                            }
                        }
                    }
                }
            }
        Ok(Some(Update::Every(self.update_interval)))
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }

    fn click(&mut self, _: &I3BarEvent) -> Result<()> {
        Ok(())
    }

    fn id(&self) -> &str {
        &self.id
    }
}
