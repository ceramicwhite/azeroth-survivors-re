//! Controllers: every pad the OS (or the browser's Gamepad API) reports, merged into one [`Pad`].
//!
//! Bevy's `GilrsPlugin` is already in `DefaultPlugins`, on native (IOKit on macOS, XInput/WGI on
//! Windows, so a Bluetooth pad pairs like any other) and on wasm (`navigator.getGamepads()`, which
//! lists a pad only after its first button press). It spawns a `Gamepad` entity per pad; this
//! module turns those into the survivors' actions and drives a focus cursor through the menus.
//!
//! The focus cursor sets `Interaction` on the focused button after `ui_focus_system` and before
//! `glue_clicks`, so the pick screen, the level-up cards and the pause and game-over buttons
//! take a pad with no click logic of their own, and the glue hover highlight shows the focus.
//! A pick-screen button fires when confirm is released, as a mouse click does; a card or a pause
//! or game-over button acts as it goes down, as theirs do under the mouse. The press that wakes
//! the focus after the mouse or keyboard only lands it; press again to confirm.
//!
//! | Pad | Action |
//! |---|---|
//! | left stick / D-pad | move; menu focus |
//! | right stick | aim (locked camera) or swing and tilt (unlocked) |
//! | A (south) | jump; confirm a focused button or card |
//! | X (west) / right trigger | the class's movement spell |
//! | Y (north) | unlock / lock the camera |
//! | LB / RB | zoom out / in |
//! | Start | pause and resume; start the run; play again |
//! | B (east) | resume from pause |
//! | Back / Select | the picture polish |

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::input::{gamepad::Gamepad, gamepad::GamepadButton, InputSystems};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform, UiSystems};

use super::cards::Card;
use super::hud::UiAction;
use super::menu::MenuAction;
use super::Phase;

/// Stick travel read as rest. A calibration knob: worn and Bluetooth sticks rest off-centre.
const DEADZONE: f32 = 0.22;
/// How far a stick must tilt to step the menu focus.
const NAV_TILT: f32 = 0.6;
/// Seconds before a held direction repeats, then between repeats.
const NAV_FIRST: f32 = 0.4;
/// How far a stick must travel between frames to count as touched.
const STICK_MOVED: f32 = 0.05;
const NAV_REPEAT: f32 = 0.12;
/// How far ahead of the hero the right stick aims, yards: the reach of the longest movement spell
/// (`Charge`, 25) would pull a charge toward the far edge, so this is `Blink`'s 18.
const AIM_REACH: f32 = 18.0;

/// One frame of controller input, rebuilt in `PreUpdate`. Edges (`jump`…) are true for the one
/// frame the button went down, like `ButtonInput::just_pressed`.
#[derive(Resource, Default)]
pub(super) struct Pad {
    /// The last thing touched was a pad, not the keyboard or mouse: the menu focus shows and the
    /// right stick aims. Movement and the buttons work whichever it is.
    pub(super) active: bool,
    /// Left stick plus D-pad, past the dead zone, at most 1 long; +y is up the screen.
    pub(super) mv: Vec2,
    /// Right stick, past the dead zone.
    pub(super) look: Vec2,
    /// Zoom rate: +1 out (LB), −1 in (RB).
    pub(super) zoom: f32,
    pub(super) jump: bool,
    pub(super) mobility: bool,
    pub(super) camera: bool,
    pub(super) grade: bool,
    pub(super) start: bool,
    pub(super) back: bool,
    /// The confirm button is held.
    confirm: bool,
    /// A menu step this frame (screen axes, +y down), repeating while held.
    nav: IVec2,
    held: IVec2,
    wait: f32,
    /// Both sticks last frame, to tell a stick that moved from one resting off-centre.
    sticks: [Vec2; 2],
}

impl Pad {
    /// A held direction steps at once, then again after [`NAV_FIRST`], then every [`NAV_REPEAT`].
    fn repeat(&mut self, dir: IVec2, dt: f32) -> IVec2 {
        if dir == IVec2::ZERO {
            self.held = IVec2::ZERO;
            return IVec2::ZERO;
        }
        if dir != self.held {
            self.held = dir;
            self.wait = NAV_FIRST;
            return dir;
        }
        self.wait -= dt;
        if self.wait <= 0.0 {
            self.wait = NAV_REPEAT;
            return dir;
        }
        IVec2::ZERO
    }
}

