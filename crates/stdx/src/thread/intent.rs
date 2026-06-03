#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreadIntent {
    Worker,
    LatencySensitive,
}

impl ThreadIntent {
    pub(super) fn apply_to_current_thread(self) {
        let class = thread_intent_to_qos_class(self);
        set_current_thread_qos_class(class);
    }

    pub(super) fn assert_is_used_on_current_thread(self) {
        if IS_QOS_AVAILABLE {
            let class = thread_intent_to_qos_class(self);
            assert_eq!(get_current_thread_qos_class(), Some(class));
        }
    }
}

use imp::QoSClass;

const IS_QOS_AVAILABLE: bool = imp::IS_QOS_AVAILABLE;

#[expect(clippy::semicolon_if_nothing_returned, reason = "thin wrapper")]
fn set_current_thread_qos_class(class: QoSClass) {
    imp::set_current_thread_qos_class(class)
}

fn get_current_thread_qos_class() -> Option<QoSClass> {
    imp::get_current_thread_qos_class()
}

fn thread_intent_to_qos_class(intent: ThreadIntent) -> QoSClass {
    imp::thread_intent_to_qos_class(intent)
}

#[cfg(target_vendor = "apple")]
mod imp {
    use super::ThreadIntent;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub(super) enum QoSClass {
        Background,
        Utility,
        UserInitiated,
        UserInteractive,
    }

    pub(super) const IS_QOS_AVAILABLE: bool = true;

    pub(super) fn set_current_thread_qos_class(class: QoSClass) {
        let c = match class {
            QoSClass::UserInteractive => libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE,
            QoSClass::UserInitiated => libc::qos_class_t::QOS_CLASS_USER_INITIATED,
            QoSClass::Utility => libc::qos_class_t::QOS_CLASS_UTILITY,
            QoSClass::Background => libc::qos_class_t::QOS_CLASS_BACKGROUND,
        };

        let code = unsafe { libc::pthread_set_qos_class_self_np(c, 0) };

        if code == 0 {
            return;
        }

        let errno = unsafe { *libc::__error() };

        match errno {
            libc::EPERM => {
                panic!("tried to set QoS of thread which has opted out of QoS (os error {errno})")
            }

            libc::EINVAL => {
                unreachable!(
                    "invalid qos_class_t value was passed to pthread_set_qos_class_self_np"
                )
            }

            _ => {
                unreachable!("`pthread_set_qos_class_self_np` returned unexpected error {errno}")
            }
        }
    }

    pub(super) fn get_current_thread_qos_class() -> Option<QoSClass> {
        let current_thread = unsafe { libc::pthread_self() };
        let mut qos_class_raw = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
        let code = unsafe {
            libc::pthread_get_qos_class_np(current_thread, &mut qos_class_raw, std::ptr::null_mut())
        };

        if code != 0 {
            let errno = unsafe { *libc::__error() };
            unreachable!("`pthread_get_qos_class_np` failed unexpectedly (os error {errno})");
        }

        match qos_class_raw {
            libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE => Some(QoSClass::UserInteractive),
            libc::qos_class_t::QOS_CLASS_USER_INITIATED => Some(QoSClass::UserInitiated),
            libc::qos_class_t::QOS_CLASS_DEFAULT => None,
            libc::qos_class_t::QOS_CLASS_UTILITY => Some(QoSClass::Utility),
            libc::qos_class_t::QOS_CLASS_BACKGROUND => Some(QoSClass::Background),

            libc::qos_class_t::QOS_CLASS_UNSPECIFIED => {
                panic!("tried to get QoS of thread which has opted out of QoS")
            }
        }
    }

    pub(super) fn thread_intent_to_qos_class(intent: ThreadIntent) -> QoSClass {
        match intent {
            ThreadIntent::Worker => QoSClass::Utility,
            ThreadIntent::LatencySensitive => QoSClass::UserInitiated,
        }
    }
}

#[cfg(not(target_vendor = "apple"))]
mod imp {
    use super::ThreadIntent;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub(super) enum QoSClass {
        Default,
    }

    pub(super) const IS_QOS_AVAILABLE: bool = false;

    pub(super) fn set_current_thread_qos_class(_: QoSClass) {}

    pub(super) fn get_current_thread_qos_class() -> Option<QoSClass> {
        None
    }

    pub(super) fn thread_intent_to_qos_class(_: ThreadIntent) -> QoSClass {
        QoSClass::Default
    }
}
