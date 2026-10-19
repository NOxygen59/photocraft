#![allow(clippy::unnecessary_cast)]
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_foundation::{
    CGFloat, CGPoint, CGRect, MainThreadMarker, NSArray, NSNumber, NSObject, NSSet, NSString, NSTimer,
};
use objc2_ui_kit::{
    UICoordinateSpace, UIEvent, UIEventButtonMask, UIForceTouchCapability, UIGestureRecognizer,
    UIGestureRecognizerDelegate, UIGestureRecognizerState, UIHoverGestureRecognizer, UIKeyInput,
    UIKeyModifierFlags, UIPanGestureRecognizer, UIPinchGestureRecognizer, UIPointerInteraction,
    UIPointerInteractionDelegate, UIPointerRegion, UIPointerStyle, UIPress,
    UIPressesEvent, UIResponder, UIRotationGestureRecognizer, UIScrollTypeMask,
    UITapGestureRecognizer, UITextInputTraits, UITouch, UITouchPhase, UITouchType,
    UITraitEnvironment, UIView,
};

use super::app_state::{self, EventWrapper};
use super::window::WinitUIWindow;
use crate::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use super::keyboard;
use crate::event::{
    ElementState, Event, Force, KeyEvent, Modifiers, MouseButton, MouseScrollDelta, Touch,
    TouchPhase, WindowEvent,
};
use crate::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey, NativeKeyCode, PhysicalKey};
use crate::platform_impl::platform::DEVICE_ID;
use crate::platform_impl::KeyEventExtra;
use crate::window::{WindowAttributes, WindowId as RootWindowId};

pub struct WinitViewState {
    pinch_gesture_recognizer: RefCell<Option<Retained<UIPinchGestureRecognizer>>>,
    doubletap_gesture_recognizer: RefCell<Option<Retained<UITapGestureRecognizer>>>,
    rotation_gesture_recognizer: RefCell<Option<Retained<UIRotationGestureRecognizer>>>,
    pan_gesture_recognizer: RefCell<Option<Retained<UIPanGestureRecognizer>>>,

    // for iOS delta references the start of the Gesture
    rotation_last_delta: Cell<CGFloat>,
    pinch_last_delta: Cell<CGFloat>,
    pan_last_delta: Cell<CGPoint>,

    // Hardware keyboards and trackpads / mice (iPadOS 13.4+).
    modifiers: Cell<ModifiersState>,
    /// HID usages of the keys whose presses were sent (their releases are sent too).
    pressed_keys: RefCell<HashSet<isize>>,
    /// UIKit doesn't repeat held keys' presses: the press repeated, and its timer.
    repeat_key: RefCell<Option<KeyEvent>>,
    repeat_timer: RefCell<Option<Retained<NSTimer>>>,
    /// The held key repeats at the repeat rate (past the first delay).
    repeating: Cell<bool>,
    /// The button the pointer's touch pressed.
    pointer_button: Cell<Option<MouseButton>>,
    hover_gesture_recognizer: RefCell<Option<Retained<UIHoverGestureRecognizer>>>,
    scroll_gesture_recognizer: RefCell<Option<Retained<UIPanGestureRecognizer>>>,
    scroll_last: Cell<CGPoint>,
    trackpad_pinch_recognizer: RefCell<Option<Retained<UIPinchGestureRecognizer>>>,
    trackpad_pinch_last: Cell<CGFloat>,
    pointer_interaction: RefCell<Option<Retained<UIPointerInteraction>>>,
    cursor_hidden: Cell<bool>,
    /// Fingers' pinch and two-finger pan (UIKit recognizers, `install_finger_gestures`).
    finger_pinch_last: Cell<CGFloat>,
    finger_pan_last: Cell<CGPoint>,
    finger_gestures: RefCell<Vec<Retained<UIGestureRecognizer>>>,
    /// Fingers on the screen, held back a moment before they reach the app (`finger_touch`).
    fingers: RefCell<Vec<Finger>>,
    /// Taps held back: their lift, sent a moment after their press (`finishTaps`).
    tap_ends: RefCell<Vec<(u64, CGPoint)>>,
}

/// A finger on the screen.
struct Finger {
    id: u64,
    start: CGPoint,
    began_at: f64,
    /// Sent to the app as a touch.
    sent: bool,
    /// Landed with another finger: one of a pinch, a pan or a tap of several, never a touch.
    together: bool,
}

/// A finger held this long (seconds) or moved this far (points) is a touch; before, a second
/// finger, a recognized gesture or iPadOS's palm rejection takes it without a trace.
const FINGER_DELAY: f64 = 0.12;
const FINGER_SLOP: CGFloat = 10.0;
/// How long after a held-back tap's press its lift is sent (seconds): in the same frame, egui
/// would see a press with no time under the pointer, and no click.
const TAP_LIFT_DELAY: f64 = 0.05;

/// Before a held key repeats, and between repeats (seconds).
const KEY_REPEAT_DELAY: f64 = 0.42;
const KEY_REPEAT_INTERVAL: f64 = 0.035;

