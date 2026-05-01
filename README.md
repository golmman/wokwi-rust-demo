# wokwi-rust-demo


## Prerequisites

```sh
rustup target add thumbv6m-none-eabi
cargo install elf2uf2-rs
```

## Build
```sh
cargo build --release
elf2uf2-rs target/thumbv6m-none-eabi/release/wokwi-test target/thumbv6m-none-eabi/release/wokwi-test.uf2
```

## Test with Wokwi

* Open the `diagram.json` file in [Wokwi](https://wokwi.com/)
* Click on "Run" to start the simulation

## Hardware Wiring

Pin assignments (all on the Raspberry Pi Pico):

| Pico pin (physical) | GPIO  | Connects to               | Notes                                       |
| ------------------- | ----- | ------------------------- | ------------------------------------------- |
| 36                  | 3V3   | MAX7219 VCC, DCF77 VCC    | onboard LDO, ~300 mA budget                 |
| 40                  | VBUS  | MAX7219 V+ (LED supply)   | 5V from USB                                 |
| 38 (or any GND)     | GND   | MAX7219 GND, DCF77 GND, button | shared ground                          |
| 24                  | GP18  | MAX7219 CLK               | SPI0 SCK                                    |
| 25                  | GP19  | MAX7219 DIN               | SPI0 MOSI                                   |
| 22                  | GP17  | MAX7219 CS                | chip select                                 |
| 20                  | GP15  | push-button               | active-low, internal pull-up                |
| 19                  | GP14  | DCF77 receiver DATA       | active-LOW pulses, internal pull-up         |
| (unused)            | GP13  | —                         | reserved for the simulator-only loopback TX; leave unconnected on real hardware |

The MAX7219 chain length (`4` modules) is hard-coded in `src/config.rs::CHAIN_LEN`. Change there and in `diagram.json` together if you add or remove modules.

### DCF77 receiver

The firmware reads a single digital signal on **GP14** (physical pin 19). With no receiver wired, the internal pull-up keeps the line idle-HIGH and the decoder stays silent — the clock just ticks from `INITIAL_TIME` (`12:34:56` by default in `src/config.rs`).

Three wires are all you need:

| Receiver module pin | Pico pin                | Notes                                           |
| ------------------- | ----------------------- | ----------------------------------------------- |
| VCC / V+            | 3V3 (pin 36)            | most modules accept 3-5V; ~1 mA draw            |
| GND                 | any GND (e.g. pin 38)   | shared ground                                   |
| DATA / TCO          | GP14 (pin 19)           | the signal pin the firmware polls every 10 ms   |

If your module has an additional **PON / EN / PDN** (power-down or enable) pin, tie it to whichever rail your datasheet specifies — usually GND. A floating enable pin is the most common reason for "module powered, but no pulses".

The default `cargo build --release` is what you want for production. The `dcf77-loopback` Cargo feature is only for the simulator (it drives GP13 with a synthetic DCF77 broadcast that's wired to GP14 in `diagram.dcf77.json`); leaving it off saves ~1 KB of flash and avoids needlessly toggling GP13 on real hardware.

#### Polarity

The decoder assumes the most common convention: idle-HIGH, ~100/200 ms LOW pulses (HKW DCF1, Conrad/DCF1, C-MAX CMMR-6P-60). If your module's output is inverted you'll see no decoding — the fix is one line in `src/main.rs::dcf77_sample`: change `dcf77_in.is_high()` to `!dcf77_in.is_high()`.

#### RF placement

DCF77 is a 77.5 kHz longwave signal at the level of nanowatts at the receiver's antenna — extremely sensitive to interference. The MAX7219 chain in this project is essentially a switching-noise generator right next to where the receiver wants to listen. To get reliable reception:

* Keep the receiver's ferrite antenna **at least 30-50 cm** from the LED matrices and the SPI wires (GP17/18/19).
* Keep it away from USB cables, switching power supplies, and any 2.4 GHz radios.
* Add a 100 nF ceramic decoupling capacitor across VCC/GND at the module's pins.
* For best reliability, consider a separate 3V3 LDO fed from VBUS rather than the Pico's onboard 3V3 — keeps the receiver isolated from supply ripple caused by the LED matrices.

#### Verifying the wiring

Two quick sanity checks before assuming a software issue:

1. **Idle level:** GP14 should sit at 3.3V most of the time. If it stays at 0V, suspect a missing pull-up, an unpowered module, or a misconfigured PON/EN pin.
2. **Pulse train:** a working receiver outputs one ~100-200 ms LOW pulse per second, plus a ~1.8 s HIGH gap once per minute (the bit-59 minute marker). Easiest to confirm with a logic analyzer or scope on GP14. If you see no pulses at all, RF noise from the matrix is the most likely cause — power off the matrix temporarily and re-test.

Once those pass, the firmware will pick up sync within ~2 minutes of boot, at which point the displayed time jumps to the broadcast value and the boot-time `INITIAL_TIME` stops mattering.

## Deploy to Pico 2040

### Without Picotool

Get the pico into bootsel mode by holding down the boot button **while** plugging in the USB cable.

```sh
cp target/thumbv6m-none-eabi/release/wokwi-test.uf2 /run/media/dirk/RPI-RP2/
```

### With Picotool

#### Prerequisites (Fedora)

see https://github.com/raspberrypi/picotool/blob/master/BUILDING.md#building

```sh
sudo dnf install @development-tools pkgconf-pkg-config libusb1-devel cmake
git clone https://github.com/raspberrypi/pico-sdk.git
git clone https://github.com/raspberrypi/picotool.git
cd picotool
mkdir build
cd build
export PICO_SDK_PATH=../../pico-sdk
cmake ..
make
sudo make install
```

#### Deploy

```sh
sudo picotool load -f target/thumbv6m-none-eabi/release/wokwi-test.uf2
```

