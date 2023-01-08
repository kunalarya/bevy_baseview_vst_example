#![allow(non_snake_case)]
mod editor_app;
mod log_helpers;

#[macro_use]
extern crate vst;

use std::sync::Arc;
use std::sync::RwLock;

use bevy_baseview_plugin::{AppProxy, ParentWin};
use vst::editor::Editor;
use vst::host::Host;
use vst::prelude::*;

const WINDOW_WIDTH: f64 = 500.0;
const WINDOW_HEIGHT: f64 = 300.0;

struct BaseviewDemo {
    host: HostCallback,
    params: Arc<BaseviewDemoParameters>,
}

struct BaseviewDemoParameters {
    gain: AtomicFloat,
    host_to_gui_tx: Arc<RwLock<Option<editor_app::HostToGuiTx>>>,
    gui_to_host_rx: Arc<RwLock<Option<editor_app::GuiToHostRx>>>,
}

impl PluginParameters for BaseviewDemoParameters {
    fn get_parameter(&self, index: i32) -> f32 {
        match index {
            0 => self.gain.get(),
            _ => 0.0,
        }
    }

    fn set_parameter(&self, index: i32, value: f32) {
        log::info!("set_parameter: {index} {value:.5}");
        if index == 0 {
            self.gain.set(value)
        }
        let host_to_gui_tx = match self.host_to_gui_tx.read() {
            Ok(host_to_gui_tx_guard) => host_to_gui_tx_guard,
            Err(err) => {
                log::error!("Unable to read host_to_gui_tx queue: {err:?}");
                return;
            }
        };
        if let Some(host_to_gui_tx) = &*host_to_gui_tx {
            // TODO(PANIC): replace panic with more intelligent error handling
            host_to_gui_tx
                .send(editor_app::HostToGui::ParamUpdate(
                    editor_app::ParamUpdate::GainUpdated(value as f64),
                ))
                .expect("send to gui");
        }
    }

    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "gain".to_string(),
            _ => "".to_string(),
        }
    }

    fn get_parameter_label(&self, index: i32) -> String {
        match index {
            0 => "%".to_string(),
            _ => "".to_string(),
        }
    }

    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            0 => {
                let gain_db = 20.0 * self.gain.get().log10();
                format!("{:.1} dB", gain_db)
            }
            _ => String::new(),
        }
    }
}

impl BaseviewDemo {
    fn process_gui_msgs(&self) {
        // TODO(PANIC): replace panic with more intelligent error handling
        // Consolidate updates
        let mut updated_gain = None;
        if let Some(gui_to_host_rx) = &*self.params.gui_to_host_rx.read().unwrap() {
            for msg in gui_to_host_rx.try_iter() {
                //log::info!("core got {msg:?}");
                match &msg {
                    editor_app::GuiToHost::ParamUpdate(param_update) => {
                        let editor_app::ParamUpdate::GainUpdated(value) = param_update;
                        updated_gain = Some(*value as f32)
                    }
                }
            }
        }
        if let Some(new_gain) = updated_gain {
            self.params.gain.set(new_gain);
            self.host.begin_edit(0);
            self.host.automate(0, new_gain);
            self.host.end_edit(0);
        }
    }
}

impl Plugin for BaseviewDemo {
    fn new(host: HostCallback) -> Self {
        BaseviewDemo {
            host,
            params: Arc::new(BaseviewDemoParameters {
                gain: AtomicFloat::new(1.0),
                host_to_gui_tx: Arc::new(RwLock::new(None)),
                gui_to_host_rx: Arc::new(RwLock::new(None)),
            }),
        }
    }

    fn init(&mut self) {
        log_helpers::setup_panic_handling();
        log_helpers::setup_tmp_log();
        log::info!("Started VST",);
    }

    fn get_info(&self) -> Info {
        Info {
            name: "Baseview Demo".to_string(),
            unique_id: 14357, // Used by hosts to differentiate between plugins.
            parameters: 1,
            ..Default::default()
        }
    }

    // Return handle to plugin editor if supported.
    fn get_editor(&mut self) -> Option<Box<dyn Editor>> {
        Some(Box::new(BaseviewDemoEditor::new(
            baseview::Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            Arc::clone(&self.params),
        )) as Box<dyn Editor>)
    }

