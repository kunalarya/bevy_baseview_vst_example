use bevy::prelude::*;
use bevy::render::texture::ImageSettings;
use bevy_baseview_plugin::{attach_to, AppProxy, DefaultBaseviewPlugins, ParentWin};
use bevy_embedded_assets::EmbeddedAssetPlugin;

pub type HostToGuiTx = crossbeam_channel::Sender<HostToGui>;
pub type HostToGuiRx = crossbeam_channel::Receiver<HostToGui>;
pub type GuiToHostTx = crossbeam_channel::Sender<GuiToHost>;
pub type GuiToHostRx = crossbeam_channel::Receiver<GuiToHost>;

#[derive(Copy, Clone, Debug)]
pub enum HostToGui {
    // Add any messages to send to the Bevy app here.
    ParamUpdate(ParamUpdate),
}

#[derive(Copy, Clone, Debug)]
pub enum GuiToHost {
    // Add any messages to send from the Bevy app here.
    ParamUpdate(ParamUpdate),
}

#[derive(Copy, Clone, Debug)]
pub enum ParamUpdate {
    GainUpdated(f64),
}

fn host_to_gui_relay(rx: Res<HostToGuiRx>, mut event_writer: EventWriter<HostToGui>) {
    for msg in rx.try_iter() {
        log::debug!("host_to_gui_relay: relaying {msg:?} to gui");
        event_writer.send(msg);
    }
}

fn gui_to_host_relay(tx: Res<GuiToHostTx>, mut event_reader: EventReader<GuiToHost>) {
    for msg in event_reader.iter() {
        if let Err(uhoh) = tx.send(*msg) {
            log::warn!("failed to send update message: {:?}", uhoh);
        }
    }
}

