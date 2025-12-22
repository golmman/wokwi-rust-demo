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

