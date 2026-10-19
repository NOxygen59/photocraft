use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSSet};
use objc2_ui_kit::{
    UIDevice, UIInterfaceOrientationMask, UIPress, UIPressesEvent, UIRectEdge, UIResponder,
    UIStatusBarStyle, UIUserInterfaceIdiom, UIView, UIViewController,
};

use super::app_state::{self};
use super::view::WinitView;
use crate::event::ElementState;
use crate::platform::ios::{ScreenEdge, StatusBarStyle, ValidOrientations};
use crate::window::WindowAttributes;

pub struct ViewControllerState {
    prefers_status_bar_hidden: Cell<bool>,
    preferred_status_bar_style: Cell<UIStatusBarStyle>,
    prefers_home_indicator_auto_hidden: Cell<bool>,
    supported_orientations: Cell<UIInterfaceOrientationMask>,
    preferred_screen_edges_deferring_system_gestures: Cell<UIRectEdge>,
}

declare_class!(
    pub(crate) struct WinitViewController;

    unsafe impl ClassType for WinitViewController {
        #[inherits(UIResponder, NSObject)]
        type Super = UIViewController;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "WinitUIViewController";
    }

    impl DeclaredClass for WinitViewController {
        type Ivars = ViewControllerState;
    }

    unsafe impl WinitViewController {
        #[method(shouldAutorotate)]
        fn should_autorotate(&self) -> bool {
            true
        }

        #[method(prefersStatusBarHidden)]
        fn prefers_status_bar_hidden(&self) -> bool {
            self.ivars().prefers_status_bar_hidden.get()
        }

        #[method(preferredStatusBarStyle)]
        fn preferred_status_bar_style(&self) -> UIStatusBarStyle {
            self.ivars().preferred_status_bar_style.get()
        }

        #[method(prefersHomeIndicatorAutoHidden)]
        fn prefers_home_indicator_auto_hidden(&self) -> bool {
            self.ivars().prefers_home_indicator_auto_hidden.get()
        }

        #[method(supportedInterfaceOrientations)]
        fn supported_orientations(&self) -> UIInterfaceOrientationMask {
            self.ivars().supported_orientations.get()
        }

        #[method(preferredScreenEdgesDeferringSystemGestures)]
        fn preferred_screen_edges_deferring_system_gestures(&self) -> UIRectEdge {
            self.ivars()
                .preferred_screen_edges_deferring_system_gestures
                .get()
        }

        // Hardware keyboards: the view controller is the first responder whenever the view isn't
        // (the view is while text is typed, and shows the on-screen keyboard), so key presses
        // always reach the app without the on-screen keyboard showing.
        #[method(canBecomeFirstResponder)]
        fn can_become_first_responder(&self) -> bool {
            true
        }

        #[method(viewDidAppear:)]
        fn view_did_appear(&self, animated: bool) {
            let _: () = unsafe { msg_send![super(self), viewDidAppear: animated] };
            unsafe { self.becomeFirstResponder() };
        }

        #[method(pressesBegan:withEvent:)]
        fn presses_began(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.forward_presses(presses, event, ElementState::Pressed) {
                let _: () = unsafe { msg_send![super(self), pressesBegan: presses, withEvent: event] };
            }
        }

        #[method(pressesEnded:withEvent:)]
        fn presses_ended(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.forward_presses(presses, event, ElementState::Released) {
                let _: () = unsafe { msg_send![super(self), pressesEnded: presses, withEvent: event] };
            }
        }

        #[method(pressesCancelled:withEvent:)]
        fn presses_cancelled(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
            if !self.forward_presses(presses, event, ElementState::Released) {
                let _: () = unsafe { msg_send![super(self), pressesCancelled: presses, withEvent: event] };
            }
        }
    }
);

impl WinitViewController {
    fn forward_presses(
        &self,
        presses: &NSSet<UIPress>,
        event: Option<&UIPressesEvent>,
        state: ElementState,
    ) -> bool {
        let Some(view) = self.view() else { return false };
        if !view.isKindOfClass(WinitView::class()) {
            return false;
        }
        // SAFETY: checked just above.
        let view: Retained<WinitView> = unsafe { Retained::cast(view) };
        view.handle_presses(presses, event, state)
    }

