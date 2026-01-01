# FNIRSI USB Device Flasher

A command-line tool for flashing firmware to FNIRSI USB testing devices. Supports automatic device detection, multiple device models, and provides clear progress feedback during flashing operations.

The development process leveraged Claude Sonnet and Opus models extensively. The implementation is based on reverse-engineering the USB protocol used by the Windows version of the FNIRSI UsbTester application when communicating with FNB-58 devices. This analysis enabled the creation of a cross-platform command-line tool that faithfully reproduces the flashing functionality.

Being the initial release, it is based on the protocol of the FNB-58 USB tester and has only been verified using that device.

## Features

- 🔍 **Automatic Device Detection** - Scans and identifies connected FNIRSI devices
- 📱 **Multi-Device Support** - Extensible registry for multiple FNIRSI device models
- 🎯 **DFU Mode Detection** - Automatically detects if device is in bootloader mode
- 📊 **Progress Tracking** - Real-time progress bar during firmware flashing
- ✅ **Safety Confirmations** - Asks for user confirmation before flashing (unless `-y` flag used)
- 📋 **Clear Instructions** - Shows detailed instructions for entering DFU mode
- 🔐 **Native .ufn Support** - Flashes encrypted FNIRSI .ufn firmware files directly via HID protocol

## Supported Devices

Currently supported FNIRSI devices:

- **FNIRSI FNB-58** (AT32F403ACGT7)
  - Normal Mode: VID:PID `2e3c:5558`
  - DFU Mode: VID:PID `0483:0038`

More devices can be easily added to the registry in `src/device_registry.rs`.

## Installation

### Prerequisites

- Rust toolchain (1.70+)
- libusb (on macOS: `brew install libusb`)

### Building

```bash
cargo build --release
```

The binary will be available at `target/release/fnirsi-usbtester-flasher`.

## Usage

### Basic Commands

**List supported device models:**
```bash
fnirsi-usbtester-flasher --list-models
```

**Scan for connected FNIRSI devices:**
```bash
fnirsi-usbtester-flasher
```

**List all USB devices (with detailed information):**
```bash
fnirsi-usbtester-flasher --list
```

This will show detailed information for each USB device including:
- Bus and device numbers
- VID:PID (Vendor ID and Product ID)
- Manufacturer, product name, and serial number
- USB class code
- Number and types of interfaces (HID, Mass Storage, CDC, etc.)
- Endpoint information for each interface
- ⚡ marker for devices that can be flashed with this tool

Example output:
```
Bus 001 Device 008 VID:PID 2e3c:5558 - Class 00 ⚡
  └─ Manufacturer: FNIRSI
  └─ Product:      FNB-58
  └─ Serial:       07A580000000
  └─ Interfaces:   4 interface(s)
     Interface 0 - Class 0x08 (Mass Storage) - 2 endpoint(s)
     Interface 1 - Class 0x02 (CDC (Communications)) - 1 endpoint(s)
     Interface 2 - Class 0x0a (CDC-Data) - 2 endpoint(s)
     Interface 3 - Class 0x03 (HID (Human Interface)) - 2 endpoint(s)

Legend: ⚡ = Can be flashed with this tool
```

**Flash firmware (with confirmation):**
```bash
fnirsi-usbtester-flasher --firmware firmware.bin
```

**Flash firmware (skip confirmation):**
```bash
fnirsi-usbtester-flasher --firmware firmware.bin -y
```

**Flash firmware (with verbose debug output):**
```bash
fnirsi-usbtester-flasher --firmware firmware.bin -v
```

This will show detailed protocol information including:
- HID device enumeration details
- Request/response packet contents
- First 3 write packets with full byte dumps
- Finalize command responses

### Complete Flashing Workflow

1. **Connect your device in DFU mode:**
   - Disconnect the device from all power sources
   - Hold down the middle button (center of back/forward toggle)
   - While holding the button, connect USB cable to the "PC" port
   - Device should appear in DFU mode

2. **Verify device is detected:**
   ```bash
   fnirsi-usbtester-flasher
   ```
   
   You should see output like:
   ```
   🔍 Scanning USB devices...
   
   ● FNIRSI FNB-58 (VID:PID 0483:0038) - DFU/Bootloader Mode
     └─ Model: FNB-58
   ```

3. **Flash the firmware:**
   ```bash
   fnirsi-usbtester-flasher --firmware path/to/firmware.bin
   ```

4. **Wait for completion:**
   The tool will show a progress bar and automatically complete the flashing process.

5. **Device will reset automatically**

## Error Handling

### Device Not in DFU Mode

If you try to flash when the device is not in DFU mode, you'll see:

