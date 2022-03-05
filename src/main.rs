use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::Duration;

use esp_idf_hal::{gpio::Pin, peripherals::Peripherals};
use esp_idf_sys::{self, esp, EspError};

#[no_mangle]
#[inline(never)]
#[link_section = ".iram1"]
pub fn irq_handler(irq_triggered: &mut AtomicBool) {
    irq_triggered.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn main() {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.
    esp_idf_sys::link_patches();

    enable_isr_service().unwrap();

    let mut irq_triggered: AtomicBool = AtomicBool::new(false);

    let peripherals = Peripherals::take().unwrap();
    activate_configure_irq(peripherals.pins.gpio35, irq_handler, &mut irq_triggered).unwrap();

    loop {
        if irq_triggered.load(std::sync::atomic::Ordering::SeqCst) {
            println!("Triggered !");
            irq_triggered.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        thread::sleep(Duration::from_millis(500));
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

    esp!(unsafe {
        esp_idf_sys::gpio_isr_handler_add(
            pin,
            Some(std::mem::transmute::<
                fn(&mut E),
                extern "C" fn(*mut esp_idf_sys::c_types::c_void),
            >(callback)),
            std::mem::transmute::<&mut E, *mut esp_idf_sys::c_types::c_void>(context),
        )
    })
}
