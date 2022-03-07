use std::time::Duration;
use std::{sync::Arc, thread};

use callback::UnsafeCallback;
use embedded_svc::event_bus::{EventBus, Postbox};
use esp_idf_hal::{gpio::Pin, peripherals::Peripherals};
use esp_idf_svc::{
    eventloop::{EspBackgroundEventLoop, EspBackgroundSubscription},
    sysloop::EspSysLoopStack,
};
use esp_idf_sys::{self, esp, EspError, gpio_isr_handler_remove};
use log::*;

mod callback {
    use esp_idf_sys::c_types;

    pub struct UnsafeCallback(*mut Box<dyn for<'a> FnMut() + 'static>);

    impl UnsafeCallback {
        #[allow(clippy::type_complexity)]
        pub fn from(boxed: &mut Box<Box<dyn for<'a> FnMut() + 'static>>) -> Self {
            Self(boxed.as_mut())
        }

        pub unsafe fn from_ptr(ptr: *mut c_types::c_void) -> Self {
            Self(ptr as *mut _)
        }

        pub fn as_ptr(&self) -> *mut c_types::c_void {
            self.0 as *mut _
        }

        pub unsafe fn call(&mut self) {
            let reference = self.0.as_mut().unwrap();

            (reference)();
        }
    }
}

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

struct PinNotifySubscription<P: Pin>(P, Box<Box<dyn for<'a> FnMut()>>);

impl<P: Pin> PinNotifySubscription<P> {
    pub fn subscribe(pin: P, callback: impl for<'a> FnMut() + 'static) -> Result<Self, EspError> {
        let pin_number: i32 = pin.pin();

        use esp_idf_sys::{
            gpio_int_type_t_GPIO_INTR_NEGEDGE, gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
            gpio_pullup_t_GPIO_PULLUP_DISABLE, GPIO_MODE_DEF_INPUT,
        };
        let gpio_isr_config = esp_idf_sys::gpio_config_t {
            mode: GPIO_MODE_DEF_INPUT,
            pull_up_en: gpio_pullup_t_GPIO_PULLUP_DISABLE,
            pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
            intr_type: gpio_int_type_t_GPIO_INTR_NEGEDGE,
            pin_bit_mask: 1 << pin_number,
        };
        esp!(unsafe { esp_idf_sys::rtc_gpio_deinit(pin_number) })?;
        esp!(unsafe { esp_idf_sys::gpio_config(&gpio_isr_config) })?;

        let callback: Box<dyn for<'a> FnMut() + 'static> = Box::new(callback);
        let mut callback = Box::new(callback);

        let unsafe_callback = callback::UnsafeCallback::from(&mut callback);

        esp!(unsafe {
            esp_idf_sys::gpio_isr_handler_add(
                pin_number,
                Some(irq_handler),
                unsafe_callback.as_ptr(),
            )
        })?;

        Ok(Self(pin, callback))
    }

    pub fn unsubscribe(self) -> Self {
        unsafe {
            gpio_isr_handler_remove(self.0.pin());
        }
        self
    }
}

unsafe extern "C" fn irq_handler(unsafe_callback: *mut esp_idf_sys::c_types::c_void) {
    let mut unsafe_callback = UnsafeCallback::from_ptr(unsafe_callback);
    unsafe_callback.call();
}

fn init_eventloop() -> Result<(EspBackgroundEventLoop, EspBackgroundSubscription), EspError> {
    info!("About to start a background event loop");
    let mut eventloop = EspBackgroundEventLoop::new(&Default::default())?;

    info!("About to subscribe to the background event loop");
    let subscription = eventloop.subscribe(|_ev: &event::EventLoopMessage| {
        info!("Got event from the event loop");
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

    let _res = enable_isr_service();

    let (mut eventloop, _subscription) = init_eventloop().unwrap();

    let peripherals = Peripherals::take().unwrap();
    let subscription = PinNotifySubscription::subscribe(peripherals.pins.gpio35, move || {
        eventloop
            .post(&event::EventLoopMessage::new(1), None)
            .unwrap();
    })
    .unwrap();

    loop {
        thread::sleep(Duration::from_millis(2000));
    }
    subscription.unsubscribe();
}

fn enable_isr_service() -> Result<(), EspError> {
    esp!(unsafe { esp_idf_sys::gpio_install_isr_service(0) })
}