```
⚠️  Device is not in DFU/Bootloader mode!

📋 To enter DFU mode for your device:

  1. Disconnect from all power sources
  2. Hold down the middle button (center of back/forward toggle)
  3. Connect USB cable while still holding the button
     → Use Micro-USB cable in the top port labeled "PC"
  4. Device should now appear in DFU mode

After entering DFU mode, run this command again.
```

### No Device Found

If no FNIRSI device is detected:
```
❌ No FNIRSI devices found

💡 Make sure your device is connected via USB
💡 Use --list-models to see supported models
```

## Project Structure

```
fnirsi_usb_tester/
├── .github/                 # GitHub configuration
├── Cargo.toml              # Project dependencies and metadata
├── README.md               # This file
├── FIRMWARE_ANALYSIS.md    # Complete protocol documentation
├── LICENSE.txt             # MIT License
├── .gitignore              # Git exclusions
├── src/
    ├── main.rs            # CLI interface and main logic
    ├── device_registry.rs # Device definitions and registry
    └── flasher.rs         # HID protocol implementation for FNIRSI devices
```

## Technical Details

The FNIRSI FNB-58 uses a proprietary HID-based flashing protocol:
- **Interface**: HID (Interface 3) with endpoints 0x03 (OUT) and 0x83 (IN)
- **Protocol**: Custom command/response over 64-byte HID reports
- **Firmware Format**: Encrypted .ufn files (flashed as-is, no decryption needed)
- **Transfer**: 58-byte payload per packet, ~6,714 packets for 389KB firmware
- **Commands**: Device info (0x26), Erase (0x28), Write (0x2b), Status (0xaa)

## Adding New Devices

To add support for a new FNIRSI device:

1. Open `src/device_registry.rs`
2. Add a new entry in `DeviceRegistry::new()`:

```rust
devices.push(DeviceInfo {
    vendor_id: 0x2e3c,        // Normal mode VID
    product_id: 0x5559,       // Normal mode PID
    name: "FNIRSI FNB-48".to_string(),
    model: "FNB-48".to_string(),
    dfu_vendor_id: Some(0x0483),   // DFU mode VID
    dfu_product_id: Some(0x0038),  // DFU mode PID
});
```

3. Rebuild the application

## Command-Line Options

```
Options:
  -f, --firmware <FILE>  Firmware file to flash
  -y, --yes              Skip confirmation prompt
  -l, --list             List all connected USB devices
      --list-models      List supported FNIRSI device models
  -h, --help             Print help
  -V, --version          Print version
```

## Troubleshooting

### Permission Denied Errors (Linux)

On Linux, you may need to run with `sudo` or set up udev rules:

```bash
# Create udev rule
sudo nano /etc/udev/rules.d/99-fnirsi.rules
```

Add:
```
SUBSYSTEM=="usb", ATTR{idVendor}=="2e3c", ATTR{idProduct}=="5558", MODE="0666"
SUBSYSTEM=="usb", ATTR{idVendor}=="0483", ATTR{idProduct}=="0038", MODE="0666"
```

Then reload:
```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### macOS Permission Issues

On macOS, you may need to grant Terminal or your IDE access to USB devices in System Settings → Privacy & Security.

### Key Modules

- **`main.rs`**: Command-line interface, device detection, and user interaction
- **`flasher.rs`**: Core flashing implementation using hidapi for HID communication
  - Implements CRC-8-DARC checksum algorithm
  - Handles device initialization, erase, write, and finalize commands
  - Manages progress reporting
- **`device_registry.rs`**: Registry of supported FNIRSI devices with VID/PID mappings

## Development

The experimental code from the initial development is preserved in the `experiments/` directory.

To run the CLI in development mode:
```bash
cargo run -- --help
cargo run -- --list-models
sudo cargo run -- --firmware firmware/Fnb58V1.11.ufn
```

### Running Tests

```bash
cargo test
```

### Building Documentation

```bash
cargo doc --open
```

## Protocol Documentation

See [FIRMWARE_ANALYSIS.md](FIRMWARE_ANALYSIS.md) for complete documentation of:
- HID-based flashing protocol
- CRC-8-DARC checksum algorithm
- Packet structures and command sequences
- Platform-specific behavior differences

## License

MIT License

## Contributing

Contributions are welcome! Please feel free to submit pull requests or open issues for bugs and feature requests.

## Acknowledgments

- Built with [hidapi](https://github.com/ruabmbua/hidapi-rs) for cross-platform HID device communication
- CLI powered by [clap](https://github.com/clap-rs/clap)
- Progress bars by [indicatif](https://github.com/console-rs/indicatif)
- USB protocol analysis using USBPcap, Wireshark and tshark