    pub(crate) fn set_prefers_status_bar_hidden(&self, val: bool) {
        self.ivars().prefers_status_bar_hidden.set(val);
        self.setNeedsStatusBarAppearanceUpdate();
    }

    pub(crate) fn set_preferred_status_bar_style(&self, val: StatusBarStyle) {
        let val = match val {
            StatusBarStyle::Default => UIStatusBarStyle::Default,
            StatusBarStyle::LightContent => UIStatusBarStyle::LightContent,
            StatusBarStyle::DarkContent => UIStatusBarStyle::DarkContent,
        };
        self.ivars().preferred_status_bar_style.set(val);
        self.setNeedsStatusBarAppearanceUpdate();
    }

    pub(crate) fn set_prefers_home_indicator_auto_hidden(&self, val: bool) {
        self.ivars().prefers_home_indicator_auto_hidden.set(val);
        let os_capabilities = app_state::os_capabilities();
        if os_capabilities.home_indicator_hidden {
            self.setNeedsUpdateOfHomeIndicatorAutoHidden();
        } else {
            os_capabilities.home_indicator_hidden_err_msg("ignoring")
        }
    }

    pub(crate) fn set_preferred_screen_edges_deferring_system_gestures(&self, val: ScreenEdge) {
        let val = {
            assert_eq!(val.bits() & !ScreenEdge::ALL.bits(), 0, "invalid `ScreenEdge`");
            UIRectEdge(val.bits().into())
        };
        self.ivars().preferred_screen_edges_deferring_system_gestures.set(val);
        let os_capabilities = app_state::os_capabilities();
        if os_capabilities.defer_system_gestures {
            self.setNeedsUpdateOfScreenEdgesDeferringSystemGestures();
        } else {
            os_capabilities.defer_system_gestures_err_msg("ignoring")
        }
    }

    pub(crate) fn set_supported_interface_orientations(
        &self,
        mtm: MainThreadMarker,
        valid_orientations: ValidOrientations,
    ) {
        let mask = match (valid_orientations, UIDevice::currentDevice(mtm).userInterfaceIdiom()) {
            (ValidOrientations::LandscapeAndPortrait, UIUserInterfaceIdiom::Phone) => {
                UIInterfaceOrientationMask::AllButUpsideDown
            },
            (ValidOrientations::LandscapeAndPortrait, _) => UIInterfaceOrientationMask::All,
            (ValidOrientations::Landscape, _) => UIInterfaceOrientationMask::Landscape,
            (ValidOrientations::Portrait, UIUserInterfaceIdiom::Phone) => {
                UIInterfaceOrientationMask::Portrait
            },
            (ValidOrientations::Portrait, _) => {
                UIInterfaceOrientationMask::Portrait
                    | UIInterfaceOrientationMask::PortraitUpsideDown
            },
        };
        self.ivars().supported_orientations.set(mask);
        #[allow(deprecated)]
        UIViewController::attemptRotationToDeviceOrientation(mtm);
    }

    pub(crate) fn new(
        mtm: MainThreadMarker,
        window_attributes: &WindowAttributes,
        view: &UIView,
    ) -> Retained<Self> {
        // These are set properly below, we just to set them to something in the meantime.
        let this = mtm.alloc().set_ivars(ViewControllerState {
            prefers_status_bar_hidden: Cell::new(false),
            preferred_status_bar_style: Cell::new(UIStatusBarStyle::Default),
            prefers_home_indicator_auto_hidden: Cell::new(false),
            supported_orientations: Cell::new(UIInterfaceOrientationMask::All),
            preferred_screen_edges_deferring_system_gestures: Cell::new(UIRectEdge::empty()),
        });
        let this: Retained<Self> = unsafe { msg_send_id![super(this), init] };

        this.set_prefers_status_bar_hidden(
            window_attributes.platform_specific.prefers_status_bar_hidden,
        );

        this.set_preferred_status_bar_style(
            window_attributes.platform_specific.preferred_status_bar_style,
        );

        this.set_supported_interface_orientations(
            mtm,
            window_attributes.platform_specific.valid_orientations,
        );

        this.set_prefers_home_indicator_auto_hidden(
            window_attributes.platform_specific.prefers_home_indicator_hidden,
        );

        this.set_preferred_screen_edges_deferring_system_gestures(
            window_attributes.platform_specific.preferred_screen_edges_deferring_system_gestures,
        );

        this.setView(Some(view));

        this
    }
}
