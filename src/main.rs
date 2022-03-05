use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::Duration;

use esp_idf_hal::peripherals::Peripherals;
use esp_idf_sys::{self, esp, EspError};

const GPIO_INTR: u8 = 35;

static IRQ_TRIGGERED: AtomicBool = AtomicBool::new(false);

#[no_mangle]
#[inline(never)]
#[link_section = ".iram1"]
pub fn irq_triggered(_: &mut ()) {
    IRQ_TRIGGERED.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn main() {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.
    esp_idf_sys::link_patches();

    enable_isr_service().unwrap();

    let _peripherals = Peripherals::take().unwrap();
    activate_configure_irq(GPIO_INTR, irq_triggered, &mut ()).unwrap();

    loop {
        if IRQ_TRIGGERED.load(std::sync::atomic::Ordering::SeqCst) {
            println!("Triggered !");
            IRQ_TRIGGERED.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn enable_isr_service() -> Result<(), EspError> {
    esp!(unsafe { esp_idf_sys::gpio_install_isr_service(0) })
}

fn activate_configure_irq<E>(
    pin: u8,
    callback: fn(&mut E),
    context: &mut E,
) -> Result<(), EspError> {
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
    esp!(unsafe { esp_idf_sys::rtc_gpio_deinit(pin.into()) })?;
    esp!(unsafe { esp_idf_sys::gpio_config(&gpio_isr_config) })?;

    esp!(unsafe {
        esp_idf_sys::gpio_isr_handler_add(
            pin.into(),
            Some(std::mem::transmute::<
                fn(&mut E),
                extern "C" fn(*mut esp_idf_sys::c_types::c_void),
            >(callback)),
            std::mem::transmute::<&mut E, *mut esp_idf_sys::c_types::c_void>(context),
        )
    })
}