/// The stick's travel past the dead zone, rescaled so it starts at 0 and tops out at 1.
fn deadzone(v: Vec2) -> Vec2 {
    let len = v.length();
    if len <= DEADZONE {
        return Vec2::ZERO;
    }
    v / len * ((len - DEADZONE) / (1.0 - DEADZONE)).min(1.0)
}

/// The one screen axis a tilt points along, if it is past [`NAV_TILT`].
fn tilt_dir(v: Vec2) -> IVec2 {
    if v.x.abs() >= v.y.abs() {
        if v.x.abs() > NAV_TILT {
            return IVec2::new(v.x.signum() as i32, 0);
        }
    } else if v.y.abs() > NAV_TILT {
        return IVec2::new(0, -v.y.signum() as i32);
    }
    IVec2::ZERO
}

/// A stick's world direction under the camera's yaw, as `drive_hero` turns WASD: up is −Z.
pub(super) fn world_dir(stick: Vec2, yaw: f32) -> Vec3 {
    let (sy, cy) = yaw.sin_cos();
    let (x, z) = (stick.x, -stick.y);
    Vec3::new(x * cy + z * sy, 0.0, z * cy - x * sy)
}

/// Where the right stick aims from the hero: the cursor's stand-in for `AimPoint`.
pub(super) fn aim_point(pad: &Pad, yaw: f32, hero: Vec3) -> Option<Vec3> {
    (pad.look != Vec2::ZERO).then(|| hero + world_dir(pad.look, yaw).normalize() * AIM_REACH)
}

/// Fold every connected pad into [`Pad`], and decide whether a pad or the keyboard and mouse
/// were touched last.
pub(super) fn read_pads(
    time: Res<Time<Real>>,
    pads: Query<&Gamepad>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut pad: ResMut<Pad>,
) {
    use GamepadButton::*;
    let mut now = Pad {
        active: pad.active,
        held: pad.held,
        wait: pad.wait,
        sticks: pad.sticks,
        ..default()
    };
    let (mut tilt, mut touched) = (Vec2::ZERO, false);
    let mut sticks = [Vec2::ZERO; 2];
    for g in &pads {
        let (left, right, dpad) = (g.left_stick(), g.right_stick(), g.dpad());
        now.mv += deadzone(left) + dpad;
        now.look += deadzone(right);
        tilt += left + dpad;
        now.jump |= g.just_pressed(South);
        now.mobility |= g.just_pressed(West) || g.just_pressed(RightTrigger2);
        now.camera |= g.just_pressed(North);
        now.grade |= g.just_pressed(Select);
        now.start |= g.just_pressed(Start);
        now.back |= g.just_pressed(East);
        now.confirm |= g.pressed(South);
        now.zoom += g.pressed(LeftTrigger) as i32 as f32 - g.pressed(RightTrigger) as i32 as f32;
        sticks[0] += left;
        sticks[1] += right;
        touched |= g.get_just_pressed().next().is_some();
    }
    // A stick resting past the dead zone (drift, a wheel, a Joy-Con) is not a touch: it must move.
    touched |= sticks
        .iter()
        .zip(now.sticks)
        .any(|(s, last)| deadzone(*s) != Vec2::ZERO && (*s - last).length() > STICK_MOVED);
    now.sticks = sticks;
    now.mv = now.mv.clamp_length_max(1.0);
    now.look = now.look.clamp_length_max(1.0);
    now.zoom = now.zoom.clamp(-1.0, 1.0);
    let typed = keys.get_just_pressed().next().is_some()
        || mouse.get_just_pressed().next().is_some()
        || motion.delta != Vec2::ZERO;
    now.active = touched || (now.active && !typed);
    now.nav = now.repeat(tilt_dir(tilt), time.delta_secs());
    *pad = now;
}

/// Which button the pad has focused on the current screen, and whether confirm may fire it.
#[derive(Resource, Default)]
pub(super) struct PadFocus {
    on: Option<Entity>,
    phase: Option<Phase>,
    /// Confirm has been up since the focus landed: a button still held from the last screen
    /// (the A that picked a card, the A that pressed Play again) must not fire the next one.
    armed: bool,
}

