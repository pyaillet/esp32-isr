use std::time::Duration;
use std::{sync::Arc, thread};

use embedded_svc::event_bus::{EventBus, Postbox};

use esp_idf_hal::{gpio::{InterruptType, Pull}, peripherals::Peripherals};
use esp_idf_svc::{
    eventloop::{EspBackgroundEventLoop, EspBackgroundSubscription},
    sysloop::EspSysLoopStack,
};
use esp_idf_sys::EspError;

use log::*;

mod event {
    use esp_idf_svc::eventloop::{
        EspEventFetchData, EspEventPostData, EspTypedEventDeserializer, EspTypedEventSerializer,
        EspTypedEventSource,
    };
    use esp_idf_sys::c_types;

    #[derive(Copy, Clone, Debug)]
    pub struct EventLoopMessage(u8);

    impl EventLoopMessage {
        pub fn new(data: u8) -> Self {
            Self(data)
        }
    }

    impl EspTypedEventSource for EventLoopMessage {
        fn source() -> *const c_types::c_char {
            b"DEMO-SERVICE\0".as_ptr() as *const _
        }
    }

    impl EspTypedEventSerializer<EventLoopMessage> for EventLoopMessage {
        fn serialize<R>(
            event: &EventLoopMessage,
            f: impl for<'a> FnOnce(&'a EspEventPostData) -> R,
        ) -> R {
            f(&unsafe { EspEventPostData::new(Self::source(), Self::event_id(), event) })
        }
    }

    impl EspTypedEventDeserializer<EventLoopMessage> for EventLoopMessage {
        fn deserialize<R>(
            data: &EspEventFetchData,
            f: &mut impl for<'a> FnMut(&'a EventLoopMessage) -> R,
        ) -> R {
            f(unsafe { data.as_payload() })
        }
    }
}

fn init_eventloop() -> Result<(EspBackgroundEventLoop, EspBackgroundSubscription), EspError> {
    info!("About to start a background event loop");
    let mut eventloop = EspBackgroundEventLoop::new(&Default::default())?;

    info!("About to subscribe to the background event loop");
    let subscription = eventloop.subscribe(|ev: &event::EventLoopMessage| {
        info!("Got event from the event loop {:?}", ev);
    })?;

    Ok((eventloop, subscription))
}

fn main() -> Result<(), EspError> {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.
    esp_idf_sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    #[allow(unused)]
    let sys_loop_stack: Arc<EspSysLoopStack> = Arc::new(EspSysLoopStack::new()?);

    let (mut eventloop, _subscription) = init_eventloop().unwrap();

    let peripherals = Peripherals::take().unwrap();
    let interrupt_pin = peripherals
        .pins
        .gpio0
        .into_input()
        .unwrap()
        .into_pull_up()
        .unwrap();
    let mut eventloop2 = eventloop.clone();
    let subscribed = unsafe {
        interrupt_pin.into_subscribed(
            move || {
                eventloop
                    .post(&event::EventLoopMessage::new(1), None)
                    .unwrap();
            },
            InterruptType::NegEdge,
        )?
    };

    let input = subscribed.unsubscribe().unwrap();

    let _subscribed = unsafe {
        input.into_subscribed(
            move || {
                eventloop2
                    .post(&event::EventLoopMessage::new(2), None)
                    .unwrap();
            },
            InterruptType::NegEdge,
        )?

    };

    loop {
        thread::sleep(Duration::from_millis(2000));
    }
}