declare_class!(
    pub(crate) struct WinitView;

    unsafe impl ClassType for WinitView {
        #[inherits(UIResponder, NSObject)]
        type Super = UIView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "WinitUIView";
    }

    impl DeclaredClass for WinitView {
        type Ivars = WinitViewState;
    }

    unsafe impl WinitView {
        #[method(drawRect:)]
        fn draw_rect(&self, rect: CGRect) {
            let mtm = MainThreadMarker::new().unwrap();
            let window = self.window().unwrap();
            app_state::handle_nonuser_event(
                mtm,
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::RedrawRequested,
                }),
            );
            let _: () = unsafe { msg_send![super(self), drawRect: rect] };
        }

        #[method(layoutSubviews)]
        fn layout_subviews(&self) {
            let mtm = MainThreadMarker::new().unwrap();
            let _: () = unsafe { msg_send![super(self), layoutSubviews] };

            let window = self.window().unwrap();
            let window_bounds = window.bounds();

            // If the app is started in landscape, the view frame and window bounds can be mismatched.
            // The view frame will be in portrait and the window bounds in landscape. So apply the
            // window bounds to the view frame to make it consistent.
            let view_frame = self.frame();
            if view_frame != window_bounds {
                self.setFrame(window_bounds);
            }
            let size = self.physical_size();

            app_state::handle_nonuser_event(
                mtm,
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::Resized(size),
                }),
            );
        }

        #[method(setContentScaleFactor:)]
        fn set_content_scale_factor(&self, untrusted_scale_factor: CGFloat) {
            let mtm = MainThreadMarker::new().unwrap();
            let _: () =
                unsafe { msg_send![super(self), setContentScaleFactor: untrusted_scale_factor] };

            // `window` is null when `setContentScaleFactor` is invoked prior to `[UIWindow
            // makeKeyAndVisible]` at window creation time (either manually or internally by
            // UIKit when the `UIView` is first created), in which case we send no events here
            let window = match self.window() {
                Some(window) => window,
                None => return,
            };
            // `setContentScaleFactor` may be called with a value of 0, which means "reset the
            // content scale factor to a device-specific default value", so we can't use the
            // parameter here. We can query the actual factor using the getter
            let scale_factor = self.contentScaleFactor();
            assert!(
                !scale_factor.is_nan()
                    && scale_factor.is_finite()
                    && scale_factor.is_sign_positive()
                    && scale_factor > 0.0,
                "invalid scale_factor set on UIView",
            );
            let scale_factor = scale_factor as f64;
            let size = self.physical_size();
            let window_id = RootWindowId(window.id());
            app_state::handle_nonuser_events(
                mtm,
                std::iter::once(EventWrapper::ScaleFactorChanged(
                    app_state::ScaleFactorChanged {
                        window,
                        scale_factor,
                        suggested_size: size,
                    },
                ))
                .chain(std::iter::once(EventWrapper::StaticEvent(
                    Event::WindowEvent {
                        window_id,
                        event: WindowEvent::Resized(size),
                    },
                ))),
            );
        }

        #[method(touchesBegan:withEvent:)]
        fn touches_began(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.handle_touches(touches, event)
        }

        #[method(touchesMoved:withEvent:)]
        fn touches_moved(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.handle_touches(touches, event)
        }

        #[method(touchesEnded:withEvent:)]
        fn touches_ended(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.handle_touches(touches, event)
        }

        #[method(touchesCancelled:withEvent:)]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
            self.handle_touches(touches, event)
        }

        #[method(pinchGesture:)]
        fn pinch_gesture(&self, recognizer: &UIPinchGestureRecognizer) {
            let window = self.window().unwrap();

            let (phase, delta) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().pinch_last_delta.set(recognizer.scale());
                    (TouchPhase::Started, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last_scale: f64 = self.ivars().pinch_last_delta.replace(recognizer.scale());
                    (TouchPhase::Moved, recognizer.scale() - last_scale)
                }
                UIGestureRecognizerState::Ended => {
                    let last_scale: f64 = self.ivars().pinch_last_delta.replace(0.0);
                    (TouchPhase::Moved, recognizer.scale() - last_scale)
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    self.ivars().rotation_last_delta.set(0.0);
                    // Pass -delta so that action is reversed
                    (TouchPhase::Cancelled, -recognizer.scale())
                }
                state => panic!("unexpected recognizer state: {state:?}"),
            };

            let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::PinchGesture {
                    device_id: DEVICE_ID,
                    delta: delta as f64,
                    phase,
                },
            });

            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(mtm, gesture_event);
        }

        #[method(doubleTapGesture:)]
        fn double_tap_gesture(&self, recognizer: &UITapGestureRecognizer) {
            let window = self.window().unwrap();

            if recognizer.state() == UIGestureRecognizerState::Ended {
                let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id: RootWindowId(window.id()),
                    event: WindowEvent::DoubleTapGesture {
                        device_id: DEVICE_ID,
                    },
                });

                let mtm = MainThreadMarker::new().unwrap();
                app_state::handle_nonuser_event(mtm, gesture_event);
            }
        }

        #[method(rotationGesture:)]
        fn rotation_gesture(&self, recognizer: &UIRotationGestureRecognizer) {
            let window = self.window().unwrap();

            let (phase, delta) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().rotation_last_delta.set(0.0);

                    (TouchPhase::Started, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last_rotation = self.ivars().rotation_last_delta.replace(recognizer.rotation());

                    (TouchPhase::Moved, recognizer.rotation() - last_rotation)
                }
                UIGestureRecognizerState::Ended => {
                    let last_rotation = self.ivars().rotation_last_delta.replace(0.0);

                    (TouchPhase::Ended, recognizer.rotation() - last_rotation)
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    self.ivars().rotation_last_delta.set(0.0);

                    // Pass -delta so that action is reversed
                    (TouchPhase::Cancelled, -recognizer.rotation())
                }
                state => panic!("unexpected recognizer state: {state:?}"),
            };

            // Make delta negative to match macos, convert to degrees
            let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::RotationGesture {
                    device_id: DEVICE_ID,
                    delta: -delta.to_degrees() as _,
                    phase,
                },
            });

            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(mtm, gesture_event);
        }

        #[method(panGesture:)]
        fn pan_gesture(&self, recognizer: &UIPanGestureRecognizer) {
            let window = self.window().unwrap();

            let translation = recognizer.translationInView(Some(self));

            let (phase, dx, dy) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().pan_last_delta.set(translation);

                    (TouchPhase::Started, 0.0, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last_pan: CGPoint = self.ivars().pan_last_delta.replace(translation);

                    let dx = translation.x - last_pan.x;
                    let dy = translation.y - last_pan.y;

                    (TouchPhase::Moved, dx, dy)
                }
                UIGestureRecognizerState::Ended => {
                    let last_pan: CGPoint = self.ivars().pan_last_delta.replace(CGPoint{x:0.0, y:0.0});

                    let dx = translation.x - last_pan.x;
                    let dy = translation.y - last_pan.y;

                    (TouchPhase::Ended, dx, dy)
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    let last_pan: CGPoint = self.ivars().pan_last_delta.replace(CGPoint{x:0.0, y:0.0});

                    // Pass -delta so that action is reversed
                    (TouchPhase::Cancelled, -last_pan.x, -last_pan.y)
                }
                state => panic!("unexpected recognizer state: {state:?}"),
            };


            let gesture_event = EventWrapper::StaticEvent(Event::WindowEvent {
                window_id: RootWindowId(window.id()),
                event: WindowEvent::PanGesture {
                    device_id: DEVICE_ID,
                    delta: PhysicalPosition::new(dx as _, dy as _),
                    phase,
                },
            });

            let mtm = MainThreadMarker::new().unwrap();
            app_state::handle_nonuser_event(mtm, gesture_event);
        }

        #[method(canBecomeFirstResponder)]
        fn can_become_first_responder(&self) -> bool {
            true
        }

        #[method(pressesBegan:withEvent:)]
        fn presses_began(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.handle_presses(presses, event, ElementState::Pressed) {
                let _: () = unsafe { msg_send![super(self), pressesBegan: presses, withEvent: event] };
            }
        }

        #[method(pressesEnded:withEvent:)]
        fn presses_ended(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.handle_presses(presses, event, ElementState::Released) {
                let _: () = unsafe { msg_send![super(self), pressesEnded: presses, withEvent: event] };
            }
        }

        #[method(pressesCancelled:withEvent:)]
        fn presses_cancelled(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.handle_presses(presses, event, ElementState::Released) {
                let _: () = unsafe { msg_send![super(self), pressesCancelled: presses, withEvent: event] };
            }
        }

        #[method(winitKeyRepeat:)]
        fn key_repeat(&self, _timer: &NSTimer) {
            let Some(mut event) = self.ivars().repeat_key.borrow().clone() else {
                self.stop_key_repeat();
                return;
            };
            // The first repeat: from now on at the repeat rate.
            if !self.ivars().repeating.replace(true) {
                self.start_key_repeat_timer(KEY_REPEAT_INTERVAL, true);
            }
            event.repeat = true;
            self.send_key(event);
        }

        /// Two fingers pinching the screen zoom, as a trackpad's pinch does.
        #[method(fingerPinch:)]
        fn finger_pinch(&self, recognizer: &UIPinchGestureRecognizer) {
            let Some(window) = self.window() else { return };
            let window_id = RootWindowId(window.id());
            let scale = recognizer.scale();
            let (phase, delta) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().finger_pinch_last.set(scale);
                    (TouchPhase::Started, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last = self.ivars().finger_pinch_last.replace(scale);
                    (TouchPhase::Moved, if last > 0.0 && scale > 0.0 { (scale / last).ln() } else { 0.0 })
                }
                UIGestureRecognizerState::Ended => (TouchPhase::Ended, 0.0),
                _ => (TouchPhase::Cancelled, 0.0),
            };
            let position = self.physical(unsafe { recognizer.locationInView(Some(self)) });
            let events = vec![
                window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }),
                window_event(window_id, WindowEvent::PinchGesture { device_id: DEVICE_ID, delta: delta as f64, phase }),
            ];
            app_state::handle_nonuser_events(MainThreadMarker::from(self), events);
        }

        /// Two fingers dragging on the screen scroll, as a trackpad's two-finger scroll does.
        #[method(fingerPan:)]
        fn finger_pan(&self, recognizer: &UIPanGestureRecognizer) {
            let Some(window) = self.window() else { return };
            let window_id = RootWindowId(window.id());
            let translation = recognizer.translationInView(Some(self));
            let state = recognizer.state();
            let (phase, dx, dy) = if state == UIGestureRecognizerState::Began {
                self.ivars().finger_pan_last.set(translation);
                (TouchPhase::Started, 0.0, 0.0)
            } else if state == UIGestureRecognizerState::Changed || state == UIGestureRecognizerState::Ended {
                let last = self.ivars().finger_pan_last.replace(translation);
                let phase = if state == UIGestureRecognizerState::Ended { TouchPhase::Ended } else { TouchPhase::Moved };
                (phase, translation.x - last.x, translation.y - last.y)
            } else {
                (TouchPhase::Cancelled, 0.0, 0.0)
            };
            let position = self.physical(unsafe { recognizer.locationInView(Some(self)) });
            let scale = self.contentScaleFactor() as f64;
            let mut events = vec![
                window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }),
                window_event(window_id, WindowEvent::MouseWheel {
                    device_id: DEVICE_ID,
                    delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(dx as f64 * scale, dy as f64 * scale)),
                    phase,
                }),
            ];
            if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                events.push(window_event(window_id, WindowEvent::CursorLeft { device_id: DEVICE_ID }));
            }
            app_state::handle_nonuser_events(MainThreadMarker::from(self), events);
        }

        /// The lifts of taps held back (see `TAP_LIFT_DELAY`).
        #[method(finishTaps)]
        fn finish_taps(&self) {
            let Some(window) = self.window() else { return };
            let window_id = RootWindowId(window.id());
            let ends: Vec<(u64, CGPoint)> = self.ivars().tap_ends.borrow_mut().drain(..).collect();
            let events = ends
                .into_iter()
                .map(|(id, at)| {
                    window_event(window_id, WindowEvent::Touch(Touch {
                        device_id: DEVICE_ID,
                        id,
                        location: self.physical(at),
                        force: None,
                        phase: TouchPhase::Ended,
                    }))
                })
                .collect::<Vec<_>>();
            app_state::handle_nonuser_events(MainThreadMarker::from(self), events);
        }

        /// A tap of two fingers undoes.
        #[method(undoTap:)]
        fn undo_tap(&self, recognizer: &UITapGestureRecognizer) {
            self.tap_shortcut(recognizer, false);
        }

        /// A tap of three fingers redoes.
        #[method(redoTap:)]
        fn redo_tap(&self, recognizer: &UITapGestureRecognizer) {
            self.tap_shortcut(recognizer, true);
        }

        #[method(hoverGesture:)]
        fn hover_gesture(&self, recognizer: &UIHoverGestureRecognizer) {
            let Some(window) = self.window() else { return };
            let window_id = RootWindowId(window.id());
            let mut events = Vec::new();
            events.extend(self.sync_modifiers(window_id, unsafe { recognizer.modifierFlags() }));
            let position = self.physical(unsafe { recognizer.locationInView(Some(self)) });
            match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    events.push(window_event(window_id, WindowEvent::CursorEntered { device_id: DEVICE_ID }));
                    events.push(window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }));
                }
                UIGestureRecognizerState::Changed => {
                    events.push(window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }));
                }
                UIGestureRecognizerState::Ended | UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    // A press keeps the pointer: its touch takes over until it lifts.
                    if self.ivars().pointer_button.get().is_none() {
                        events.push(window_event(window_id, WindowEvent::CursorLeft { device_id: DEVICE_ID }));
                    }
                }
                _ => {}
            }
            app_state::handle_nonuser_events(MainThreadMarker::from(self), events);
        }

        #[method(scrollGesture:)]
        fn scroll_gesture(&self, recognizer: &UIPanGestureRecognizer) {
            let Some(window) = self.window() else { return };
            let window_id = RootWindowId(window.id());
            let translation = recognizer.translationInView(Some(self));
            let (phase, dx, dy) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().scroll_last.set(translation);
                    (TouchPhase::Started, 0.0, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last = self.ivars().scroll_last.replace(translation);
                    (TouchPhase::Moved, translation.x - last.x, translation.y - last.y)
                }
                UIGestureRecognizerState::Ended => {
                    let last = self.ivars().scroll_last.replace(CGPoint { x: 0.0, y: 0.0 });
                    (TouchPhase::Ended, translation.x - last.x, translation.y - last.y)
                }
                _ => {
                    self.ivars().scroll_last.set(CGPoint { x: 0.0, y: 0.0 });
                    (TouchPhase::Cancelled, 0.0, 0.0)
                }
            };
            let mut events = Vec::new();
            events.extend(self.sync_modifiers(window_id, unsafe { recognizer.modifierFlags() }));
            let position = self.physical(unsafe { recognizer.locationInView(Some(self)) });
            events.push(window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }));
            let scale = self.contentScaleFactor() as f64;
            events.push(window_event(window_id, WindowEvent::MouseWheel {
                device_id: DEVICE_ID,
                delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(dx as f64 * scale, dy as f64 * scale)),
                phase,
            }));
            app_state::handle_nonuser_events(MainThreadMarker::from(self), events);
        }

        #[method(trackpadPinchGesture:)]
        fn trackpad_pinch_gesture(&self, recognizer: &UIPinchGestureRecognizer) {
            // A pinch on the screen has touches (the app sees them as touches); a trackpad's has none.
            if unsafe { recognizer.numberOfTouches() } != 0 {
                return;
            }
            let Some(window) = self.window() else { return };
            let window_id = RootWindowId(window.id());
            let scale = recognizer.scale();
            let (phase, delta) = match recognizer.state() {
                UIGestureRecognizerState::Began => {
                    self.ivars().trackpad_pinch_last.set(scale);
                    (TouchPhase::Started, 0.0)
                }
                UIGestureRecognizerState::Changed => {
                    let last = self.ivars().trackpad_pinch_last.replace(scale);
                    (TouchPhase::Moved, if last > 0.0 && scale > 0.0 { (scale / last).ln() } else { 0.0 })
                }
                UIGestureRecognizerState::Ended => (TouchPhase::Ended, 0.0),
                _ => (TouchPhase::Cancelled, 0.0),
            };
            let mut events = Vec::new();
            events.extend(self.sync_modifiers(window_id, unsafe { recognizer.modifierFlags() }));
            let position = self.physical(unsafe { recognizer.locationInView(Some(self)) });
            events.push(window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }));
            events.push(window_event(window_id, WindowEvent::PinchGesture { device_id: DEVICE_ID, delta: delta as f64, phase }));
            app_state::handle_nonuser_events(MainThreadMarker::from(self), events);
        }
    }

    unsafe impl NSObjectProtocol for WinitView {}

    unsafe impl UIGestureRecognizerDelegate for WinitView {
        #[method(gestureRecognizer:shouldRecognizeSimultaneouslyWithGestureRecognizer:)]
        fn should_recognize_simultaneously(&self, _gesture_recognizer: &UIGestureRecognizer, _other_gesture_recognizer: &UIGestureRecognizer) -> bool {
            true
        }
    }

    unsafe impl UITextInputTraits for WinitView {
    }

    unsafe impl UIPointerInteractionDelegate for WinitView {
        /// The system pointer, or none while the app draws its own cursor
        /// (`Window::set_cursor_visible(false)`).
        #[method_id(pointerInteraction:styleForRegion:)]
        fn pointer_style(&self, _interaction: &UIPointerInteraction, _region: &UIPointerRegion) -> Option<Retained<UIPointerStyle>> {
            self.ivars()
                .cursor_hidden
                .get()
                .then(|| unsafe { UIPointerStyle::hiddenPointerStyle(MainThreadMarker::from(self)) })
        }
    }

    unsafe impl UIKeyInput for WinitView {
        #[method(hasText)]
        fn has_text(&self) -> bool {
            true
        }

        #[method(insertText:)]
        fn insert_text(&self, text: &NSString) {
            self.handle_insert_text(text)
        }

        #[method(deleteBackward)]
        fn delete_backward(&self) {
            self.handle_delete_backward()
        }
    }
);

