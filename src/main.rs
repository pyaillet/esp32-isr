use std::{thread, sync::Arc};
use std::time::Duration;

use embedded_svc::event_bus::{EventBus, Postbox};
use esp_idf_hal::{gpio::Pin, peripherals::Peripherals};
use esp_idf_svc::{eventloop::{EspBackgroundEventLoop, EspBackgroundSubscription}, sysloop::EspSysLoopStack};
use esp_idf_sys::{self, esp, EspError};
use log::*;

mod event {
    use esp_idf_svc::eventloop::{EspTypedEventSource, EspTypedEventSerializer, EspEventPostData, EspTypedEventDeserializer, EspEventFetchData};
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

pub fn irq_handler(eventloop: &mut EspBackgroundEventLoop) {
    eventloop.post(&event::EventLoopMessage::new(0), None).unwrap();
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
    let sys_loop_stack = Arc::new(EspSysLoopStack::new()?);

    let _res = enable_isr_service();

    let (mut eventloop, _subscription) = init_eventloop().unwrap();

    let peripherals = Peripherals::take().unwrap();
    activate_configure_irq(peripherals.pins.gpio35, irq_handler, &mut eventloop).unwrap();

    loop {
        thread::sleep(Duration::from_millis(2000));
    }
}

fn enable_isr_service() -> Result<(), EspError> {
    esp!(unsafe { esp_idf_sys::gpio_install_isr_service(0) })
}

fn activate_configure_irq<P, E>(
    pin: P,
    callback: fn(&mut E),
    context: &mut E,
) -> Result<(), EspError>
where
    P: Pin,
{
    let pin: i32 = pin.pin();

    use esp_idf_sys::{
        gpio_int_type_t_GPIO_INTR_NEGEDGE, gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
        gpio_pullup_t_GPIO_PULLUP_DISABLE, GPIO_MODE_DEF_INPUT,
    };
    let gpio_isr_config = esp_idf_sys::gpio_config_t {
        mode: GPIO_MODE_DEF_INPUT,
        pull_up_en: gpio_pullup_t_GPIO_PULLUP_DISABLE,
        pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
        intr_type: gpio_int_type_t_GPIO_INTR_NEGEDGE,
        pin_bit_mask: 1 << pin,
    };
    esp!(unsafe { esp_idf_sys::rtc_gpio_deinit(pin) })?;
    esp!(unsafe { esp_idf_sys::gpio_config(&gpio_isr_config) })?;

    // Casting from Rust generic type to native C format
    let callback = unsafe {
        std::mem::transmute::<fn(&mut E), extern "C" fn(*mut esp_idf_sys::c_types::c_void)>(
            callback,
        )
    };

    // Casting from Rust generic type to native C format
    let context =
        unsafe { std::mem::transmute::<&mut E, *mut esp_idf_sys::c_types::c_void>(context) };

    esp!(unsafe { esp_idf_sys::gpio_isr_handler_add(pin, Some(callback), context) })
}
