//! Native notifications, posted as the bundled app rather than as a script.
//!
//! `osascript`-driven notifications wear Script Editor's icon and cannot
//! carry buttons. `UNUserNotificationCenter` gives both, at the price of
//! requiring a bundle identity — so every entry point here degrades to
//! [`crate::platform::notify`] when monk is running outside its bundle.
//!
//! Objective-C interop is unsafe by construction: subclassing NSObject and
//! sending `init` to a freshly allocated instance have no safe spelling.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSArray, NSBundle, NSObject, NSObjectProtocol, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

/// Category the buttons hang off. A notification only shows actions when its
/// content names a category that was registered up front.
const SESSION_CATEGORY: &str = "session";
/// Same notification minus the stop button, for a session that cannot be
/// stopped.
const HARD_CATEGORY: &str = "session.hard";
const EXTEND_MINUTES: u64 = 15;
const ACTION_EXTEND: &str = "session.extend";
const ACTION_STOP: &str = "session.stop";

/// What the user pressed on a delivered notification.
#[derive(Debug, Clone, Copy)]
pub enum Action {
    Extend(Duration),
    Stop,
}

/// Whether a notification should offer the session buttons, and whether it
/// deserves a sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A session is running: offer "add time" and "stop".
    Session,
    /// A hard-mode session: offer "add time" only. Stop is refused by the
    /// daemon by design, and a button that always fails is worse than none.
    HardSession,
    /// Nothing to act on — the session already ended, or the click failed.
    Plain,
}

impl Kind {
    fn category(self) -> Option<&'static str> {
        match self {
            Kind::Session => Some(SESSION_CATEGORY),
            Kind::HardSession => Some(HARD_CATEGORY),
            Kind::Plain => None,
        }
    }
}

pub struct Notifier {
    center: Option<Retained<UNUserNotificationCenter>>,
    /// Set once macOS confirms the user allowed notifications. Until then —
    /// and forever, if they said no — posting through the center would go
    /// nowhere silently, so the scripted path takes over.
    authorized: Arc<AtomicBool>,
    // Held for as long as the app runs: the center keeps only a weak
    // reference, and a dropped delegate silently stops the buttons working.
    _delegate: Option<Retained<Delegate>>,
}

impl Notifier {
    /// Sets up the notification center, asks for permission and registers the
    /// session buttons. Outside the app bundle this returns a notifier that
    /// quietly falls back to the scripted path — `currentNotificationCenter`
    /// aborts the process when there is no bundle to attribute it to.
    pub fn new(mtm: MainThreadMarker, on_action: impl Fn(Action) + 'static) -> Self {
        // Both checks matter: the path says which binary is running, the
        // identity says whether macOS agrees it is an app. Asking the
        // notification center for its instance without one does not fail —
        // it aborts the process.
        if !super::bundle::is_bundled() || !has_bundle_identity() {
            tracing::info!(
                "running outside monk.app: notifications fall back to osascript. \
                 run `monk menubar install` for the bundled experience"
            );
            return Self {
                center: None,
                authorized: Arc::new(AtomicBool::new(false)),
                _delegate: None,
            };
        }

        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate = Delegate::new(mtm, Box::new(on_action));
        let proto = ProtocolObject::from_ref(&*delegate);
        center.setDelegate(Some(proto));

        let authorized = Arc::new(AtomicBool::new(false));
        let flag = authorized.clone();
        let granted = RcBlock::new(
            move |granted: objc2::runtime::Bool, error: *mut objc2_foundation::NSError| {
                flag.store(granted.as_bool(), Ordering::SeqCst);
                if !granted.as_bool() {
                    tracing::warn!(
                        "notifications are not allowed for monk; falling back to scripted \
                         banners. Turn them on in System Settings > Notifications > monk"
                    );
                }
                if !error.is_null() {
                    tracing::warn!("notification authorization returned an error");
                }
            },
        );
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &granted,
        );

        center.setNotificationCategories(&categories());
        Self { center: Some(center), authorized, _delegate: Some(delegate) }
    }

    pub fn post(&self, title: &str, body: &str, kind: Kind) {
        let Some(center) = self.center.as_ref().filter(|_| self.authorized.load(Ordering::SeqCst))
        else {
            crate::platform::notify(title, body);
            return;
        };
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        if let Some(category) = kind.category() {
            content.setCategoryIdentifier(&NSString::from_str(category));
        }
        let id = NSString::from_str(&uuid::Uuid::new_v4().to_string());
        let request =
            UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        center.addNotificationRequest_withCompletionHandler(&request, None);
    }
}

/// Whether macOS resolved an enclosing bundle for this process, and it is
/// ours. `UNUserNotificationCenter` is unusable — fatally so — without one.
fn has_bundle_identity() -> bool {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .is_some_and(|id| id.to_string() == super::bundle::BUNDLE_ID)
}

fn categories() -> Retained<NSSet<UNNotificationCategory>> {
    let minutes = format!("{EXTEND_MINUTES}m");
    let extend_title = crate::i18n::t!("menubar.notify_action_extend", duration = minutes);
    let extend = UNNotificationAction::actionWithIdentifier_title_options(
        &NSString::from_str(ACTION_EXTEND),
        &NSString::from_str(&extend_title),
        UNNotificationActionOptions::empty(),
    );
    let stop = UNNotificationAction::actionWithIdentifier_title_options(
        &NSString::from_str(ACTION_STOP),
        &NSString::from_str(&crate::i18n::t!("menubar.stop")),
        UNNotificationActionOptions::Destructive,
    );
    let soft = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
        &NSString::from_str(SESSION_CATEGORY),
        &NSArray::from_retained_slice(&[extend.clone(), stop]),
        &NSArray::from_retained_slice(&[]),
        UNNotificationCategoryOptions::empty(),
    );
    let hard = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
        &NSString::from_str(HARD_CATEGORY),
        &NSArray::from_retained_slice(&[extend]),
        &NSArray::from_retained_slice(&[]),
        UNNotificationCategoryOptions::empty(),
    );
    NSSet::from_retained_slice(&[soft, hard])
}

struct DelegateIvars {
    on_action: Box<dyn Fn(Action)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MonkNotificationDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        // Without this, a notification posted while monk is frontmost is
        // swallowed. A menu bar accessory is never really "frontmost", but
        // the system asks all the same.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::Sound
                | UNNotificationPresentationOptions::List,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let action = match response.actionIdentifier().to_string().as_str() {
                ACTION_EXTEND => Some(Action::Extend(Duration::from_secs(EXTEND_MINUTES * 60))),
                ACTION_STOP => Some(Action::Stop),
                _ => None,
            };
            if let Some(action) = action {
                (self.ivars().on_action)(action);
            }
            handler.call(());
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, on_action: Box<dyn Fn(Action)>) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(DelegateIvars { on_action });
        unsafe { msg_send![super(this), init] }
    }
}