fn window_event(window_id: RootWindowId, event: WindowEvent) -> EventWrapper {
    EventWrapper::StaticEvent(Event::WindowEvent { window_id, event })
}

impl WinitView {
    /// The view's size in physical pixels: what the app lays out and draws, in the coordinates of
    /// touches and the pointer (`physical`). Not the window's frame on the screen, which a floating
    /// window (Stage Manager, tweaks such as Aerial) moves, scales or insets with safe areas: sizing
    /// the drawing from it stretched the picture away from where touches land.
    pub(crate) fn physical_size(&self) -> PhysicalSize<u32> {
        let size = self.bounds().size;
        LogicalSize::new(size.width as f64, size.height as f64)
            .to_physical(self.contentScaleFactor() as f64)
    }

    fn physical(&self, logical: CGPoint) -> PhysicalPosition<f64> {
        let scale_factor = self.contentScaleFactor() as f64;
        PhysicalPosition::from_logical::<(f64, f64), f64>(
            (logical.x as _, logical.y as _),
            scale_factor,
        )
    }

    /// Trackpads and mice: hovering, scrolling (two fingers, a wheel), pinching, and the system
    /// pointer hidden while the app draws its own cursor.
    fn install_pointer_support(&self, mtm: MainThreadMarker) {
        let hover = unsafe {
            UIHoverGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(self),
                Some(sel!(hoverGesture:)),
            )
        };
        hover.setDelegate(Some(ProtocolObject::from_ref(self)));
        unsafe { hover.setCancelsTouchesInView(false) };
        self.addGestureRecognizer(&hover);
        self.ivars().hover_gesture_recognizer.replace(Some(hover));