    fn can_do(&self, can_do: CanDo) -> Supported {
        match can_do {
            CanDo::ReceiveMidiEvent
            | CanDo::ReceiveTimeInfo
            | CanDo::SendEvents
            | CanDo::ReceiveEvents => Supported::Yes,
            _ => Supported::Maybe,
        }
    }

    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        self.process_gui_msgs();
        // For each input and output
        let gain = self.params.gain.get();
        for (input, output) in buffer.zip() {
            // For each input sample and output sample in buffer
            for (in_frame, out_frame) in input.iter().zip(output.iter_mut()) {
                *out_frame = *in_frame * gain;
            }
        }
    }

    fn process_f64(&mut self, buffer: &mut AudioBuffer<f64>) {
        self.process_gui_msgs();
        // For each input and output
        let gain = self.params.gain.get() as f64;
        for (input, output) in buffer.zip() {
            // For each input sample and output sample in buffer
            for (in_frame, out_frame) in input.iter().zip(output.iter_mut()) {
                *out_frame = *in_frame * gain;
            }
        }
    }

    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        Arc::clone(&self.params) as Arc<dyn PluginParameters>
    }

    fn get_input_info(&self, input: i32) -> vst::channels::ChannelInfo {
        vst::channels::ChannelInfo::new(
            format!("Input channel {}", input),
            Some(format!("In {}", input)),
            true,
            None,
        )
    }

    fn get_output_info(&self, output: i32) -> vst::channels::ChannelInfo {
        vst::channels::ChannelInfo::new(
            format!("Output channel {}", output),
            Some(format!("Out {}", output)),
            true,
            None,
        )
    }
}

plugin_main!(BaseviewDemo);

struct BaseviewDemoEditor {
    params: Arc<BaseviewDemoParameters>,
    window_info: baseview::WindowInfo,
    //size: baseview::Size,
    open: bool,
    app: Option<AppProxy>,
}

impl BaseviewDemoEditor {
    fn new(size: baseview::Size, params: Arc<BaseviewDemoParameters>) -> Self {
        // TODO: Fix scale factor/DPI settings.
        let window_info = baseview::WindowInfo::from_logical_size(size, 1.0);
        Self {
            params,
            window_info,
            open: false,
            app: None,
        }
    }
}

impl Editor for BaseviewDemoEditor {
    fn size(&self) -> (i32, i32) {
        let phy_size = self.window_info.physical_size();
        (phy_size.width as i32, phy_size.height as i32)
    }

    fn position(&self) -> (i32, i32) {
        (0, 0)
    }

    fn open(&mut self, parent: *mut std::os::raw::c_void) -> bool {
        log::info!("vst-baseview: open");
        if self.open {
            return false;
        }
        let window_open_options = baseview::WindowOpenOptions {
            title: "Baseview Gain Demo".to_string(),
            size: self.window_info.logical_size(),
            scale: baseview::WindowScalePolicy::SystemScaleFactor,
        };
        let (host_to_gui_tx, gui_to_host_rx, app_proxy) =
            editor_app::create_app(&window_open_options, ParentWin::new(parent));
        // TODO: Clean up parameter pre-population.
        host_to_gui_tx
            .send(editor_app::HostToGui::ParamUpdate(
                editor_app::ParamUpdate::GainUpdated(self.params.gain.get() as f64),
            ))
            .expect("send to gui");

        if let Ok(mut host_to_gui_tx_ref) = self.params.host_to_gui_tx.write() {
            *host_to_gui_tx_ref = Some(host_to_gui_tx);
        }
        if let Ok(mut gui_to_host_rx_ref) = self.params.gui_to_host_rx.write() {
            *gui_to_host_rx_ref = Some(gui_to_host_rx);
        }

        self.open = true;
        self.app = Some(app_proxy);
        true
    }

    fn close(&mut self) {
        log::info!("vst-baseview: close");
        self.open = false;
        self.app = None; // Triggers App drop.

        if let Ok(mut host_to_gui_tx_ref) = self.params.host_to_gui_tx.write() {
            *host_to_gui_tx_ref = None;
        }
        if let Ok(mut gui_to_host_rx_ref) = self.params.gui_to_host_rx.write() {
            *gui_to_host_rx_ref = None;
        }
    }

    fn is_open(&mut self) -> bool {
        self.open
    }
}
