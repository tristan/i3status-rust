use x11::{
    xlib::{Display, XCloseDisplay, XDefaultRootWindow, XFree, XOpenDisplay},
    xss::{XScreenSaverAllocInfo, XScreenSaverInfo, XScreenSaverQueryInfo}
};

use std::{
    ptr,
    os::{
        raw::{c_void},
    },
    env,
    ffi::CString
};

use std::time::{Duration, Instant};
use crossbeam_channel::Sender;
use serde::Deserialize;

use crate::blocks::{Block, ConfigBlock, Update};
use crate::config::Config;
use crate::de::deserialize_duration;
use crate::errors::*;
use crate::widgets::text::TextWidget;
use crate::widget::{I3BarWidget, State};
use crate::input::I3BarEvent;
use crate::scheduler::Task;

use uuid::Uuid;

struct DeferXClose(*mut Display);
impl Drop for DeferXClose {
    fn drop(&mut self) {
        unsafe { XCloseDisplay(self.0); }
    }
}
struct DeferXFree(*mut c_void);
impl Drop for DeferXFree {
    fn drop(&mut self) {
        unsafe { XFree(self.0); }
    }
}

fn get_idle(display: *mut Display, info: *mut XScreenSaverInfo) -> Result<u64> {
    if unsafe { XScreenSaverQueryInfo(display, XDefaultRootWindow(display), info) } == 0 {
        // not supported
        Ok(0)
    } else {
        Ok(unsafe { (*info).idle })
    }
}

pub struct Activity {
    text: TextWidget,
    id: String,
    update_interval: Duration,
    reset_time: Duration,
    idle_threshold: Duration,
    start_time: Instant,
    display: *mut Display,
    info: *mut XScreenSaverInfo,
    _defer_free_display: DeferXClose,
    _defer_free_info: DeferXFree,
    idle_start_time: Instant,
    idle_last_reading: u64,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ActivityConfig {
    /// Update interval in seconds
    #[serde(default = "ActivityConfig::default_interval", deserialize_with = "deserialize_duration")]
    pub interval: Duration,

    /// Reset after idle time
    #[serde(default = "ActivityConfig::default_reset_time", deserialize_with = "deserialize_duration")]
    pub reset_time: Duration,

    /// Only count as idle if over the threshold
    #[serde(default = "ActivityConfig::default_idle_threshold", deserialize_with = "deserialize_duration")]
    pub idle_threshold: Duration,

}

impl ActivityConfig {
    fn default_interval() -> Duration {
        Duration::from_secs(1)
    }

    fn default_reset_time() -> Duration {
        Duration::from_secs(300)
    }

    fn default_idle_threshold() -> Duration {
        Duration::from_secs(10)
    }
}

impl ConfigBlock for Activity {
    type Config = ActivityConfig;

    fn new(block_config: Self::Config, config: Config, _send: Sender<Task>) -> Result<Self> {

        let id = Uuid::new_v4().to_simple().to_string();

        let (_disp_name_ptr, disp_name) = match env::var("DISPLAY") {
            Ok(name) => {
                let cstr = CString::new(name.as_str()).unwrap();
                (cstr.as_ptr(), name)
            },
            Err(_) => (ptr::null(), String::from("N/A"))
        };

        //let display = unsafe { XOpenDisplay(disp_name_ptr as *const i8) };
        // https://github.com/pftbest/x11-rust-example/blob/master/src/lib.rs
        let display = unsafe { XOpenDisplay(ptr::null_mut()) };
        if display.is_null() {
            panic!("failed to open x server: {}", disp_name);
        }
        let display_cleanup = DeferXClose(display);

        let info = unsafe { XScreenSaverAllocInfo() };
        let info_cleanup = DeferXFree(info as *mut c_void);

        Ok(Activity {
            id: id,
            update_interval: block_config.interval,
            text: TextWidget::new(config),
            idle_threshold: block_config.idle_threshold,
            reset_time: block_config.reset_time,
            start_time: Instant::now(),
            display: display,
            info: info,
            _defer_free_display: display_cleanup,
            _defer_free_info: info_cleanup,
            idle_start_time: Instant::now(),
            idle_last_reading: 0
        })
    }
}

impl Block for Activity {
    fn update(&mut self) -> Result<Option<Update>> {

        let mut idle = get_idle(self.display, self.info).unwrap();

        // the XScreenSaver details for some reason stops increasing when
        // i3lock starts. This only seems to happen when running in i3bar
        // and not when running in the terminal. The following code is to
        // attempt to detect this happening, and to keep the idle counter
        // increasing even if XScreenSaver isn't giving an updated number
        if idle == self.idle_last_reading {
            if self.idle_start_time.elapsed().as_secs() >= self.idle_threshold.as_secs() {
                idle += self.idle_start_time.elapsed().as_secs() * 1000;
            }
        } else {
            self.idle_start_time = Instant::now();
            self.idle_last_reading = idle;
        }

        idle /= 1000;

        let (elapsed, state) = if idle >= self.reset_time.as_secs() {
            self.start_time = Instant::now();
            (0, State::Info)
        } else if idle >= self.idle_threshold.as_secs() {
            let elapsed = self.reset_time.as_secs() - idle;
            let state = if elapsed > 0 {
                State::Warning
            } else {
                State::Info
            };
            (elapsed, state)
        } else {
            let elapsed = self.start_time.elapsed().as_secs();
            (elapsed, match elapsed {
                0..=1800 => State::Info, // 30 minute warning
                1801..=3000 => State::Warning,
                _ => State::Critical,
            })
        };

        let mut seconds = elapsed;
        let mut minutes = (seconds - (seconds % 60)) / 60;
        if seconds >= 60 {
            seconds %= 60;
        }
        let hours = (minutes - (minutes % 60)) / 60;
        if minutes >= 60 {
            minutes %= 60;
        }

        self.text.set_text(format!("{:02}h{:02}m{:02}", hours, minutes, seconds));
        self.text.set_state(state);

        Ok(Some(Update::Every(self.update_interval)))
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }

    fn click(&mut self, _: &I3BarEvent) -> Result<()> {
        self.start_time = Instant::now();
        Ok(())
    }

    fn id(&self) -> &str {
        &self.id
    }
}