/// The nearest button in a direction: along it first, with the sideways drift counting double.
fn step(from: Vec2, dir: IVec2, others: &[(Entity, Vec2)]) -> Option<Entity> {
    let d = dir.as_vec2();
    others
        .iter()
        .filter_map(|&(e, p)| {
            let v = p - from;
            let along = v.dot(d);
            (along > 1.0).then(|| (e, along + 2.0 * v.perp_dot(d).abs()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|c| c.0)
}

/// A laid-out, visible button's centre.
fn live(
    e: Entity,
    at: &UiGlobalTransform,
    node: &ComputedNode,
    vis: &InheritedVisibility,
) -> Option<(Entity, Vec2)> {
    (vis.get() && node.size().cmpgt(Vec2::ZERO).all()).then(|| (e, at.translation))
}

/// Focus a button with the D-pad or left stick and hold it `Hovered`, or `Pressed` while confirm
/// is down. Only while a pad was touched last, so the mouse takes the screen back the moment it
/// moves.
#[allow(clippy::type_complexity)]
pub(super) fn drive_focus(
    pad: Res<Pad>,
    phase: Res<State<Phase>>,
    mut focus: ResMut<PadFocus>,
    menu: Query<(
        Entity,
        &UiGlobalTransform,
        &ComputedNode,
        &InheritedVisibility,
        &MenuAction,
    )>,
    cards: Query<
        (
            Entity,
            &UiGlobalTransform,
            &ComputedNode,
            &InheritedVisibility,
        ),
        With<Card>,
    >,
    actions: Query<
        (
            Entity,
            &UiGlobalTransform,
            &ComputedNode,
            &InheritedVisibility,
        ),
        With<UiAction>,
    >,
    mut interactions: Query<&mut Interaction>,
) {
    let phase = *phase.get();
    if focus.phase != Some(phase) {
        *focus = PadFocus {
            phase: Some(phase),
            ..default()
        };
    }
    let mut enter = None;
    let cands: Vec<(Entity, Vec2)> = match phase {
        Phase::Menu => menu
            .iter()
            .filter_map(|(e, at, n, v, a)| {
                if *a == MenuAction::Enter {
                    enter = Some(e);
                }
                live(e, at, n, v)
            })
            .collect(),
        Phase::LevelUp => cards
            .iter()
            .filter_map(|(e, a, n, v)| live(e, a, n, v))
            .collect(),
        Phase::Paused | Phase::GameOver => actions
            .iter()
            .filter_map(|(e, a, n, v)| live(e, a, n, v))
            .collect(),
        Phase::Playing => Vec::new(),
    };
    if !pad.active || cands.is_empty() {
        // Hand the button back: a `Pressed` set here would outlast the pad (the mouse keeps it).
        if let Some(Ok(mut i)) = focus.on.map(|e| interactions.get_mut(e)) {
            i.set_if_neq(Interaction::None);
        }
        *focus = PadFocus {
            phase: Some(phase),
            ..default()
        };
        return;
    }
    let from = focus
        .on
        .and_then(|f| cands.iter().find(|c| c.0 == f).map(|c| c.1));
    let at = match from {
        Some(at) => at,
        None => {
            // Play on the pick screen; the leftmost card; the top button.
            let key = |p: Vec2| if phase == Phase::LevelUp { p.x } else { p.y };
            let first = cands
                .iter()
                .find(|c| Some(c.0) == enter)
                .or_else(|| cands.iter().min_by(|a, b| key(a.1).total_cmp(&key(b.1))))
                .copied();
            let Some((e, at)) = first else { return };
            focus.on = Some(e);
            focus.armed = false;
            at
        }
    };
    if pad.nav != IVec2::ZERO {
        if let Some(to) = step(at, pad.nav, &cands) {
            focus.on = Some(to);
            // A moved focus needs a fresh press: A held through a nudge must not fire the new one.
            focus.armed = false;
        }
    }
    focus.armed |= !pad.confirm;
    let held = if pad.confirm && focus.armed {
        Interaction::Pressed
    } else {
        Interaction::Hovered
    };
    for &(e, _) in &cands {
        if let Ok(mut i) = interactions.get_mut(e) {
            i.set_if_neq(if Some(e) == focus.on {
                held
            } else {
                Interaction::None
            });
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Pad>()
        .init_resource::<PadFocus>()
        .add_systems(
            PreUpdate,
            (read_pads.after(InputSystems), drive_focus)
                .chain()
                .after(UiSystems::Focus)
                .before(crate::glue::glue_clicks),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::gamepad::{
        GamepadAxis, GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
        RawGamepadButtonChangedEvent, RawGamepadEvent,
    };
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::{ButtonState, InputPlugin};

    #[test]
    fn the_dead_zone_ignores_rest_and_rescales_the_rest() {
        assert_eq!(deadzone(Vec2::new(0.1, -0.15)), Vec2::ZERO);
        let full = deadzone(Vec2::new(0.0, 1.0));
        assert!((full - Vec2::Y).length() < 1e-5);
        let corner = deadzone(Vec2::new(1.0, 1.0));
        assert!(
            (corner.length() - 1.0).abs() < 1e-5,
            "a square gate is clamped round"
        );
        let mid = deadzone(Vec2::new(0.61, 0.0));
        assert!((mid.x - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_held_direction_steps_at_once_then_repeats() {
        let mut pad = Pad::default();
        let right = IVec2::X;
        assert_eq!(pad.repeat(right, 0.016), right);
        assert_eq!(
            pad.repeat(right, 0.3),
            IVec2::ZERO,
            "inside the first delay"
        );
        assert_eq!(pad.repeat(right, 0.2), right, "past it");
        assert_eq!(pad.repeat(right, 0.05), IVec2::ZERO);
        assert_eq!(pad.repeat(right, 0.08), right, "every repeat");
        assert_eq!(pad.repeat(IVec2::ZERO, 0.016), IVec2::ZERO);
        assert_eq!(
            pad.repeat(right, 0.016),
            right,
            "a fresh tilt steps at once"
        );
    }

    #[test]
    fn a_tilt_points_along_one_axis_only() {
        assert_eq!(tilt_dir(Vec2::new(0.9, 0.5)), IVec2::X);
        assert_eq!(
            tilt_dir(Vec2::new(0.3, 0.9)),
            IVec2::new(0, -1),
            "up the screen is -y"
        );
        assert_eq!(
            tilt_dir(Vec2::new(-0.5, 0.4)),
            IVec2::ZERO,
            "under the tilt"
        );
    }

    #[test]
    fn focus_steps_to_the_nearest_button_in_a_direction() {
        let mut world = World::new();
        let e: Vec<Entity> = (0..6).map(|_| world.spawn_empty().id()).collect();
        // Two columns of three, as the race buttons lay out.
        let at = |c: f32, r: f32| Vec2::new(100.0 + c * 60.0, 100.0 + r * 60.0);
        let cands: Vec<_> = (0..6)
            .map(|i| (e[i], at((i / 3) as f32, (i % 3) as f32)))
            .collect();
        assert_eq!(step(cands[0].1, IVec2::Y, &cands), Some(e[1]));
        assert_eq!(
            step(cands[1].1, IVec2::X, &cands),
            Some(e[4]),
            "across, same row"
        );
        assert_eq!(step(cands[4].1, IVec2::new(0, -1), &cands), Some(e[3]));
        assert_eq!(step(cands[2].1, IVec2::Y, &cands), None, "no wrap");
        assert_eq!(step(cands[0].1, IVec2::new(-1, 0), &cands), None);
    }

    #[test]
    fn the_right_stick_aims_where_the_camera_looks() {
        let pad = Pad {
            look: Vec2::new(0.0, 1.0),
            ..default()
        };
        let at = aim_point(&pad, 0.0, Vec3::new(5.0, 2.0, 5.0)).unwrap();
        assert!(
            (at - Vec3::new(5.0, 2.0, 5.0 - AIM_REACH)).length() < 1e-4,
            "up is -Z"
        );
        let turned = aim_point(&pad, std::f32::consts::FRAC_PI_2, Vec3::ZERO).unwrap();
        assert!((turned - Vec3::new(-AIM_REACH, 0.0, 0.0)).length() < 1e-4);
        assert_eq!(aim_point(&Pad::default(), 0.0, Vec3::ZERO), None);
    }

    fn app_with_pad() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin))
            .init_resource::<Pad>()
            .init_resource::<AccumulatedMouseMotion>()
            .add_systems(PreUpdate, read_pads.after(InputSystems));
        let id = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(GamepadConnectionEvent::new(
            id,
            GamepadConnection::Connected {
                name: "test".into(),
                vendor_id: None,
                product_id: None,
            },
        ));
        app.update();
        (app, id)
    }

    #[test]
    fn a_connected_pad_moves_jumps_and_takes_over_from_the_keyboard() {
        let (mut app, id) = app_with_pad();
        assert!(
            !app.world().resource::<Pad>().active,
            "connecting is not touching"
        );

        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                id,
                GamepadAxis::LeftStickX,
                1.0,
            )));
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                id,
                GamepadButton::South,
                1.0,
            )));
        app.update();
        let pad = app.world().resource::<Pad>();
        assert!(pad.active);
        assert!(
            pad.mv.x > 0.99 && pad.mv.y == 0.0,
            "stick right: {:?}",
            pad.mv
        );
        assert!(pad.jump && pad.confirm);

        app.update();
        let pad = app.world().resource::<Pad>();
        assert!(!pad.jump, "an edge lasts one frame");
        assert!(pad.mv.x > 0.99, "the stick is still held");

        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: Key::Character("w".into()),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                id,
                GamepadAxis::LeftStickX,
                0.0,
            )));
        app.update();
        let pad = app.world().resource::<Pad>();
        assert_eq!(pad.mv, Vec2::ZERO);
        assert!(!pad.active, "a key after the pad hands the screen back");
    }

    #[test]
    fn a_resting_stick_inside_the_dead_zone_is_not_input() {
        let (mut app, id) = app_with_pad();
        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                id,
                GamepadAxis::LeftStickX,
                0.12,
            )));
        app.update();
        let pad = app.world().resource::<Pad>();
        assert_eq!(pad.mv, Vec2::ZERO);
        assert!(!pad.active);
    }

    #[test]
    fn a_stick_resting_past_the_dead_zone_does_not_hold_the_screen() {
        let (mut app, id) = app_with_pad();
        let stick =
            |app: &mut App, x: f32| {
                app.world_mut().write_message(RawGamepadEvent::Axis(
                    RawGamepadAxisChangedEvent::new(id, GamepadAxis::LeftStickX, x),
                ));
                app.update();
            };
        stick(&mut app, 0.5);
        assert!(app.world().resource::<Pad>().active, "moving it is a touch");
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: Key::Character("w".into()),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(!app.world().resource::<Pad>().active);
        app.update();
        assert!(
            !app.world().resource::<Pad>().active,
            "the same tilt, held, stays the keyboard's screen"
        );
    }

    /// Two buttons side by side on the pick screen; `Pad` is driven by hand.
    fn focus_app() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<Phase>()
            .init_resource::<Pad>()
            .init_resource::<PadFocus>()
            .add_systems(Update, drive_focus);
        let mut button = |action, x| {
            app.world_mut()
                .spawn((
                    action,
                    Interaction::default(),
                    UiGlobalTransform::from_translation(Vec2::new(x, 100.0)),
                    ComputedNode {
                        size: Vec2::splat(40.0),
                        ..default()
                    },
                    InheritedVisibility::VISIBLE,
                ))
                .id()
        };
        let (a, b) = (
            button(MenuAction::Enter, 100.0),
            button(MenuAction::Randomize, 200.0),
        );
        app.update(); // the state machine settles on Menu
        (app, a, b)
    }

    fn interaction(app: &App, e: Entity) -> Interaction {
        *app.world().get::<Interaction>(e).unwrap()
    }

    #[test]
    fn the_focus_lands_on_play_steps_across_and_needs_a_fresh_press() {
        let (mut app, enter, other) = focus_app();
        app.update();
        assert_eq!(
            interaction(&app, enter),
            Interaction::None,
            "no pad touched yet"
        );

        // A still held from the previous screen when the focus lands: no press.
        {
            let mut pad = app.world_mut().resource_mut::<Pad>();
            pad.active = true;
            pad.confirm = true;
        }
        app.update();
        assert_eq!(
            interaction(&app, enter),
            Interaction::Hovered,
            "lands on Enter World, and the held A does not press it"
        );

        // Released, then pressed: it goes down.
        app.world_mut().resource_mut::<Pad>().confirm = false;
        app.update();
        app.world_mut().resource_mut::<Pad>().confirm = true;
        app.update();
        assert_eq!(interaction(&app, enter), Interaction::Pressed);

        // A nudge to the right with A held moves the focus but does not press the new button.
        app.world_mut().resource_mut::<Pad>().nav = IVec2::X;
        app.update();
        app.world_mut().resource_mut::<Pad>().nav = IVec2::ZERO;
        app.update();
        assert_eq!(interaction(&app, enter), Interaction::None);
        assert_eq!(interaction(&app, other), Interaction::Hovered);

        // Released and pressed again, it goes down; the mouse taking over clears it.
        app.world_mut().resource_mut::<Pad>().confirm = false;
        app.update();
        app.world_mut().resource_mut::<Pad>().confirm = true;
        app.update();
        assert_eq!(interaction(&app, other), Interaction::Pressed);
        app.world_mut().resource_mut::<Pad>().active = false;
        app.update();
        assert_eq!(interaction(&app, other), Interaction::None);
    }
}