        let scroll = unsafe {
            UIPanGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(self),
                Some(sel!(scrollGesture:)),
            )
        };
        scroll.setDelegate(Some(ProtocolObject::from_ref(self)));
        unsafe {
            scroll.setAllowedScrollTypesMask(UIScrollTypeMask::All);
            scroll.setCancelsTouchesInView(false);
        }
        // Scrolls only: no touch drives it.
        scroll.setMinimumNumberOfTouches(0);
        scroll.setMaximumNumberOfTouches(0);
        self.addGestureRecognizer(&scroll);
        self.ivars().scroll_gesture_recognizer.replace(Some(scroll));

        let pinch = unsafe {
            UIPinchGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(self),
                Some(sel!(trackpadPinchGesture:)),
            )
        };
        pinch.setDelegate(Some(ProtocolObject::from_ref(self)));
        unsafe {
            pinch.setCancelsTouchesInView(false);
            pinch.setDelaysTouchesEnded(false);
        }
        self.addGestureRecognizer(&pinch);
        self.ivars().trackpad_pinch_recognizer.replace(Some(pinch));

        let interaction = unsafe {
            UIPointerInteraction::initWithDelegate(
                mtm.alloc(),
                Some(ProtocolObject::from_ref(self)),
            )
        };
        unsafe { self.addInteraction(ProtocolObject::from_ref(&*interaction)) };
        self.ivars().pointer_interaction.replace(Some(interaction));
    }

    /// Hide the system pointer while the app draws its own cursor, or show it again.
    pub(crate) fn set_cursor_hidden(&self, hidden: bool) {
        if self.ivars().cursor_hidden.replace(hidden) != hidden {
            if let Some(interaction) = &*self.ivars().pointer_interaction.borrow() {
                unsafe { interaction.invalidate() };
            }
        }
    }

    fn pointer_touch(
        &self,
        events: &mut Vec<EventWrapper>,
        window_id: RootWindowId,
        touch: &UITouch,
        event: Option<&UIEvent>,
    ) {
        let position = self.physical(touch.locationInView(Some(self)));
        events.push(window_event(window_id, WindowEvent::CursorMoved { device_id: DEVICE_ID, position }));
        match touch.phase() {
            UITouchPhase::Began => {
                let mask = event.map(|e| unsafe { e.buttonMask() });
                let button = match mask {
                    Some(m) if m.contains(UIEventButtonMask::Secondary) => MouseButton::Right,
                    Some(m) if m.0 & 0b100 != 0 => MouseButton::Middle,
                    _ => MouseButton::Left,
                };
                if let Some(previous) = self.ivars().pointer_button.replace(Some(button)) {
                    events.push(window_event(window_id, WindowEvent::MouseInput {
                        device_id: DEVICE_ID,
                        state: ElementState::Released,
                        button: previous,
                    }));
                }
                events.push(window_event(window_id, WindowEvent::MouseInput {
                    device_id: DEVICE_ID,
                    state: ElementState::Pressed,
                    button,
                }));
            }
            UITouchPhase::Ended | UITouchPhase::Cancelled => {
                if let Some(button) = self.ivars().pointer_button.take() {
                    events.push(window_event(window_id, WindowEvent::MouseInput {
                        device_id: DEVICE_ID,
                        state: ElementState::Released,
                        button,
                    }));
                }
            }
            _ => {}
        }
    }

    /// A `ModifiersChanged` event when `flags` differ from the modifiers last sent.
    fn sync_modifiers(&self, window_id: RootWindowId, flags: UIKeyModifierFlags) -> Option<EventWrapper> {
        self.set_modifiers(window_id, keyboard::modifiers_from_flags(flags))
    }

    fn set_modifiers(&self, window_id: RootWindowId, state: ModifiersState) -> Option<EventWrapper> {
        (self.ivars().modifiers.replace(state) != state)
            .then(|| window_event(window_id, WindowEvent::ModifiersChanged(Modifiers::from(state))))
    }

    /// Hardware keyboard presses, from this view or (while it isn't the first responder) the view
    /// controller. Returns false when UIKit should have them: characters typed into a text field
    /// (this view the first responder), so that dead keys and input methods compose them and they
    /// arrive through `insertText:`.
    pub(crate) fn handle_presses(
        &self,
        presses: &NSSet<UIPress>,
        event: Option<&UIPressesEvent>,
        state: ElementState,
    ) -> bool {
        let mtm = MainThreadMarker::from(self);
        let Some(window) = self.window() else { return false };
        let window_id = RootWindowId(window.id());
        let editing_text = unsafe { self.isFirstResponder() };
        let mut events = Vec::new();
        let mut handled = false;
        for press in presses {
            let Some(key) = (unsafe { press.key(mtm) }) else { continue };
            let pressed = keyboard::pressed_key(&key);
            let usage = pressed.usage;
            let ours = if state == ElementState::Pressed {
                if editing_text && pressed.types_text {
                    false
                } else {
                    self.ivars().pressed_keys.borrow_mut().insert(usage);
                    true
                }
            } else {
                self.ivars().pressed_keys.borrow_mut().remove(&usage)
            };
            // The modifiers: as the event has them, and the modifier key itself as it goes.
            let mut modifiers = event
                .map(|e| keyboard::modifiers_from_flags(unsafe { e.modifierFlags() }))
                .unwrap_or_else(|| self.ivars().modifiers.get());
            if let Some(m) = keyboard::hid_modifier(usage) {
                modifiers.set(m, state == ElementState::Pressed);
            }
            events.extend(self.set_modifiers(window_id, modifiers));
            if !ours {
                continue;
            }
            handled = true;
            let key_event = KeyEvent {
                physical_key: pressed.physical_key,
                logical_key: pressed.logical_key,
                text: if state == ElementState::Pressed { pressed.text } else { None },
                location: pressed.location,
                state,
                repeat: false,
                platform_specific: KeyEventExtra {},
            };
            if keyboard::hid_modifier(usage).is_none() {
                if state == ElementState::Pressed {
                    self.ivars().repeat_key.replace(Some(key_event.clone()));
                    self.ivars().repeating.set(false);
                    self.start_key_repeat_timer(KEY_REPEAT_DELAY, false);
                } else if self.ivars().repeat_key.borrow().as_ref().is_some_and(|k| k.physical_key == key_event.physical_key) {
                    self.stop_key_repeat();
                }
            }
            events.push(window_event(window_id, WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                event: key_event,
                is_synthetic: false,
            }));
        }
        app_state::handle_nonuser_events(mtm, events);
        handled
    }

    fn send_key(&self, event: KeyEvent) {
        let Some(window) = self.window() else { return };
        app_state::handle_nonuser_event(
            MainThreadMarker::from(self),
            window_event(RootWindowId(window.id()), WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                event,
                is_synthetic: false,
            }),
        );
    }

    fn start_key_repeat_timer(&self, interval: f64, repeats: bool) {
        if let Some(old) = self.ivars().repeat_timer.take() {
            unsafe { old.invalidate() };
        }
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                interval,
                self,
                sel!(winitKeyRepeat:),
                None,
                repeats,
            )
        };
        self.ivars().repeat_timer.replace(Some(timer));
    }

    fn stop_key_repeat(&self) {
        self.ivars().repeat_key.replace(None);
        if let Some(timer) = self.ivars().repeat_timer.take() {
            unsafe { timer.invalidate() };
        }
    }

    pub(crate) fn new(
        mtm: MainThreadMarker,
        window_attributes: &WindowAttributes,
        frame: CGRect,
    ) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(WinitViewState {
            pinch_gesture_recognizer: RefCell::new(None),
            doubletap_gesture_recognizer: RefCell::new(None),
            rotation_gesture_recognizer: RefCell::new(None),
            pan_gesture_recognizer: RefCell::new(None),

            rotation_last_delta: Cell::new(0.0),
            pinch_last_delta: Cell::new(0.0),
            pan_last_delta: Cell::new(CGPoint { x: 0.0, y: 0.0 }),

            modifiers: Cell::new(ModifiersState::empty()),
            pressed_keys: RefCell::new(HashSet::new()),
            repeat_key: RefCell::new(None),
            repeat_timer: RefCell::new(None),
            repeating: Cell::new(false),
            pointer_button: Cell::new(None),
            hover_gesture_recognizer: RefCell::new(None),
            scroll_gesture_recognizer: RefCell::new(None),
            scroll_last: Cell::new(CGPoint { x: 0.0, y: 0.0 }),
            trackpad_pinch_recognizer: RefCell::new(None),
            trackpad_pinch_last: Cell::new(1.0),
            pointer_interaction: RefCell::new(None),
            cursor_hidden: Cell::new(false),
            finger_pinch_last: Cell::new(1.0),
            finger_pan_last: Cell::new(CGPoint { x: 0.0, y: 0.0 }),
            finger_gestures: RefCell::new(Vec::new()),
            fingers: RefCell::new(Vec::new()),
            tap_ends: RefCell::new(Vec::new()),
        });
        let this: Retained<Self> = unsafe { msg_send_id![super(this), initWithFrame: frame] };

        this.setMultipleTouchEnabled(true);
        this.install_pointer_support(mtm);
        this.install_finger_gestures(mtm);

        if let Some(scale_factor) = window_attributes.platform_specific.scale_factor {
            this.setContentScaleFactor(scale_factor as _);
        }

        this
    }

    fn window(&self) -> Option<Retained<WinitUIWindow>> {
        // SAFETY: `WinitView`s are always installed in a `WinitUIWindow`
        (**self).window().map(|window| unsafe { Retained::cast(window) })
    }

    pub(crate) fn recognize_pinch_gesture(&self, should_recognize: bool) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().pinch_gesture_recognizer.borrow().is_none() {
                let pinch = unsafe {
                    UIPinchGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(pinchGesture:)),
                    )
                };
                pinch.setDelegate(Some(ProtocolObject::from_ref(self)));
                self.addGestureRecognizer(&pinch);
                self.ivars().pinch_gesture_recognizer.replace(Some(pinch));
            }
        } else if let Some(recognizer) = self.ivars().pinch_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    pub(crate) fn recognize_pan_gesture(
        &self,
        should_recognize: bool,
        minimum_number_of_touches: u8,
        maximum_number_of_touches: u8,
    ) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().pan_gesture_recognizer.borrow().is_none() {
                let pan = unsafe {
                    UIPanGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(panGesture:)),
                    )
                };
                pan.setDelegate(Some(ProtocolObject::from_ref(self)));
                pan.setMinimumNumberOfTouches(minimum_number_of_touches as _);
                pan.setMaximumNumberOfTouches(maximum_number_of_touches as _);
                self.addGestureRecognizer(&pan);
                self.ivars().pan_gesture_recognizer.replace(Some(pan));
            }
        } else if let Some(recognizer) = self.ivars().pan_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    pub(crate) fn recognize_doubletap_gesture(&self, should_recognize: bool) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().doubletap_gesture_recognizer.borrow().is_none() {
                let tap = unsafe {
                    UITapGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(doubleTapGesture:)),
                    )
                };
                tap.setDelegate(Some(ProtocolObject::from_ref(self)));
                tap.setNumberOfTapsRequired(2);
                tap.setNumberOfTouchesRequired(1);
                self.addGestureRecognizer(&tap);
                self.ivars().doubletap_gesture_recognizer.replace(Some(tap));
            }
        } else if let Some(recognizer) = self.ivars().doubletap_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    pub(crate) fn recognize_rotation_gesture(&self, should_recognize: bool) {
        let mtm = MainThreadMarker::from(self);
        if should_recognize {
            if self.ivars().rotation_gesture_recognizer.borrow().is_none() {
                let rotation = unsafe {
                    UIRotationGestureRecognizer::initWithTarget_action(
                        mtm.alloc(),
                        Some(self),
                        Some(sel!(rotationGesture:)),
                    )
                };
                rotation.setDelegate(Some(ProtocolObject::from_ref(self)));
                self.addGestureRecognizer(&rotation);
                self.ivars().rotation_gesture_recognizer.replace(Some(rotation));
            }
        } else if let Some(recognizer) = self.ivars().rotation_gesture_recognizer.take() {
            self.removeGestureRecognizer(&recognizer);
        }
    }

    /// Touches on the screen. The Pencil's are sent as they come. Two-finger pinches and pans and
    /// the undo / redo taps are UIKit's gesture recognizers (`install_finger_gestures`): when one
    /// recognizes, UIKit cancels the fingers' touches, as iPadOS's own palm rejection does while
    /// the Pencil draws. egui can't take back a press, so fingers are held back a moment
    /// (`finger_touch`): those cancelled by then never reach the app.
    fn handle_touches(&self, touches: &NSSet<UITouch>, event: Option<&UIEvent>) {
        let Some(window) = self.window() else { return };
        let window_id = RootWindowId(window.id());
        let mut events = Vec::new();
        if let Some(event) = event {
            events.extend(self.sync_modifiers(window_id, unsafe { event.modifierFlags() }));
        }
        for touch in touches {
            // A trackpad's or a mouse's pointer (with UIApplicationSupportsIndirectInputEvents):
            // a mouse, with its buttons, not a finger.
            if touch.r#type() == UITouchType::IndirectPointer {
                self.pointer_touch(&mut events, window_id, touch, event);
                continue;
            }
            let phase = match touch.phase() {
                UITouchPhase::Began => TouchPhase::Started,
                UITouchPhase::Moved => TouchPhase::Moved,
                UITouchPhase::Ended => TouchPhase::Ended,
                UITouchPhase::Cancelled => TouchPhase::Cancelled,
                _ => continue,
            };
            if touch.r#type() == UITouchType::Pencil {
                events.push(self.touch_event(window_id, touch, phase, touch.locationInView(Some(self))));
            } else {
                self.finger_touch(&mut events, window_id, touch, phase);
            }
        }
        app_state::handle_nonuser_events(MainThreadMarker::from(self), events);
    }

    fn finger_touch(&self, events: &mut Vec<EventWrapper>, window_id: RootWindowId, touch: &UITouch, phase: TouchPhase) {
        let id = touch as *const UITouch as u64;
        let at = touch.locationInView(Some(self));
        let now = touch.timestamp();
        let mut fingers = self.ivars().fingers.borrow_mut();
        if phase == TouchPhase::Started {
            fingers.retain(|f| f.id != id);
            let together = !fingers.is_empty();
            for f in fingers.iter_mut().filter(|f| !f.sent) {
                f.together = true;
            }
            fingers.push(Finger { id, start: at, began_at: now, sent: false, together });
            return;
        }
        let Some(i) = fingers.iter().position(|f| f.id == id) else { return };
        if phase == TouchPhase::Moved {
            let f = &mut fingers[i];
            if f.sent {
                events.push(self.touch_event(window_id, touch, TouchPhase::Moved, at));
            } else if !f.together
                && ((at.x - f.start.x).hypot(at.y - f.start.y) > FINGER_SLOP || now - f.began_at > FINGER_DELAY)
            {
                f.sent = true;
                events.push(self.touch_event(window_id, touch, TouchPhase::Started, f.start));
                events.push(self.touch_event(window_id, touch, TouchPhase::Moved, at));
            }
            return;
        }
        let f = fingers.remove(i);
        drop(fingers);
        if f.sent {
            events.push(self.touch_event(window_id, touch, phase, at));
        } else if phase == TouchPhase::Ended && !f.together {
            // A tap: its press now, its lift a moment later.
            events.push(self.touch_event(window_id, touch, TouchPhase::Started, f.start));
            self.ivars().tap_ends.borrow_mut().push((id, at));
            let _: () = unsafe {
                msg_send![self, performSelector: sel!(finishTaps), withObject: std::ptr::null::<NSObject>(), afterDelay: TAP_LIFT_DELAY]
            };
        }
    }

    fn touch_event(&self, window_id: RootWindowId, touch: &UITouch, phase: TouchPhase, at: CGPoint) -> EventWrapper {
        let pencil = touch.r#type() == UITouchType::Pencil;
        let force = (app_state::os_capabilities().force_touch
            && (pencil
                || self.traitCollection().forceTouchCapability() == UIForceTouchCapability::Available))
            .then(|| Force::Calibrated {
                force: touch.force() as _,
                max_possible_force: touch.maximumPossibleForce() as _,
                altitude_angle: pencil.then(|| touch.altitudeAngle() as _),
            });
        window_event(window_id, WindowEvent::Touch(Touch {
            device_id: DEVICE_ID,
            id: touch as *const UITouch as u64,
            location: self.physical(at),
            force,
            phase,
        }))
    }

    /// Fingers only (never the Pencil or a trackpad): two pinch and pan the view, together; a tap
    /// of two undoes and of three redoes once the pinch and the pan have failed, as in Procreate's
    /// undo gesture sample (BSD-2-Clause).
    fn install_finger_gestures(&self, mtm: MainThreadMarker) {
        let fingers = NSArray::from_vec(vec![NSNumber::new_isize(UITouchType::Direct.0)]);
        let pinch: Retained<UIGestureRecognizer> = Retained::into_super(unsafe {
            UIPinchGestureRecognizer::initWithTarget_action(mtm.alloc(), Some(self), Some(sel!(fingerPinch:)))
        });
        let pan = unsafe {
            UIPanGestureRecognizer::initWithTarget_action(mtm.alloc(), Some(self), Some(sel!(fingerPan:)))
        };
        pan.setMinimumNumberOfTouches(2);
        let pan: Retained<UIGestureRecognizer> = Retained::into_super(pan);
        let tap = |action, touches| {
            let tap = unsafe { UITapGestureRecognizer::initWithTarget_action(mtm.alloc(), Some(self), Some(action)) };
            tap.setNumberOfTouchesRequired(touches);
            for manipulation in [&pinch, &pan] {
                unsafe { tap.requireGestureRecognizerToFail(manipulation) };
            }
            let tap: Retained<UIGestureRecognizer> = Retained::into_super(tap);
            tap
        };
        let undo = tap(sel!(undoTap:), 2);
        let redo = tap(sel!(redoTap:), 3);
        for recognizer in [&pinch, &pan, &undo, &redo] {
            unsafe { recognizer.setAllowedTouchTypes(&fingers) };
            recognizer.setDelegate(Some(ProtocolObject::from_ref(self)));
            self.addGestureRecognizer(recognizer);
        }
        self.ivars().finger_gestures.borrow_mut().extend([pinch, pan, undo, redo]);
    }

    fn tap_shortcut(&self, recognizer: &UITapGestureRecognizer, redo: bool) {
        if recognizer.state() != UIGestureRecognizerState::Ended {
            return;
        }
        let Some(window) = self.window() else { return };
        let mut events = Vec::new();
        self.undo_shortcut(&mut events, RootWindowId(window.id()), redo);
        app_state::handle_nonuser_events(MainThreadMarker::from(self), events);
    }

    /// A tap of two fingers undoes, one of three redoes, as in drawing apps (UIKit cancels the
    /// fingers' touches when the tap recognizes): sent as ⌘Z and ⇧⌘Z, which
    /// both apps bind.
    fn undo_shortcut(&self, events: &mut Vec<EventWrapper>, window_id: RootWindowId, redo: bool) {
        let before = self.ivars().modifiers.get();
        let held = if redo { ModifiersState::SUPER | ModifiersState::SHIFT } else { ModifiersState::SUPER };
        events.extend(self.set_modifiers(window_id, held));
        for state in [ElementState::Pressed, ElementState::Released] {
            events.push(window_event(window_id, WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                event: KeyEvent {
                    physical_key: PhysicalKey::Code(KeyCode::KeyZ),
                    logical_key: Key::Character(if redo { "Z" } else { "z" }.into()),
                    text: None,
                    location: KeyLocation::Standard,
                    state,
                    repeat: false,
                    platform_specific: KeyEventExtra {},
                },
                is_synthetic: false,
            }));
        }
        events.extend(self.set_modifiers(window_id, before));
    }

    fn handle_insert_text(&self, text: &NSString) {
        let window = self.window().unwrap();
        let window_id = RootWindowId(window.id());
        let mtm = MainThreadMarker::new().unwrap();
        // send individual events for each character
        app_state::handle_nonuser_events(
            mtm,
            text.to_string().chars().flat_map(|c| {
                let text = smol_str::SmolStr::from_iter([c]);
                // Emit both press and release events
                [ElementState::Pressed, ElementState::Released].map(|state| {
                    EventWrapper::StaticEvent(Event::WindowEvent {
                        window_id,
                        event: WindowEvent::KeyboardInput {
                            event: KeyEvent {
                                text: if state == ElementState::Pressed {
                                    Some(text.clone())
                                } else {
                                    None
                                },
                                state,
                                location: KeyLocation::Standard,
                                repeat: false,
                                logical_key: Key::Character(text.clone()),
                                physical_key: PhysicalKey::Unidentified(
                                    NativeKeyCode::Unidentified,
                                ),
                                platform_specific: KeyEventExtra {},
                            },
                            is_synthetic: false,
                            device_id: DEVICE_ID,
                        },
                    })
                })
            }),
        );
    }

    fn handle_delete_backward(&self) {
        let window = self.window().unwrap();
        let window_id = RootWindowId(window.id());
        let mtm = MainThreadMarker::new().unwrap();
        app_state::handle_nonuser_events(
            mtm,
            [ElementState::Pressed, ElementState::Released].map(|state| {
                EventWrapper::StaticEvent(Event::WindowEvent {
                    window_id,
                    event: WindowEvent::KeyboardInput {
                        device_id: DEVICE_ID,
                        event: KeyEvent {
                            state,
                            logical_key: Key::Named(NamedKey::Backspace),
                            physical_key: PhysicalKey::Code(KeyCode::Backspace),
                            platform_specific: KeyEventExtra {},
                            repeat: false,
                            location: KeyLocation::Standard,
                            text: None,
                        },
                        is_synthetic: false,
                    },
                })
            }),
        );
    }
}
