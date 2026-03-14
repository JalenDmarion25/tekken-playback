use std::sync::Arc;

use crate::Frame;
use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

pub struct X360Pad {
    // keep client alive
    _client: Arc<Client>,
    pad: Xbox360Wired<Arc<Client>>,
    state: XGamepad,
}

impl X360Pad {
    pub fn new() -> Result<Self, String> {
        let client = Arc::new(
            Client::connect().map_err(|e| format!("Failed to connect to ViGEmBus: {e:?}"))?,
        );

        // must be mutable because plugin() takes &mut self
        let mut pad = Xbox360Wired::new(client.clone(), TargetId::XBOX360_WIRED);

        pad.plugin()
            .map_err(|e| format!("Failed to plugin virtual pad: {e:?}"))?;

        // Give Windows/ViGEm a moment to finish bringing the target online
        std::thread::sleep(std::time::Duration::from_millis(150));

        Ok(Self {
            _client: client,
            pad,
            state: XGamepad::default(),
        })
    }

    pub fn set_frame(&mut self, f: Frame) -> Result<(), String> {
        self.state.buttons = XButtons { raw: f.buttons };
        self.state.left_trigger = f.lt;
        self.state.right_trigger = f.rt;
        self.state.thumb_lx = f.lx;
        self.state.thumb_ly = f.ly;
        self.state.thumb_rx = f.rx;
        self.state.thumb_ry = f.ry;

        self.pad
            .update(&self.state)
            .map_err(|e| format!("Update failed: {e:?}"))
    }

    pub fn reset(&mut self) -> Result<(), String> {
        self.state = XGamepad::default();
        self.pad
            .update(&self.state)
            .map_err(|e| format!("Reset failed: {e:?}"))
    }

    pub fn unplug(&mut self) -> Result<(), String> {
        self.pad
            .unplug()
            .map_err(|e| format!("Unplug failed: {e:?}"))
    }
}

impl Drop for X360Pad {
    fn drop(&mut self) {
        let _ = self.unplug();
    }
}
