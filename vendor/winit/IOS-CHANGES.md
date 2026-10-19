# winit 0.30.13, patched for iPadOS

A copy of winit 0.30.13 (crates.io, Apache-2.0) used through `[patch.crates-io]` in the workspace
`Cargo.toml`. Only the iOS backend (`src/platform_impl/ios/`) and `Cargo.toml` (UIKit and
Foundation features) differ from the release; every other platform builds the code as released.

All APIs used are available from iOS 13.4, so the app keeps its iOS 16.0 minimum.

## Hardware keyboards (`keyboard.rs`, `view.rs`, `view_controller.rs`, `window.rs`)

- `pressesBegan/Ended/Cancelled` on the view and on the view controller send `KeyboardInput` with
  the physical key (USB HID usage), the logical key (the layout's character, as on macOS) and the
  text typed, and `ModifiersChanged` (⌘ is `SUPER`).
- The view controller is the first responder whenever the view isn't, so keys reach the app
  without the on-screen keyboard showing; the view becomes first responder while text is typed
  (`set_ime_allowed(true)`), and the controller again after (`set_ime_allowed(false)`).
- While text is typed, characters and Backspace go to iPadOS's text input (dead keys such as
  ^ + e, input methods, its own key repeat) and arrive through `insertText:` / `deleteBackward`.
- UIKit doesn't repeat held keys: a timer repeats the last key (0.42 s, then every 35 ms).

## Trackpads and mice (`view.rs`, `window.rs`)

- Pointer touches (`UITouchType::IndirectPointer`, with `UIApplicationSupportsIndirectInputEvents`)
  become `CursorMoved` + `MouseInput` (right button from `UIEvent.buttonMask`), not touches.
- `UIHoverGestureRecognizer`: `CursorEntered`, `CursorMoved` and `CursorLeft` while hovering.
- A pan recognizer limited to scrolls (no touches): `MouseWheel` with pixel deltas.
- A pinch recognizer that only acts on touch-less (trackpad) pinches: `PinchGesture`.
- `UIPointerInteraction`: `Window::set_cursor_visible(false)` hides the system pointer (the app
  draws its own cursor).

## Window size (`view.rs`, `window.rs`)

- The drawing's size (`Resized`, `inner_size`, `outer_size`) is the view's own bounds times its
  `contentScaleFactor`, the same coordinates as touches and the pointer. winit used the window's
  frame converted to screen space, minus the safe area: in a floating window (Stage Manager,
  tweaks such as Aerial) that frame is moved, scaled or inset, and the picture no longer lined up
  with where touches landed. `inner_position` is the outer one.

## Fingers and the Pencil (`view.rs`)

- Touches (fingers and the Pencil) are sent as they come. Palm rejection is iPadOS's own: while
  the Pencil draws, it cancels the palm's touches (`TouchPhase::Cancelled`), as Apple's
  TouchCanvas sample relies on.
- UIKit gesture recognizers, for fingers only (`allowedTouchTypes` = direct), as in Procreate's
  undo gesture sample (BSD-2-Clause): two fingers pinch and pan together, sent as a trackpad's
  `PinchGesture` and pixel `MouseWheel`, which the apps already zoom and scroll with; a tap of two
  fingers sends ⌘Z (undo) and one of three ⇧⌘Z (redo), once the pinch and the pan have failed.
  When a recognizer takes the fingers, UIKit cancels their touches.
- egui can't take a press back (a cancelled touch would still paint a dot), so a finger reaches
  the app only once it has moved 10 pt or stayed 0.12 s (a tap: when it lifts, its release 0.05 s
  later so egui sees a click). Cancelled before that, by a gesture or by palm rejection, it never
  does. Fingers that land together are never touches, only gestures.

## Frame rate (`app_state.rs`, `platform/ios.rs`)

- A frame gate caps redraws at 60 per second by default (`frame_gate`): on a ProMotion iPad every
  touch could redraw at 120 Hz, which doubles the CPU and GPU work while drawing for no gain in
  image editing. Requests that come too early are merged and issued at the next slot (the run
  loop's timer wakes up for them); an idle app requests nothing, so it is unaffected.
- `winit::platform::ios::set_max_frame_rate(fps)` changes the cap at run time: 60, or 0 / 120 for
  the display's own rate. PhotoCraft switches it from the title bar.

egui-winit 0.36.2 (`vendor/egui-winit`, MIT OR Apache-2.0) is patched in two places: ⌘ is Command
on iOS as on macOS (`ModifiersChanged`), and a cancelled touch releases the button (egui kept it
down after `PointerGone`).