fn update_from_host(mut event_reader: EventReader<HostToGui>, mut gain_value: ResMut<GainValue>) {
    for msg in event_reader.iter() {
        match msg {
            HostToGui::ParamUpdate(ParamUpdate::GainUpdated(new_value)) => {
                gain_value.current = *new_value;
                gain_value.proposed = None;
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum AppState {
    Idle,
    AdjustingKnob,
}

#[derive(Debug, Default, Clone)]
struct DragState {
    start: Option<Vec2>,
}

#[derive(Debug, Default, Clone)]
struct CursorPosition(Vec2);

#[derive(Debug, Default, Clone)]
struct GainValue {
    current: f64,
    proposed: Option<f64>,
}

impl GainValue {
    fn new(value: f64) -> Self {
        Self {
            current: value,
            proposed: None,
        }
    }
}

pub fn create_app<P: Into<ParentWin>>(
    window_open_options: &baseview::WindowOpenOptions,
    parent: P,
) -> (
    crossbeam_channel::Sender<HostToGui>,
    crossbeam_channel::Receiver<GuiToHost>,
    AppProxy,
) {
    log::debug!("vst: create_app");
    let (gui_tx, gui_rx) = crossbeam_channel::bounded(128);
    let (host_tx, host_rx) = crossbeam_channel::bounded(128);

    let mut app = App::new();
    let proxy = attach_to(&mut app, window_open_options, parent);
    app.add_plugins_with(DefaultBaseviewPlugins, |group| {
        group.add_before::<bevy::asset::AssetPlugin, _>(EmbeddedAssetPlugin)
    });
    app.insert_resource(ImageSettings::default_nearest()) // prevents blurry sprites
        .insert_resource(DragState::default())
        .insert_resource(CursorPosition::default())
        .insert_resource(GainValue::new(0.0))
        .insert_resource(gui_rx)
        .add_event::<HostToGui>()
        .add_system(host_to_gui_relay)
        .insert_resource(host_tx)
        .add_event::<GuiToHost>()
        .add_system(gui_to_host_relay)
        .add_system(update_from_host)
        .add_system(cursor_position)
        .add_state(AppState::Idle)
        .add_startup_system(setup)
        .add_system_set(SystemSet::on_update(AppState::Idle).with_system(idle))
        .add_system_set(SystemSet::on_update(AppState::AdjustingKnob).with_system(knob_activated))
        .add_system(knob_render)
        .run();
    (gui_tx, host_rx, proxy)
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
) {
    let background_image = asset_server.load("background.png");
    let texture_handle = asset_server.load("knobman_2007.png");
    let texture_atlas = TextureAtlas::from_grid(texture_handle, Vec2::new(85.0, 85.0), 1, 100);
    let texture_atlas_handle = texture_atlases.add(texture_atlas);
    commands.spawn_bundle(Camera2dBundle::default());
    commands
        .spawn_bundle(SpriteBundle {
            texture: background_image,
            transform: Transform::from_scale(Vec3::splat(0.5)),
            ..default()
        })
        .with_children(|parent| {
            parent.spawn_bundle(SpriteSheetBundle {
                texture_atlas: texture_atlas_handle,
                transform: Transform::from_xyz(0.0, 0.0, 1.0),
                ..default()
            });
        });
}

fn idle(
    mut state: ResMut<State<AppState>>,
    mut drag_state: ResMut<DragState>,
    cursor_position: Res<CursorPosition>,
    mut buttons: ResMut<Input<MouseButton>>,
) {
    if buttons.just_pressed(MouseButton::Left) {
        buttons.clear_just_pressed(MouseButton::Left);
        if let Err(err) = state.set(AppState::AdjustingKnob) {
            log::error!("unable to set state to AdjustingKnob: {err:?}");
            return;
        }
        log::debug!("Setting state to AdjustingKnob");
        drag_state.start = Some(cursor_position.0);
    }
}

fn knob_activated(
    wnds: Res<Windows>,
    mut state: ResMut<State<AppState>>,
    mut drag_state: ResMut<DragState>,
    mut gain_value: ResMut<GainValue>,
    mut event_writer: EventWriter<GuiToHost>,
    cursor_position: Res<CursorPosition>,
    mut buttons: ResMut<Input<MouseButton>>,
) {
    if buttons.just_released(MouseButton::Left) {
        buttons.clear_just_released(MouseButton::Left);
        if let Err(err) = state.set(AppState::Idle) {
            log::error!("unable to set state to Idle: {err:?}");
            return;
        }
        log::debug!("Setting state to Idle");
        drag_state.start = None;
        if let Some(new_gain) = gain_value.proposed {
            gain_value.current = new_gain;
            event_writer.send(GuiToHost::ParamUpdate(ParamUpdate::GainUpdated(new_gain)));
        } else {
            // Restore the previous value.
            event_writer.send(GuiToHost::ParamUpdate(ParamUpdate::GainUpdated(
                gain_value.current,
            )));
        }
    } else {
        let wnd = match wnds.get_primary() {
            Some(wnd) => wnd,
            None => {
                log::error!("failed to get primary window");
                return;
            }
        };

        // compute delta
        let start = match drag_state.start {
            Some(start) => start,
            None => {
                log::error!("drag_state.start is None; expected starting coords");
                return;
            }
        };

        let delta = start - cursor_position.0;
        let pct = (delta.y / (wnd.height() / 1.5)) as f64;

        // TODO: Factor this out into a separate system
        let current_gain = gain_value.current;
        let new_gain = (current_gain + pct).clamp(0.0, 1.0);
        event_writer.send(GuiToHost::ParamUpdate(ParamUpdate::GainUpdated(new_gain)));
        gain_value.proposed = Some(new_gain);
    }
}

fn cursor_position(
    mut last_cursor_pos: ResMut<CursorPosition>,
    mut cursor_moved_events: EventReader<CursorMoved>,
) {
    for event in cursor_moved_events.iter() {
        last_cursor_pos.0 = event.position;
    }
}

fn knob_render(
    gain_value: ResMut<GainValue>,
    mut query: Query<(&mut TextureAtlasSprite, &Handle<TextureAtlas>)>,
    texture_atlases: ResMut<Assets<TextureAtlas>>,
) {
    for (mut sprite, texture_atlas_handle) in &mut query {
        let texture_atlas = texture_atlases.get(texture_atlas_handle).unwrap();
        let count = texture_atlas.textures.len();
        let gain = gain_value.proposed.unwrap_or(gain_value.current);
        sprite.index = ((count as f64 * gain) as usize).clamp(0, count - 1);
    }
}
